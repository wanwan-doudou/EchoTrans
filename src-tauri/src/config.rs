use std::{fs, path::Path, path::PathBuf, sync::Mutex};

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
            api_base: String::new(),
            api_key: String::new(),
            model: String::new(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.into(),
            temperature: 0.3,
            enable_machine: true,
            theme: "light".into(),
            snip_hotkey: "Alt+W".into(),
        }
    }
}

impl AppConfig {
    pub fn validate_for_save(&self) -> Result<(), String> {
        if self.api_base.trim().is_empty() {
            return Err("接口地址不能为空，已取消保存以避免覆盖现有配置".into());
        }
        if self.api_key.trim().is_empty() {
            return Err("API Key 不能为空，已取消保存以避免覆盖现有配置".into());
        }
        if self.model.trim().is_empty() {
            return Err("模型不能为空，已取消保存以避免覆盖现有配置".into());
        }
        if self.system_prompt.trim().is_empty() {
            return Err("系统提示词不能为空，已取消保存以避免覆盖现有配置".into());
        }
        Ok(())
    }
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("config.json"))
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name("config.json.bak")
}

fn temp_path(path: &Path) -> PathBuf {
    path.with_file_name("config.json.tmp")
}

fn read_config_file(path: &Path) -> Result<AppConfig, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("读取配置文件失败：{}：{e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析配置文件失败：{}：{e}", path.display()))
}

pub fn load(app: &AppHandle) -> Result<AppConfig, String> {
    let Some(path) = config_path(app) else {
        return Err("无法获取配置目录".into());
    };

    if !path.exists() {
        return Ok(AppConfig::default());
    }

    match read_config_file(&path) {
        Ok(config) => Ok(config),
        Err(primary_error) => {
            let backup = backup_path(&path);
            if backup.exists() {
                read_config_file(&backup).map_err(|backup_error| {
                    format!("{primary_error}；备份配置也无法恢复：{backup_error}")
                })
            } else {
                Err(primary_error)
            }
        }
    }
}

pub fn save(app: &AppHandle, config: &AppConfig) -> Result<(), String> {
    config.validate_for_save()?;

    let path = config_path(app).ok_or("无法获取配置目录")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let temp = temp_path(&path);
    let backup = backup_path(&path);

    fs::write(&temp, raw).map_err(|e| format!("写入临时配置失败：{e}"))?;
    let _ = read_config_file(&temp)?;

    if path.exists() {
        if read_config_file(&path).is_ok() {
            fs::copy(&path, &backup).map_err(|e| format!("备份旧配置失败：{e}"))?;
        }
        fs::remove_file(&path).map_err(|e| format!("替换配置前移除旧文件失败：{e}"))?;
    }

    if let Err(error) = fs::rename(&temp, &path) {
        if backup.exists() && !path.exists() {
            let _ = fs::copy(&backup, &path);
        }
        return Err(format!("替换配置文件失败：{error}"));
    }

    Ok(())
}
