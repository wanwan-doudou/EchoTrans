use futures_util::StreamExt;

use crate::config::AppConfig;

/// 调用 OpenAI 兼容的 /chat/completions 流式接口，每收到一段增量文本回调一次 on_delta，
/// 结束后返回完整译文。
pub async fn translate_stream<F>(
    cfg: &AppConfig,
    text: &str,
    is_ocr: bool,
    mut on_delta: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    if cfg.api_key.trim().is_empty() {
        return Err("尚未配置 API Key，请在托盘图标 → 设置中填写".into());
    }

    // 容错：用户填 https://xx/v1 或直接填完整 /chat/completions 均可
    let base = cfg.api_base.trim().trim_end_matches('/');
    let url = if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    };

    let system_prompt = if is_ocr {
        format!(
            "{}\n输入文本来自屏幕 OCR。翻译前请结合上下文静默修正明显的字符误识别、断词和视觉换行；不得编造缺失内容，最终只输出译文。",
            cfg.system_prompt
        )
    } else {
        cfg.system_prompt.clone()
    };

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": text }
        ],
        "temperature": cfg.temperature,
        "stream": true
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(cfg.api_key.trim())
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        let detail: String = detail.chars().take(300).collect();
        return Err(format!("接口返回 {status}：{detail}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取响应流失败：{e}"))?;
        buf.extend_from_slice(&chunk);

        // SSE 按行切分后再解码：网络 chunk 可能把多字节 UTF-8 字符截断，
        // 行边界（\n 单字节）一定完整，逐行解码不会出现乱码
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(full);
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(delta) = value["choices"][0]["delta"]["content"].as_str() {
                if !delta.is_empty() {
                    full.push_str(delta);
                    on_delta(delta);
                }
            }
        }
    }

    if full.is_empty() {
        return Err("接口未返回内容，请检查接口地址与模型名是否正确".into());
    }
    Ok(full)
}
