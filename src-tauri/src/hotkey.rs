use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use rdev::{Event, EventType, Key};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::{config::ConfigState, mt, snip, translator, HotkeyState};

/// 翻译请求代际号：连续触发时，让旧的流式任务停止向弹窗推送
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 相邻两次 Ctrl+C 的最大间隔
const TRIPLE_WINDOW: Duration = Duration::from_millis(600);
/// 防止误触超长文本消耗过多 token
const MAX_CHARS: usize = 5000;

/// 自定义组合键（截屏翻译），修饰键精确匹配避免误触发
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

/// 解析 "Alt+W"、"Ctrl+Shift+S" 形式的组合键；必须包含至少一个修饰键和一个字母/数字
pub fn parse_hotkey(s: &str) -> Option<Hotkey> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key = None;

    for part in s.split('+') {
        let part = part.trim().to_lowercase();
        match part.as_str() {
            "ctrl" | "control" if !ctrl => ctrl = true,
            "alt" if !alt => alt = true,
            "shift" if !shift => shift = true,
            _ => {
                if key.is_some() {
                    return None;
                }
                let mut chars = part.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                key = Some(char_to_key(c)?);
            }
        }
    }

    let key = key?;
    if !(ctrl || alt || shift) {
        return None;
    }
    Some(Hotkey {
        ctrl,
        alt,
        shift,
        key,
    })
}

fn char_to_key(c: char) -> Option<Key> {
    use Key::*;
    Some(match c {
        'a' => KeyA,
        'b' => KeyB,
        'c' => KeyC,
        'd' => KeyD,
        'e' => KeyE,
        'f' => KeyF,
        'g' => KeyG,
        'h' => KeyH,
        'i' => KeyI,
        'j' => KeyJ,
        'k' => KeyK,
        'l' => KeyL,
        'm' => KeyM,
        'n' => KeyN,
        'o' => KeyO,
        'p' => KeyP,
        'q' => KeyQ,
        'r' => KeyR,
        's' => KeyS,
        't' => KeyT,
        'u' => KeyU,
        'v' => KeyV,
        'w' => KeyW,
        'x' => KeyX,
        'y' => KeyY,
        'z' => KeyZ,
        '0' => Num0,
        '1' => Num1,
        '2' => Num2,
        '3' => Num3,
        '4' => Num4,
        '5' => Num5,
        '6' => Num6,
        '7' => Num7,
        '8' => Num8,
        '9' => Num9,
        _ => return None,
    })
}

enum Trigger {
    Translate,
    Snip,
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Trigger>();

    // 处理线程：键盘钩子回调内不能做耗时操作（Windows 会因超时移除钩子），
    // 触发信号丢到这里慢慢处理
    let app_worker = app.clone();
    std::thread::spawn(move || {
        while let Ok(trigger) = rx.recv() {
            match trigger {
                Trigger::Translate => handle_clipboard_trigger(&app_worker),
                Trigger::Snip => snip::open_overlay(&app_worker),
            }
        }
    });

    // 监听线程：rdev 被动监听全局键盘，不拦截按键，系统复制功能不受影响
    std::thread::spawn(move || {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut count: u32 = 0;
        let mut last = Instant::now();

        let result = rdev::listen(move |event: Event| match event.event_type {
            EventType::KeyPress(k) => {
                match k {
                    Key::ControlLeft | Key::ControlRight => ctrl = true,
                    Key::Alt | Key::AltGr => alt = true,
                    Key::ShiftLeft | Key::ShiftRight => shift = true,
                    _ => {}
                }

                if k == Key::KeyC && ctrl {
                    // 三连 Ctrl+C 翻译
                    let now = Instant::now();
                    if now.duration_since(last) <= TRIPLE_WINDOW {
                        count += 1;
                    } else {
                        count = 1;
                    }
                    last = now;
                    if count >= 3 {
                        count = 0;
                        let _ = tx.send(Trigger::Translate);
                    }
                } else if let Ok(guard) = app.state::<HotkeyState>().0.lock() {
                    // 截屏翻译快捷键
                    if let Some(hk) = *guard {
                        if k == hk.key && ctrl == hk.ctrl && alt == hk.alt && shift == hk.shift {
                            let _ = tx.send(Trigger::Snip);
                        }
                    }
                }
            }
            EventType::KeyRelease(k) => match k {
                Key::ControlLeft | Key::ControlRight => ctrl = false,
                Key::Alt | Key::AltGr => alt = false,
                Key::ShiftLeft | Key::ShiftRight => shift = false,
                _ => {}
            },
            _ => {}
        });

        if let Err(e) = result {
            eprintln!("键盘监听启动失败：{e:?}");
        }
    });
}

