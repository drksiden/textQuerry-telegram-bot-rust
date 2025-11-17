use crate::api_client::{ApiClient, QueryRequest};
use crate::utils::{format_query_response, format_error, format_help, create_suggestions_keyboard};
use teloxide::prelude::*;
use teloxide::types::Message;
use tracing::{info, error};
use std::sync::Arc;

pub async fn handle_message(bot: Bot, msg: Message, api_client: Arc<ApiClient>) -> ResponseResult<()> {
    let user_id = msg.chat.id.to_string();
    let text = msg.text().unwrap_or_default().trim();

    if text.is_empty() {
        return Ok(());
    }

    info!("Received message from user {}: {}", user_id, text);

    // Обрабатываем кнопки меню
    use crate::menu::button_to_query;
    
    // Проверяем специальные кнопки
    match text {
        "❓ Помощь" => {
            return handle_help(bot, msg).await;
        }
        "🔄 Очистить контекст" => {
            return handle_clear(bot, msg, api_client).await;
        }
        _ => {
            // Проверяем, является ли это кнопкой меню с запросом
            if let Some(query) = button_to_query(text) {
                // Это кнопка меню, преобразуем в запрос
                // Отправляем сообщение "обрабатывается"
                let processing_msg = bot.send_message(msg.chat.id, "⏳ <b>Обрабатываю запрос...</b>")
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .reply_to_message_id(msg.id)
                    .await?;
                
                let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;
                
                // Определяем формат вывода из запроса
                let (clean_query, output_type) = detect_output_format(&query);
                
                let query_request = QueryRequest {
                    question: clean_query,
                    include_analysis: true, // Для кнопок меню всегда включаем анализ
                    use_cache: true,
                    include_sql: false,
                    user_id: Some(user_id.clone()),
                    output_type,
                };
                
                match api_client.query(query_request).await {
                    Ok(response) => {
                        // Удаляем сообщение "обрабатывается"
                        let _ = bot.delete_message(msg.chat.id, processing_msg.id).await;
                        // Обрабатываем ответ так же, как обычное сообщение
                        return process_query_response(bot, msg, response, api_client).await;
                    }
                    Err(e) => {
                        // Удаляем сообщение "обрабатывается" даже при ошибке
                        let _ = bot.delete_message(msg.chat.id, processing_msg.id).await;
                        error!("Error processing menu button query: {}", e);
                        bot.send_message(msg.chat.id, &format_error(&format!("Не удалось обработать запрос: {}", e)))
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .await?;
                        return Ok(());
                    }
                }
            }
        }
    }

    // Отправляем сообщение "обрабатывается"
    let processing_msg = bot.send_message(msg.chat.id, "⏳ <b>Обрабатываю запрос...</b>")
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_to_message_id(msg.id)
        .await?;
    
    // Отправляем индикатор печати
    let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;

    // Определяем формат вывода из запроса
    let (clean_text, output_type) = detect_output_format(text);

    // Определяем, нужен ли анализ
    let include_analysis = clean_text.to_lowercase().contains("с анализом") 
        || clean_text.to_lowercase().contains("анализ");

    // Убираем фразу про анализ из запроса
    let question = clean_text
        .replace("с анализом", "")
        .replace("анализ", "")
        .trim()
        .to_string();

    // Пытаемся сначала как SQL-запрос
    let query_request = QueryRequest {
        question: question.clone(),
        include_analysis,
        use_cache: true,
        include_sql: false, // Не показываем SQL в Telegram
        user_id: Some(user_id.clone()),
        output_type,
    };

    match api_client.query(query_request).await {
        Ok(response) => {
            // Удаляем сообщение "обрабатывается"
            let _ = bot.delete_message(msg.chat.id, processing_msg.id).await;
            
            // Если есть текстовый ответ (обычный вопрос)
            if let Some(text_response) = &response.text_response {
                bot.send_message(msg.chat.id, text_response)
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await?;
                return Ok(());
            }

            // Отправляем CSV файл, если есть данные
            if !response.data.is_empty() {
                use crate::utils::format_as_csv;
                let csv_content = format_as_csv(&response.data);
                if !csv_content.is_empty() {
                    let filename = format!("data_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
                    // Создаем временный файл
                    let temp_path = std::env::temp_dir().join(&filename);
                    std::fs::write(&temp_path, csv_content.as_bytes())?;
                    bot.send_document(msg.chat.id, teloxide::types::InputFile::file(&temp_path))
                        .caption("📊 Данные в формате CSV")
                        .await?;
                    let _ = std::fs::remove_file(&temp_path);
                }
            }
            
            // Отправляем диаграмму, если есть данные для неё
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
                                error!("Failed to send chart image: {}", e);
                            }
                            let _ = std::fs::remove_file(&temp_path);
                        }
                    }
                    Err(e) => {
                        error!("Failed to generate chart image: {}", e);
                    }
                }
            }
            
            // Форматируем ответ
            let formatted = format_query_response(&response);
            
            // Создаем клавиатуру с предложениями, если есть анализ
            // Показываем кнопки с подсказками всегда, если они есть
            let keyboard = if let Some(analysis) = &response.analysis {
                if !analysis.suggested_questions.is_empty() {
                    Some(create_suggestions_keyboard(&analysis.suggested_questions))
                } else {
                    None
                }
            } else {
                None
            };
            
            // Если нет анализа, но есть данные - предлагаем стандартные вопросы
            let keyboard = keyboard.or_else(|| {
                if !response.data.is_empty() && response.row_count > 0 {
                    let suggestions = vec![
                        "📊 Показать больше данных".to_string(),
                        "📈 С анализом".to_string(),
                    ];
                    Some(create_suggestions_keyboard(&suggestions))
                } else {
                    None
                }
            });
            
            // Отправляем ответ (Telegram ограничивает длину сообщения)
            if formatted.len() > 4096 {
                // Разбиваем на части с учетом UTF-8 границ
                let mut chunks = Vec::new();
                let mut current = String::new();
                
                for line in formatted.lines() {
                    if current.len() + line.len() + 1 > 4000 {
                        if !current.is_empty() {
                            chunks.push(current.clone());
                            current.clear();
                        }
                    }
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(line);
                }
                if !current.is_empty() {
                    chunks.push(current);
                }
                
                // Отправляем все части кроме последней
                for chunk in chunks.iter().take(chunks.len().saturating_sub(1)) {
                    bot.send_message(msg.chat.id, chunk)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                }
                
                // Последняя часть с клавиатурой
                let mut last_msg = bot.send_message(msg.chat.id, chunks.last().unwrap_or(&formatted))
                    .parse_mode(teloxide::types::ParseMode::Html);
                
                if let Some(kb) = keyboard {
                    last_msg = last_msg.reply_markup(kb);
                }
                
                last_msg.await?;
            } else {
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
            
            error!("Error querying backend: {}", e);
            
            // Если ошибка SQL (обычно означает, что вопрос не про БД), 
            // попробуем ответить через chat API
            let error_str = e.to_string();
            if error_str.contains("syntax error") || 
               error_str.contains("SQL") || 
               error_str.contains("database") {
                info!("SQL error detected, trying chat API instead");
                
                // Пробуем через chat API
                match api_client.chat(crate::api_client::ChatRequest {
                    message: question.clone(),
                    session_id: None,
                    user_id: Some(user_id.clone()),
                }).await {
                    Ok(chat_response) => {
                        bot.send_message(msg.chat.id, &chat_response.message)
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .await?;
                        return Ok(());
                    }
                    Err(chat_err) => {
                        error!("Chat API also failed: {}", chat_err);
                        // Показываем понятное сообщение
                        bot.send_message(msg.chat.id, 
                            "🤔 Похоже, ваш вопрос не связан с базой данных. Я могу помочь с анализом платежных транзакций.\n\nПопробуйте задать вопрос, например:\n• Сколько транзакций было сегодня?\n• Топ 10 городов по объему транзакций")
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .await?;
                        return Ok(());
                    }
                }
            }
            
            // Для других ошибок показываем стандартное сообщение
            let error_msg = format_error(&format!("Не удалось обработать запрос. Попробуйте переформулировать вопрос или используйте /help для примеров."));
            bot.send_message(msg.chat.id, &error_msg)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
    }

    Ok(())
}

