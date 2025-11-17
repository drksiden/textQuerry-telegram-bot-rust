use crate::config::Config;
use crate::api_client::ApiClient;
use crate::handlers;
use teloxide::prelude::*;
use teloxide::types::Message;
use anyhow::Result;
use tracing::info;
use std::sync::Arc;

pub async fn start_bot(bot: Bot, config: Config) -> Result<()> {
    info!("Bot is starting...");

    let api_client = Arc::new(ApiClient::new(config.backend_url.clone()));

    // Проверяем подключение к бэкенду
    match api_client.health_check().await {
        Ok(true) => info!("Backend is available"),
        Ok(false) => {
            tracing::warn!("Backend is not available, but continuing anyway");
        }
        Err(e) => {
            tracing::warn!("Failed to check backend: {} (continuing anyway)", e);
        }
    }

    let api_client_clone1 = api_client.clone();
    let api_client_clone2 = api_client.clone();
    let api_client_clone3 = api_client.clone();
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter(|msg: Message| {
                    if let Some(text) = msg.text() {
                        text.starts_with('/')
                    } else {
                        false
                    }
                })
                .endpoint(move |bot: Bot, msg: Message| {
                    let api_client = api_client_clone1.clone();
                    async move {
                        handle_commands(bot, msg, api_client).await
                    }
                })
        )
        .branch(
            Update::filter_callback_query()
                .endpoint(move |bot: Bot, q: teloxide::types::CallbackQuery| {
                    let api_client = api_client_clone2.clone();
                    async move {
                        handle_callback(bot, q, api_client).await
                    }
                })
        )
        .branch(
            Update::filter_message()
                .endpoint(move |bot: Bot, msg: Message| {
                    let api_client = api_client_clone3.clone();
                    async move {
                        handle_messages(bot, msg, api_client).await
                    }
                })
        );

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn handle_commands(
    bot: Bot,
    msg: Message,
    api_client: Arc<ApiClient>,
) -> ResponseResult<()> {
    let text = msg.text().unwrap_or_default();
    let command = text.split_whitespace().next().unwrap_or("");

    match command {
        "/start" => {
            handlers::handle_start(bot, msg).await?;
        }
        "/help" => {
            handlers::handle_help(bot, msg).await?;
        }
        "/clear" => {
            handlers::handle_clear(bot, msg, api_client).await?;
        }
        "/status" => {
            handlers::handle_status(bot, msg, api_client).await?;
        }
        "/menu" => {
            use crate::menu::create_main_menu;
            bot.send_message(msg.chat.id, "📋 Главное меню")
                .reply_markup(create_main_menu())
                .reply_to_message_id(msg.id)
                .await?;
        }
        _ => {
            // Неизвестная команда, игнорируем
        }
    }

    Ok(())
}

async fn handle_callback(
    bot: Bot,
    q: teloxide::types::CallbackQuery,
    api_client: Arc<ApiClient>,
) -> ResponseResult<()> {
    if let Some(data) = q.data {
        // Отвечаем на callback сразу
        bot.answer_callback_query(q.id).await?;
        
        if let Some(msg) = q.message {
            // Отправляем сообщение "обрабатывается"
            let processing_msg = bot.send_message(msg.chat.id, "⏳ <b>Обрабатываю запрос...</b>")
                .parse_mode(teloxide::types::ParseMode::Html)
                .reply_to_message_id(msg.id)
                .await?;
            
            // Отправляем индикатор печати
            let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;
            
            let question = if data.starts_with("query:") {
                let q = data.strip_prefix("query:").unwrap_or("").to_string();
                       // Suggested questions всегда SQL запросы, добавляем префикс если его нет
                       if !q.to_lowercase().starts_with("sql:") {
                           format!("sql: {}", q)
                       } else {
                           q
                       }
            } else if data.starts_with("q:") {
                // Это хеш, нужно получить оригинальный вопрос
                // Пока что просто возвращаем пустую строку - это не должно происходить
                // В будущем можно добавить кеш вопросов по хешам
                tracing::warn!("Received hash-based callback, but no mapping available: {}", data);
                return Ok(());
            } else {
                return Ok(());
            };
            
            if question.is_empty() {
                return Ok(());
            }
                
            // Обрабатываем запрос напрямую
            let user_id = msg.chat.id.to_string();
            let query_request = crate::api_client::QueryRequest {
                question: question.clone(),
                include_analysis: true,
                use_cache: true,
                include_sql: false,
                user_id: Some(user_id.clone()),
                output_type: crate::api_client::OutputType::Auto,
            };
            
            match api_client.query(query_request).await {
                Ok(response) => {
                    // Удаляем сообщение "обрабатывается"
                    let _ = bot.delete_message(msg.chat.id, processing_msg.id).await;
                    
                    // Отправляем CSV, если есть
                    if !response.data.is_empty() {
                        use crate::utils::format_as_csv;
                        let csv_content = format_as_csv(&response.data);
                        if !csv_content.is_empty() {
                            let filename = format!("data_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
                            let temp_path = std::env::temp_dir().join(&filename);
                            if let Ok(_) = std::fs::write(&temp_path, csv_content.as_bytes()) {
                                let _ = bot.send_document(msg.chat.id, teloxide::types::InputFile::file(&temp_path))
                                    .caption("📊 Данные в формате CSV")
                                    .await;
                                let _ = std::fs::remove_file(&temp_path);
                            }
                        }
                    }
                    
                    // Отправляем диаграмму, если есть
                    if let Some(chart_data) = &response.chart_data {
                        use crate::utils::generate_chart_image;
                        // Генерируем изображение синхронно перед await
                        let image_result = generate_chart_image(chart_data, 1000, 700);
                        match image_result {
                            Ok(image_bytes) => {
                                let temp_path = std::env::temp_dir().join(format!("chart_{}.png", std::process::id()));
                                if let Ok(_) = std::fs::write(&temp_path, &image_bytes) {
                                    if let Err(e) = bot.send_photo(msg.chat.id, teloxide::types::InputFile::file(&temp_path))
                                        .caption("📈 Визуализация данных")
                                        .await {
                                        tracing::error!("Failed to send chart image: {}", e);
                                    }
                                    let _ = std::fs::remove_file(&temp_path);
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to generate chart image: {}", e);
                            }
                        }
                    }
                    
                    // Отправляем текстовый ответ
                    if let Some(text_response) = &response.text_response {
                        bot.send_message(msg.chat.id, text_response)
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .await?;
                    } else {
                        let formatted = crate::utils::format_query_response(&response);
                        let keyboard = if let Some(analysis) = &response.analysis {
                            if !analysis.suggested_questions.is_empty() {
                                Some(crate::utils::create_suggestions_keyboard(&analysis.suggested_questions))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        
                        let mut message = bot.send_message(msg.chat.id, &formatted)
                            .parse_mode(teloxide::types::ParseMode::Html);
                        
                        if let Some(kb) = keyboard {
                            message = message.reply_markup(kb);
                        }
                        
                        message.await?;
                    }
                }
                Err(e) => {
                    // Удаляем сообщение "обрабатывается" даже при ошибке
                    let _ = bot.delete_message(msg.chat.id, processing_msg.id).await;
                    
                    tracing::error!("Error processing callback query: {}", e);
                    bot.send_message(msg.chat.id, &format!("❌ Ошибка: {}", e))
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                }
            }
        }
    }
    Ok(())
}

async fn handle_messages(
    bot: Bot,
    msg: Message,
    api_client: Arc<ApiClient>,
) -> ResponseResult<()> {
    handlers::handle_message(bot, msg, api_client).await?;
    Ok(())
}

