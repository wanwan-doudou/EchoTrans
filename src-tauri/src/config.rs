use std::{fs, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub type ConfigState = Mutex<AppConfig>;

pub const DEFAULT_SYSTEM_PROMPT: &str = "你是专业翻译引擎。将用户输入的文本翻译：原文以中文为主时译为英文，否则译为简体中文。即使输入只是单个单词或短语，也必须给出译文，禁止原样返回；输入为单个单词时，输出其常用义项并标注词性（如 n. 检查；核对 v. 检查，查看）。除译文或义项外不要输出任何解释、注音或多余内容，保留原文的换行与格式。";

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
            theme: "system".into(),
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
