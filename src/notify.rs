use anyhow::{Context, Result, bail};
use serde_json::json;
use std::time::Duration;

/// Publish a plain-text message on the private ntfy topic (`NTFY_TOPIC_URL`,
/// e.g. `https://ntfy.sh/<topic>`, with an access token). Uses ntfy's JSON API:
/// UTF-8 titles and bodies go through untouched, no Markdown parsing (mail
/// bodies, addresses and error strings routinely contain `_`, `*` or `[`).
pub async fn notify(topic_url: &str, token: &str, title: &str, message: &str, urgent: bool) -> Result<()> {
    let (server, topic) = topic_url
        .trim_end_matches('/')
        .rsplit_once('/')
        .context("NTFY_TOPIC_URL must look like https://<server>/<topic>")?;
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .post(server)
        .bearer_auth(token)
        .json(&json!({
            "topic": topic,
            "title": title,
            "message": message,
            "priority": if urgent { 4 } else { 3 },
            "tags": if urgent { vec!["warning", "presence"] } else { vec!["presence"] },
        }))
        .send()
        .await
        .context("failed to send the ntfy notification")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("ntfy error: {body}");
    }
    Ok(())
}
