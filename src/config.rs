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
        Ok(Self {
            gmail_address: env("GMAIL_ADDRESS")?,
            gmail_app_password: env("GMAIL_APP_PASSWORD")?,
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
