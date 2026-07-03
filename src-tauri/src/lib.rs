mod config;
mod hotkey;
mod mt;
mod translator;
mod tray;

use std::sync::Mutex;

use config::{AppConfig, ConfigState};
use tauri::{Emitter, Manager};

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
    config::save(&app, &config)?;
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
            get_default_prompt
        ])
        .setup(|app| {
            let cfg = config::load(app.handle());
            let need_setup = cfg.api_key.trim().is_empty();
            app.manage::<ConfigState>(Mutex::new(cfg));

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
            // 悬浮窗失焦自动收起
            tauri::WindowEvent::Focused(false) if window.label() == "popup" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
