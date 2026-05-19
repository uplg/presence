use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::{Value, json};

/// Send a Markdown-formatted message via Telegram Bot API.
pub async fn notify(bot_token: &str, chat_id: &str, message: &str) -> Result<()> {
    send(bot_token, chat_id, message, true).await
}

/// Send a plain-text alert (no Markdown parsing). Safe for arbitrary error
/// strings, which often contain `[`, `_`, `*` that break Markdown parsing
/// (e.g. `["cpu (avail: 0MB)", ...]`).
pub async fn notify_error(bot_token: &str, chat_id: &str, message: &str) -> Result<()> {
    send(bot_token, chat_id, message, false).await
}

async fn send(bot_token: &str, chat_id: &str, message: &str, markdown: bool) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let mut payload = json!({
        "chat_id": chat_id,
        "text": message,
    });
    if markdown {
        payload["parse_mode"] = Value::String("Markdown".into());
    }
    let resp = Client::new()
        .post(&url)
        .json(&payload)
        .send()
        .await
        .context("failed to send Telegram message")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Telegram API error: {body}");
    }
    Ok(())
}
