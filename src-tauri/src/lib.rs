mod config;
mod hotkey;
mod mt;
mod snip;
mod translator;
mod tray;

use std::sync::Mutex;

use config::{AppConfig, ConfigState};
use tauri::{Emitter, Manager};

/// 截屏翻译快捷键的运行时状态，保存配置时热更新
pub struct HotkeyState(pub Mutex<Option<hotkey::Hotkey>>);

#[tauri::command]
fn get_config(state: tauri::State<'_, ConfigState>) -> AppConfig {
    state.lock().map(|c| c.clone()).unwrap_or_default()
}

#[tauri::command]
fn save_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, ConfigState>,
    config: AppConfig,
) -> Result<(), String> {
    // 先校验快捷键格式，避免坏配置落盘；留空表示禁用截屏翻译
    let snip_hotkey = config.snip_hotkey.trim();
    let parsed_hotkey = if snip_hotkey.is_empty() {
        None
    } else {
        Some(
            hotkey::parse_hotkey(snip_hotkey)
                .ok_or("截屏快捷键格式无效，示例：Alt+W、Ctrl+Shift+S（留空禁用）")?,
        )
    };

    config::save(&app, &config)?;
    if let Ok(mut hk) = app.state::<HotkeyState>().0.lock() {
        *hk = parsed_hotkey;
    }
    // 广播主题变化，悬浮窗即时切换
    let _ = app.emit("theme-changed", config.theme.clone());
    if let Ok(mut current) = state.lock() {
        *current = config;
    }
    Ok(())
}

#[tauri::command]
fn get_default_prompt() -> &'static str {
    config::DEFAULT_SYSTEM_PROMPT
}

#[tauri::command]
async fn test_translate(
    state: tauri::State<'_, ConfigState>,
    text: String,
) -> Result<String, String> {
    let cfg = state.lock().map_err(|_| "读取配置失败")?.clone();
    translator::translate_stream(&cfg, &text, |_| {}).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 重复启动时唤起已有实例的设置窗口
            tray::show_settings(app);
        }))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            test_translate,
            get_default_prompt,
            snip::snip_capture
        ])
        .setup(|app| {
            let cfg = config::load(app.handle());
            let need_setup = cfg.api_key.trim().is_empty();
            let parsed_hotkey = hotkey::parse_hotkey(cfg.snip_hotkey.trim());
            app.manage::<ConfigState>(Mutex::new(cfg));
            app.manage(HotkeyState(Mutex::new(parsed_hotkey)));
            app.manage(snip::SnipState(Mutex::new(None)));

            tray::create(app.handle())?;
            hotkey::start(app.handle().clone());

            // 首次使用尚未配置 API Key 时，直接打开设置页引导配置
            if need_setup {
                tray::show_settings(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // 关闭窗口只是隐藏，应用常驻托盘
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // 截屏遮罩失焦自动取消；翻译结果保持显示，避免阅读或生成过程中误收起
            tauri::WindowEvent::Focused(false) if window.label() == "snip" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