fn handle_clipboard_trigger(app: &AppHandle) {
    // 第三次 Ctrl+C 的系统复制是异步完成的，稍等再读剪贴板
    std::thread::sleep(Duration::from_millis(250));

    let Some(text) = read_clipboard_text() else {
        return;
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    translate_and_show(app, text);
}

/// 剪贴板翻译管线：弹出悬浮窗，机翻与 AI 双通道并行
pub fn translate_and_show(app: &AppHandle, text: String) {
    let text: String = text.chars().take(MAX_CHARS).collect();
    if text.trim().is_empty() {
        return;
    }

    let cfg = {
        let state = app.state::<ConfigState>();
        let guard = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.clone()
    };

    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let use_machine = cfg.enable_machine;

    show_popup_at_cursor(app);
    let _ = app.emit_to(
        "popup",
        "translate-start",
        serde_json::json!({ "text": text, "model": cfg.model, "machine": use_machine }),
    );

    if use_machine {
        let app_mt = app.clone();
        let text_mt = text.clone();
        tauri::async_runtime::spawn(async move {
            let result = mt::translate(&text_mt).await;
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            match result {
                Ok(translated) => {
                    let _ = app_mt.emit_to("popup", "mt-result", translated);
                }
                Err(msg) => {
                    let _ = app_mt.emit_to("popup", "mt-error", msg);
                }
            }
        });
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let result = translator::translate_stream(&cfg, &text, move |delta| {
            if GENERATION.load(Ordering::SeqCst) == generation {
                let _ = emitter.emit_to("popup", "translate-chunk", delta);
            }
        })
        .await;

        // 已被新的翻译请求取代，静默丢弃
        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        match result {
            Ok(full) => {
                let _ = app.emit_to("popup", "translate-done", full);
            }
            Err(msg) => {
                let _ = app.emit_to("popup", "translate-error", msg);
            }
        }
    });
}

/// 截屏翻译管线：截图直接交给视觉模型识别并翻译（图片输入不适用机翻通道）
pub fn translate_image_and_show(app: &AppHandle, png_base64: String) {
    let cfg = {
        let state = app.state::<ConfigState>();
        let guard = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.clone()
    };

    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    show_popup_at_cursor(app);
    let _ = app.emit_to(
        "popup",
        "translate-start",
        serde_json::json!({ "text": "屏幕截图 · 视觉识别翻译", "model": cfg.model, "machine": false }),
    );

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let result = translator::translate_image_stream(&cfg, &png_base64, move |delta| {
            if GENERATION.load(Ordering::SeqCst) == generation {
                let _ = emitter.emit_to("popup", "translate-chunk", delta);
            }
        })
        .await;

        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        match result {
            Ok(full) => {
                let _ = app.emit_to("popup", "translate-done", full);
            }
            Err(mut msg) => {
                // 4xx 大概率是模型不支持图片输入，附上排查提示
                if msg.starts_with("接口返回 4") {
                    msg.push_str(
                        "\n\n若提示不支持图片输入，请在设置中更换支持视觉的模型（如 gpt-5.5、qwen-vl-max、glm-4.5v）",
                    );
                }
                let _ = app.emit_to("popup", "translate-error", msg);
            }
        }
    });
}

/// 截屏失败等场景：复用悬浮窗展示错误
pub fn show_error_popup(app: &AppHandle, title: &str, msg: String) {
    let model = app
        .state::<ConfigState>()
        .lock()
        .map(|c| c.model.clone())
        .unwrap_or_default();

    // 作废进行中的流式任务，避免旧内容污染错误弹窗
    GENERATION.fetch_add(1, Ordering::SeqCst);

    show_popup_at_cursor(app);
    let _ = app.emit_to(
        "popup",
        "translate-start",
        serde_json::json!({ "text": title, "model": model, "machine": false }),
    );
    let _ = app.emit_to("popup", "translate-error", msg);
}

fn read_clipboard_text() -> Option<String> {
    // 剪贴板可能被其它进程短暂占用，小退避重试
    for _ in 0..3 {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                return Some(text);
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

fn show_popup_at_cursor(app: &AppHandle) {
    let Some(win) = app.get_webview_window("popup") else {
        return;
    };

    // 已显示时仅更新内容，保持用户调整后的位置，也避免重复 show 造成闪烁
    if win.is_visible().unwrap_or(false) {
        return;
    }

    if let Ok(cursor) = app.cursor_position() {
        let mut x = cursor.x + 16.0;
        let mut y = cursor.y + 20.0;

        // 贴近屏幕边缘时往回收，保证弹窗完整可见（多显示器按鼠标所在屏计算）
        if let (Ok(Some(monitor)), Ok(size)) =
            (app.monitor_from_point(cursor.x, cursor.y), win.outer_size())
        {
            let mpos = monitor.position();
            let msize = monitor.size();
            let (w, h) = (size.width as f64, size.height as f64);
            let max_x = mpos.x as f64 + msize.width as f64 - w - 8.0;
            let max_y = mpos.y as f64 + msize.height as f64 - h - 8.0;
            x = x.min(max_x).max(mpos.x as f64 + 8.0);
            y = y.min(max_y).max(mpos.y as f64 + 8.0);
        }

        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    let _ = win.show();
    let _ = win.set_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_hotkeys() {
        assert_eq!(
            parse_hotkey("Ctrl + Shift + S"),
            Some(Hotkey {
                ctrl: true,
                alt: false,
                shift: true,
                key: Key::KeyS,
            })
        );
        assert_eq!(
            parse_hotkey("alt+w"),
            Some(Hotkey {
                ctrl: false,
                alt: true,
                shift: false,
                key: Key::KeyW,
            })
        );
    }

    #[test]
    fn rejects_ambiguous_hotkeys() {
        assert!(parse_hotkey("W").is_none());
        assert!(parse_hotkey("Alt+W+X").is_none());
        assert!(parse_hotkey("Alt+Alt+W").is_none());
        assert!(parse_hotkey("Ctrl+F1").is_none());
    }
}
