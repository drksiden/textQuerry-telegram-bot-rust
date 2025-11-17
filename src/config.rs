use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub telegram_token: String,
    pub backend_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            telegram_token: env::var("TELEGRAM_BOT_TOKEN")
                .context("TELEGRAM_BOT_TOKEN environment variable is required")?,
            backend_url: env::var("BACKEND_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
        })
    }
}

