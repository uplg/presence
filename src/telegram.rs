use anyhow::{Context, Result, bail};
use serde_json::json;
use std::time::Duration;

/// Send a plain-text message via the Telegram Bot API. No `parse_mode`:
/// mail bodies, addresses and error strings routinely contain `_`, `*` or `[`
/// which would make Telegram reject a Markdown message.
pub async fn notify(bot_token: &str, chat_id: &str, message: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(&url)
        .json(&json!({ "chat_id": chat_id, "text": message }))
        .send()
        .await
        .context("failed to send Telegram message")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Telegram API error: {body}");
    }
    Ok(())
}
