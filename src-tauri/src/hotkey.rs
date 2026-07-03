use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use rdev::{Event, EventType, Key};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::{config::ConfigState, mt, translator};

/// 翻译请求代际号：连续触发时，让旧的流式任务停止向弹窗推送
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 相邻两次 Ctrl+C 的最大间隔
const TRIPLE_WINDOW: Duration = Duration::from_millis(600);
/// 防止误触超长文本消耗过多 token
const MAX_CHARS: usize = 5000;

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();

    // 处理线程：键盘钩子回调内不能做耗时操作（Windows 会因超时移除钩子），
    // 触发信号丢到这里慢慢处理
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            handle_trigger(&app);
        }
    });

    // 监听线程：rdev 被动监听全局键盘，不拦截按键，系统复制功能不受影响
    std::thread::spawn(move || {
        let mut ctrl_down = false;
        let mut count: u32 = 0;
        let mut last = Instant::now();

        let result = rdev::listen(move |event: Event| match event.event_type {
            EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => {
                ctrl_down = true;
            }
            EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => {
                ctrl_down = false;
            }
            EventType::KeyPress(Key::KeyC) if ctrl_down => {
                let now = Instant::now();
                if now.duration_since(last) <= TRIPLE_WINDOW {
                    count += 1;
                } else {
                    count = 1;
                }
                last = now;
                if count >= 3 {
                    count = 0;
                    let _ = tx.send(());
                }
            }
            _ => {}
        });

        if let Err(e) = result {
            eprintln!("键盘监听启动失败：{e:?}");
        }
    });
}

fn handle_trigger(app: &AppHandle) {
    // 第三次 Ctrl+C 的系统复制是异步完成的，稍等再读剪贴板
    std::thread::sleep(Duration::from_millis(250));

    let Some(text) = read_clipboard_text() else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let text: String = text.chars().take(MAX_CHARS).collect();

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
        serde_json::json!({ "text": text, "model": cfg.model, "machine": cfg.enable_machine }),
    );

    // 机器翻译通道：与 AI 并行，先出结果作对照
    if cfg.enable_machine {
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

    if let Ok(cursor) = app.cursor_position() {
        let mut x = cursor.x + 16.0;
        let mut y = cursor.y + 20.0;

        // 贴近屏幕边缘时往回收，保证弹窗完整可见（多显示器按鼠标所在屏计算）
        if let (Ok(Some(monitor)), Ok(size)) = (
            app.monitor_from_point(cursor.x, cursor.y),
            win.outer_size(),
        ) {
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