/// Обрабатывает ответ на запрос (общая функция для переиспользования)
async fn process_query_response(
    bot: Bot,
    msg: Message,
    response: crate::api_client::QueryResponse,
    _api_client: Arc<ApiClient>,
) -> ResponseResult<()> {
    // Если есть текстовый ответ (обычный вопрос)
    if let Some(text_response) = &response.text_response {
        bot.send_message(msg.chat.id, text_response)
            .parse_mode(teloxide::types::ParseMode::Html)
            .await?;
        return Ok(());
    }

    // Отправляем CSV файл, если есть данные
    if !response.data.is_empty() {
        use crate::utils::format_as_csv;
        let csv_content = format_as_csv(&response.data);
        if !csv_content.is_empty() {
            let filename = format!("data_{}.csv", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
            // Создаем временный файл
            let temp_path = std::env::temp_dir().join(&filename);
            if let Ok(_) = std::fs::write(&temp_path, csv_content.as_bytes()) {
                let _ = bot.send_document(msg.chat.id, teloxide::types::InputFile::file(&temp_path))
                    .caption("📊 Данные в формате CSV")
                    .await;
                let _ = std::fs::remove_file(&temp_path);
            }
        }
    }
    
    // Отправляем диаграмму, если есть данные для неё
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
                        error!("Failed to send chart image: {}", e);
                    }
                    let _ = std::fs::remove_file(&temp_path);
                }
            }
            Err(e) => {
                error!("Failed to generate chart image: {}", e);
            }
        }
    }
    
    // Форматируем ответ
    let formatted = format_query_response(&response);
    
    // Создаем клавиатуру с предложениями, если есть анализ
    // Показываем кнопки с подсказками всегда, если они есть
    let keyboard = if let Some(analysis) = &response.analysis {
        if !analysis.suggested_questions.is_empty() {
            Some(create_suggestions_keyboard(&analysis.suggested_questions))
        } else {
            None
        }
    } else {
        None
    };
    
    // Если нет анализа, но есть данные - предлагаем стандартные вопросы
    let keyboard = keyboard.or_else(|| {
        if !response.data.is_empty() && response.row_count > 0 {
            let suggestions = vec![
                "📊 Показать больше данных".to_string(),
                "📈 С анализом".to_string(),
            ];
            Some(create_suggestions_keyboard(&suggestions))
        } else {
            None
        }
    });
    
    // Отправляем ответ (Telegram ограничивает длину сообщения)
    if formatted.len() > 4096 {
        // Разбиваем на части с учетом UTF-8 границ
        let mut chunks = Vec::new();
        let mut current = String::new();
        
        for line in formatted.lines() {
            if current.len() + line.len() + 1 > 4000 {
                if !current.is_empty() {
                    chunks.push(current.clone());
                    current.clear();
                }
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        
        // Отправляем все части кроме последней
        for chunk in chunks.iter().take(chunks.len().saturating_sub(1)) {
            bot.send_message(msg.chat.id, chunk)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        
        // Последняя часть с клавиатурой
        let mut last_msg = bot.send_message(msg.chat.id, chunks.last().unwrap_or(&formatted))
            .parse_mode(teloxide::types::ParseMode::Html);
        
        if let Some(kb) = keyboard {
            last_msg = last_msg.reply_markup(kb);
        }
        
        last_msg.await?;
    } else {
        let mut message = bot.send_message(msg.chat.id, &formatted)
            .parse_mode(teloxide::types::ParseMode::Html);
        
        if let Some(kb) = keyboard {
            message = message.reply_markup(kb);
        }
        
        message.await?;
    }
    
    Ok(())
}

/// Определяет желаемый формат вывода из текста запроса
/// Возвращает очищенный текст и тип вывода
fn detect_output_format(text: &str) -> (String, crate::api_client::OutputType) {
    let text_lower = text.to_lowercase();
    
    // Ключевые слова для таблицы
    let table_keywords = [
        "таблица", "table", "таблицу", "таблицей", 
        "в таблице", "как таблица", "покажи таблицу",
        "табличный", "табличный формат"
    ];
    
    // Ключевые слова для диаграммы
    let chart_keywords = [
        "диаграмма", "chart", "график", "графиком",
        "диаграмму", "диаграммой", "в диаграмме",
        "как диаграмма", "покажи диаграмму", "визуализация",
        "визуализацию", "визуализацией", "визуализировать",
        "графически", "графический", "plot", "график",
        "нарисуй", "построй", "visualization"
    ];
    
    // Проверяем наличие ключевых слов
    let has_table = table_keywords.iter().any(|keyword| text_lower.contains(keyword));
    let has_chart = chart_keywords.iter().any(|keyword| text_lower.contains(keyword));
    
    // Определяем тип вывода
    let output_type = if has_chart {
        crate::api_client::OutputType::Chart
    } else if has_table {
        crate::api_client::OutputType::Table
    } else if text_lower.contains("json") {
        crate::api_client::OutputType::Json
    } else {
        crate::api_client::OutputType::Auto
    };
    
    // Убираем ключевые слова из текста запроса
    let mut clean_text = text.to_string();
    let text_lower = clean_text.to_lowercase();
    
    // Находим все ключевые слова и убираем их
    let mut to_remove: Vec<(usize, usize)> = Vec::new();
    
    // Находим позиции ключевых слов для таблицы
    for keyword in &table_keywords {
        let keyword_lower = keyword.to_lowercase();
        let mut search_pos = 0;
        while let Some(pos) = text_lower[search_pos..].find(&keyword_lower) {
            let actual_pos = search_pos + pos;
            to_remove.push((actual_pos, actual_pos + keyword.len()));
            search_pos = actual_pos + keyword.len();
        }
    }
    
    // Находим позиции ключевых слов для диаграммы
    for keyword in &chart_keywords {
        let keyword_lower = keyword.to_lowercase();
        let mut search_pos = 0;
        while let Some(pos) = text_lower[search_pos..].find(&keyword_lower) {
            let actual_pos = search_pos + pos;
            to_remove.push((actual_pos, actual_pos + keyword.len()));
            search_pos = actual_pos + keyword.len();
        }
    }
    
    // Сортируем позиции по убыванию, чтобы удалять с конца
    to_remove.sort_by(|a, b| b.0.cmp(&a.0));
    
    // Удаляем ключевые слова с конца к началу
    for (start, end) in to_remove {
        if end <= clean_text.len() {
            // Безопасное удаление с учетом UTF-8
            let mut chars: Vec<char> = clean_text.chars().collect();
            let start_char = clean_text.chars().take(start).count();
            let end_char = clean_text.chars().take(end).count();
            if end_char <= chars.len() {
                chars.drain(start_char..end_char);
                clean_text = chars.into_iter().collect();
            }
        }
    }
    
    // Очищаем лишние пробелы и запятые
    clean_text = clean_text
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_matches(',')
        .trim()
        .to_string();
    
    (clean_text, output_type)
}

pub async fn handle_start(bot: Bot, msg: Message) -> ResponseResult<()> {
    use crate::menu::create_main_menu;
    
    let welcome = r#"👋 <b>Добро пожаловать в Payment Analytics Bot!</b>

🤖 Я умный помощник для анализа платежных транзакций.

Просто задавайте вопросы на естественном языке, и я сгенерирую SQL-запросы и предоставлю детальную аналитику!

✨ <b>Что я умею:</b>
• Анализ транзакций в реальном времени
• Генерация SQL-запросов из обычных вопросов
• Детальная аналитика с инсайтами и рекомендациями
• Экспорт данных в CSV
• Генерация диаграмм
• Поддержка русского, английского и казахского языков
• Контекстная память ваших запросов

🔍 <b>ВАЖНО: Для SQL запросов к базе данных ОБЯЗАТЕЛЬНО используйте префикс:</b>
• <code>sql:</code> - например: <code>sql: Показать транзакции за сегодня</code>

⚠️ <b>Без префикса</b> бот может неправильно определить тип запроса и ответить как в чате.

⚠️ <b>Важно о данных:</b> Все данные в базе на латинице (Astana, Almaty, Halyk Bank). Бот автоматически преобразует кириллицу.

💡 Используйте кнопки меню для быстрого доступа к популярным запросам или просто напишите свой вопрос!"#;

    bot.send_message(msg.chat.id, welcome)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(create_main_menu())
        .reply_to_message_id(msg.id)
        .await?;

    Ok(())
}

pub async fn handle_help(bot: Bot, msg: Message) -> ResponseResult<()> {
    let help_text = format_help();
    
    bot.send_message(msg.chat.id, &help_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_to_message_id(msg.id)
        .await?;

    Ok(())
}

pub async fn handle_clear(bot: Bot, msg: Message, api_client: Arc<ApiClient>) -> ResponseResult<()> {
    let user_id = msg.chat.id.to_string();
    
    match api_client.clear_context(&user_id).await {
        Ok(_) => {
            bot.send_message(msg.chat.id, "✅ Контекст запросов очищен!")
                .reply_to_message_id(msg.id)
                .await?;
        }
        Err(e) => {
            error!("Error clearing context: {}", e);
            bot.send_message(msg.chat.id, &format!("❌ Ошибка при очистке контекста: {}", e))
                .reply_to_message_id(msg.id)
                .await?;
        }
    }

    Ok(())
}

pub async fn handle_status(bot: Bot, msg: Message, api_client: Arc<ApiClient>) -> ResponseResult<()> {
    match api_client.health_check().await {
        Ok(true) => {
            bot.send_message(msg.chat.id, "✅ Бэкенд работает нормально!")
                .reply_to_message_id(msg.id)
                .await?;
        }
        Ok(false) => {
            bot.send_message(msg.chat.id, "⚠️ Бэкенд недоступен")
                .reply_to_message_id(msg.id)
                .await?;
        }
        Err(e) => {
            error!("Error checking backend status: {}", e);
            bot.send_message(msg.chat.id, &format!("❌ Ошибка при проверке статуса: {}", e))
                .reply_to_message_id(msg.id)
                .await?;
        }
    }

    Ok(())
}

