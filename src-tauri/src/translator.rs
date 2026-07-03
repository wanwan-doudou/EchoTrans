use futures_util::StreamExt;

use crate::config::AppConfig;

/// 文本翻译：调用 OpenAI 兼容的 /chat/completions 流式接口，
/// 每收到一段增量文本回调一次 on_delta，结束后返回完整译文。
pub async fn translate_stream<F>(cfg: &AppConfig, text: &str, on_delta: F) -> Result<String, String>
where
    F: FnMut(&str),
{
    let messages = serde_json::json!([
        { "role": "system", "content": cfg.system_prompt },
        { "role": "user", "content": text }
    ]);
    stream_chat(cfg, messages, on_delta).await
}

/// 截屏翻译：截图直接交给视觉模型，识别与翻译一步完成（需模型支持图片输入）
pub async fn translate_image_stream<F>(
    cfg: &AppConfig,
    png_base64: &str,
    on_delta: F,
) -> Result<String, String>
where
    F: FnMut(&str),
{
    let system_prompt = format!(
        "{}\n用户消息是一张屏幕截图：先识别图中文字，再按上述规则翻译；忽略按钮、菜单等界面装饰元素，只输出正文的译文。",
        cfg.system_prompt
    );
    let messages = serde_json::json!([
        { "role": "system", "content": system_prompt },
        {
            "role": "user",
            "content": [{
                "type": "image_url",
                "image_url": { "url": format!("data:image/png;base64,{png_base64}") }
            }]
        }
    ]);
    stream_chat(cfg, messages, on_delta).await
}

async fn stream_chat<F>(
    cfg: &AppConfig,
    messages: serde_json::Value,
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

    let body = serde_json::json!({
        "model": cfg.model,
        "messages": messages,
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
