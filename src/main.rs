mod bot;
mod config;
mod handlers;
mod api_client;
mod utils;
mod menu;

use anyhow::Result;
use config::Config;
use teloxide::prelude::*;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    
    info!("Starting Telegram bot...");
    info!("Backend URL: {}", config.backend_url);
    
    // Create bot
    let bot = Bot::new(&config.telegram_token);
    
    // Start bot
    bot::start_bot(bot, config).await?;
    
    Ok(())
}

