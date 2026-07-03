use std::{fs, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub type ConfigState = Mutex<AppConfig>;

pub const DEFAULT_SYSTEM_PROMPT: &str = "你是翻译引擎：中文为主的文本译为英文，其余译为简体中文。单词和短语也必须翻译，单词需给出词性与常用义项（如 check → n. 检查 v. 核对）。只输出译文，不要解释，保留原文换行与格式。";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub system_prompt: String,
    pub temperature: f32,
    /// 是否同时调用微软免费机器翻译作为快速对照
    pub enable_machine: bool,
    /// 界面主题：system（跟随系统）/ dark / light
    pub theme: String,
    /// 截屏翻译快捷键，如 "Alt+W"；留空禁用
    pub snip_hotkey: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
            temperature: 0.3,
            enable_machine: true,
            theme: "light".into(),
            snip_hotkey: "Alt+W".into(),
        }
    }
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("config.json"))
}

pub fn load(app: &AppHandle) -> AppConfig {
    config_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app).ok_or("无法获取配置目录")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| format!("写入配置失败：{e}"))
}
