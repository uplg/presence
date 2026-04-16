use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::json;

/// Send a message via Telegram Bot API.
pub async fn notify(bot_token: &str, chat_id: &str, message: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let resp = Client::new()
        .post(&url)
        .json(&json!({
            "chat_id": chat_id,
            "text": message,
            "parse_mode": "Markdown",
        }))
        .send()
        .await
        .context("failed to send Telegram message")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Telegram API error: {body}");
    }
    Ok(())
}
