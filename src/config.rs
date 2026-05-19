use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub gmail_address: String,
    pub gmail_app_password: String,
    pub recipient_1: String,
    pub recipient_2: String,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub sender_name: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        match dotenvy::dotenv() {
            Ok(path) => tracing::info!("loaded .env from {}", path.display()),
            Err(e) => tracing::warn!("no .env loaded: {e}"),
        }
        // Google displays App Passwords grouped as "abcd efgh ijkl mnop" for
        // readability, but the real secret is the 16 contiguous chars with no
        // whitespace. Strip ALL whitespace so a copy-pasted (and/or quoted)
        // value works — avoids a silent 535 BadCredentials at send time.
        let gmail_app_password: String =
            env("GMAIL_APP_PASSWORD")?.split_whitespace().collect();
        if gmail_app_password.len() != 16 {
            tracing::warn!(
                "GMAIL_APP_PASSWORD is {} chars after stripping whitespace (Google App Passwords are 16) — Gmail may reject it",
                gmail_app_password.len()
            );
        }

        Ok(Self {
            gmail_address: env("GMAIL_ADDRESS")?,
            gmail_app_password,
            recipient_1: env("RECIPIENT_1")?,
            recipient_2: env("RECIPIENT_2")?,
            telegram_bot_token: env("TELEGRAM_BOT_TOKEN")?,
            telegram_chat_id: env("TELEGRAM_CHAT_ID")?,
            sender_name: env("SENDER_NAME")?,
        })
    }
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing env var: {key}"))
}
