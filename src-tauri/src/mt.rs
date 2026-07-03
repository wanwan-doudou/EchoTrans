//! 微软 Edge 免费机器翻译通道：先取临时 JWT，再调 Translator 接口。
//! 无需注册与 API Key，作为 AI 翻译的快速对照结果。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const AUTH_URL: &str = "https://edge.microsoft.com/translate/auth";
const API_URL: &str =
    "https://api-edge.cognitive.microsofttranslator.com/translate?api-version=3.0";
/// JWT 实际有效期约 10 分钟，留出余量提前刷新
const TOKEN_TTL: Duration = Duration::from_secs(8 * 60);

static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            // 接口校验 User-Agent，缺失会返回 400，需模拟 Edge 浏览器
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0")
            .timeout(Duration::from_secs(15))
            .build()
            .expect("构建 HTTP 客户端失败")
    })
}

async fn get_token(force_refresh: bool) -> Result<String, String> {
    if !force_refresh {
        if let Ok(guard) = TOKEN.lock() {
            if let Some((token, fetched_at)) = guard.as_ref() {
                if fetched_at.elapsed() < TOKEN_TTL {
                    return Ok(token.clone());
                }
            }
        }
    }

    let token = client()
        .get(AUTH_URL)
        .send()
        .await
        .map_err(|e| format!("获取翻译授权失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("获取翻译授权失败：{e}"))?
        .text()
        .await
        .map_err(|e| format!("读取翻译授权失败：{e}"))?
        .trim()
        .to_string();

    if token.is_empty() {
        return Err("翻译授权为空".into());
    }
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some((token.clone(), Instant::now()));
    }
    Ok(token)
}

/// 与 AI 通道的默认方向保持一致：原文以中文为主则译成英文，否则译成简体中文
fn target_lang(text: &str) -> &'static str {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for c in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&c) {
            cjk += 1;
        } else if c.is_alphabetic() {
            other += 1;
        }
    }
    if cjk > other {
        "en"
    } else {
        "zh-Hans"
    }
}

pub async fn translate(text: &str) -> Result<String, String> {
    let to = target_lang(text);
    let body = serde_json::json!([{ "Text": text }]);

    // 第一次失败于鉴权时强制刷新 token 再试一次
    for attempt in 0..2 {
        let token = get_token(attempt > 0).await?;
        let resp = client()
            .post(format!("{API_URL}&to={to}"))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("请求失败：{e}"))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
            continue;
        }
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            let detail: String = detail.chars().take(200).collect();
            return Err(format!("接口返回 {status}：{detail}"));
        }

        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析响应失败：{e}"))?;
        return match value[0]["translations"][0]["text"].as_str() {
            Some(t) if !t.is_empty() => Ok(t.to_string()),
            _ => Err("接口未返回译文".into()),
        };
    }
    Err("翻译授权已失效，请稍后重试".into())
}
