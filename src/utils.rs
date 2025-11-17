use serde_json::Value;
use crate::api_client::ChartData;

/// Форматирует данные в CSV
pub fn format_as_csv(data: &[Value]) -> String {
    if data.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    
    if let Some(first_obj) = data[0].as_object() {
        let keys: Vec<String> = first_obj.keys().map(|k| k.clone()).collect();
        
        // Заголовок
        result.push_str(&keys.join(","));
        result.push_str("\n");
        
        // Данные
        for row in data {
            if let Some(obj) = row.as_object() {
                let values: Vec<String> = keys.iter()
                    .map(|key| {
                        let value = obj.get(key)
                            .and_then(|v| {
                                if v.is_number() {
                                    Some(format!("{}", v.as_f64().unwrap_or(0.0)))
                                } else {
                                    v.as_str().map(|s| format!("\"{}\"", s.replace("\"", "\"\"")))
                                }
                            })
                            .unwrap_or_else(|| "".to_string());
                        value
                    })
                    .collect();
                result.push_str(&values.join(","));
                result.push_str("\n");
            }
        }
    }

    result
}

/// Генерирует изображение диаграммы из данных
/// Возвращает PNG изображение в виде байтов
pub fn generate_chart_image(
    chart_data: &ChartData,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    use plotters::prelude::*;
    
    // Создаем временный файл для plotters
    let temp_path = std::env::temp_dir().join(format!("chart_{}.png", std::process::id()));
    
    {
        // Используем файл для создания изображения
        let root = BitMapBackend::new(&temp_path, (width, height))
            .into_drawing_area();
        root.fill(&WHITE)?;
        
        let root = root.margin(50, 20, 20, 50);
        
        let max_val = chart_data.datasets[0].data.iter().fold(0f64, |a, &b| a.max(b));
        let label_count = chart_data.labels.len();
        
        if label_count == 0 {
            return Ok(Vec::new());
        }
        
        // Определяем тип диаграммы
        let chart_type = chart_data.chart_type.to_lowercase();
        
        // Улучшенная визуализация с поддержкой разных типов
        let mut chart = ChartBuilder::on(&root)
            .caption(
                &chart_data.title.clone().unwrap_or_else(|| "Данные".to_string()),
                ("sans-serif", 24).into_font()
            )
            .x_label_area_size(60)
            .y_label_area_size(80)
            .build_cartesian_2d(0..label_count as i32, 0f64..max_val)?;
        
        // Настраиваем сетку и подписи
        chart.configure_mesh()
            .x_labels(label_count.min(20)) // Ограничиваем количество меток на оси X
            .y_label_formatter(&|y| {
                // Форматируем большие числа
                if *y >= 1_000_000_000.0 {
                    format!("{:.1}B", y / 1_000_000_000.0)
                } else if *y >= 1_000_000.0 {
                    format!("{:.1}M", y / 1_000_000.0)
                } else if *y >= 1_000.0 {
                    format!("{:.1}K", y / 1_000.0)
                } else {
                    format!("{:.0}", y)
                }
            })
            .x_label_formatter(&|x| {
                // Обрезаем длинные метки
                if let Some(label) = chart_data.labels.get(*x as usize) {
                    if label.chars().count() > 10 {
                        label.chars().take(8).collect::<String>() + ".."
                    } else {
                        label.clone()
                    }
                } else {
                    format!("{}", x)
                }
            })
            .draw()?;
        
        // Рисуем в зависимости от типа диаграммы
        match chart_type.as_str() {
            "line" | "trend" => {
                // Линейный график
                let points: Vec<(i32, f64)> = chart_data.datasets[0].data.iter()
                    .enumerate()
                    .map(|(i, &val)| (i as i32, val))
                    .collect();
                
                chart.draw_series(LineSeries::new(
                    points.iter().map(|&(x, y)| (x, y)),
                    RED.stroke_width(2),
                ))?;
                
                // Добавляем точки
                chart.draw_series(
                    points.iter().map(|&(x, y)| {
                        Circle::new((x, y), 3, RED.filled())
                    })
                )?;
            }
            "pie" => {
                // Круговая диаграмма - используем bar chart как fallback
                // (plotters не поддерживает pie напрямую, можно добавить позже)
                for (i, value) in chart_data.datasets[0].data.iter().enumerate() {
                    let x = i as i32;
                    let y_val = *value;
                    let color = Palette99::pick(i);
                    
                    chart.draw_series(std::iter::once(
                        Rectangle::new([(x, 0.0), (x + 1, y_val)], color.filled())
                    ))?;
                }
            }
            _ => {
                // Bar chart (по умолчанию)
                for (i, value) in chart_data.datasets[0].data.iter().enumerate() {
                    let x = i as i32;
                    let y_val = *value;
                    let color = Palette99::pick(i);
                    
                    // Рисуем столбец
                    chart.draw_series(std::iter::once(
                        Rectangle::new([(x, 0.0), (x + 1, y_val)], color.filled())
                    ))?;
                }
            }
        }
    }
    
    // Читаем файл в буфер
    let buffer = std::fs::read(&temp_path)?;
    // Удаляем временный файл
    let _ = std::fs::remove_file(&temp_path);
    
    Ok(buffer)
}

pub fn format_query_response(response: &crate::api_client::QueryResponse) -> String {
    let mut result = String::new();

    // Если есть текстовый ответ (обычный вопрос)
    if let Some(text_response) = &response.text_response {
        result.push_str(&escape_html(text_response));
        return result;
    }

    // Если есть анализ, показываем его
    if let Some(analysis) = &response.analysis {
        result.push_str(&format!("📊 <b>{}</b>\n\n", escape_html(&analysis.headline)));
        
        if !analysis.insights.is_empty() {
            result.push_str("💡 <b>Основные выводы:</b>\n");
            for insight in &analysis.insights {
                let emoji = match insight.significance.as_str() {
                    "High" => "🔴",
                    "Medium" => "🟡",
                    _ => "🟢",
                };
                result.push_str(&format!("{} <b>{}</b>\n{}\n\n", emoji, escape_html(&insight.title), escape_html(&insight.description)));
            }
        }

        result.push_str(&format!("📝 <b>Объяснение:</b>\n{}\n\n", escape_html(&analysis.explanation)));

        if !analysis.suggested_questions.is_empty() {
            result.push_str("💭 <b>Рекомендуемые вопросы:</b>\n");
            result.push_str("<i>Нажмите на кнопку ниже, чтобы выполнить запрос</i>\n\n");
            for (idx, question) in analysis.suggested_questions.iter().enumerate() {
                result.push_str(&format!("{}. {}\n", idx + 1, escape_html(question)));
            }
            result.push_str("\n");
        }
    }

    // Показываем данные только если есть таблица (не для одиночных агрегаций)
    // Для одиночных значений (COUNT, SUM, AVG) показываем только текстовое описание из анализа
    if let Some(table) = &response.table {
        if !table.is_empty() {
            result.push_str(&format!("📋 <b>Результаты ({})</b>:\n\n", response.row_count));
            
            // Если данных немного, показываем таблицу
            if response.row_count <= 10 {
                result.push_str(table);
            } else {
                // Если много данных, показываем первые 5 строк
                let lines: Vec<&str> = table.lines().collect();
                let first_lines = lines.iter().take(10).map(|s| *s).collect::<Vec<_>>().join("\n");
                result.push_str(&first_lines);
                result.push_str(&format!("\n... и еще {} строк(и)\n", response.row_count - 5));
            }
            result.push_str("\n");
        }
    } else if !response.data.is_empty() && response.row_count > 1 {
        // Если нет таблицы, но есть данные (множественные строки), показываем краткую информацию
        result.push_str(&format!("📊 <b>Найдено результатов:</b> {}\n\n", response.row_count));
    } else if response.data.is_empty() {
        result.push_str("📭 Нет данных для отображения\n");
    }

    result.push_str(&format!("\n⏱ <b>Время выполнения:</b> {}ms", response.execution_time_ms));
    if response.cached {
        result.push_str(" (из кэша)");
    }

    result
}

fn format_data_as_table(data: &[Value]) -> String {
    if data.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    
    // Получаем все ключи из первой строки
    if let Some(first_obj) = data[0].as_object() {
        let keys: Vec<&String> = first_obj.keys().collect();
        
        // Формируем заголовок
        result.push_str("```\n");
        for key in &keys {
            result.push_str(&format!("{:20} | ", key));
        }
        result.push_str("\n");
        result.push_str(&"-".repeat(keys.len() * 23));
        result.push_str("\n");

        // Формируем строки данных
        for row in data {
            if let Some(obj) = row.as_object() {
                for key in &keys {
                    let value = obj.get(&**key)
                        .and_then(|v| {
                            if v.is_number() {
                                Some(format!("{:.2}", v.as_f64().unwrap_or(0.0)))
                            } else {
                                v.as_str().map(|s| s.to_string())
                            }
                        })
                        .unwrap_or_else(|| "N/A".to_string());
                    
                    // Обрезаем длинные значения (с учетом UTF-8)
                    let display_value = if value.len() > 18 {
                        // Безопасное обрезание UTF-8
                        let mut chars: Vec<char> = value.chars().take(15).collect();
                        chars.push('…');
                        chars.into_iter().collect::<String>()
                    } else {
                        value
                    };
                    
                    result.push_str(&format!("{:20} | ", display_value));
                }
                result.push_str("\n");
            }
        }
        
        result.push_str("```\n");
    }

    result
}

pub fn format_error(error: &str) -> String {
    format!("❌ <b>Ошибка:</b>\n{}", escape_html(error))
}

pub fn format_help() -> String {
    r#"📖 <b>Справка по использованию бота</b>

🤖 <b>Основные команды:</b>
/start - Начать работу с ботом
/help - Показать эту справку
/clear - Очистить контекст запросов
/status - Проверить статус бэкенда
/menu - Показать главное меню

💡 <b>Как использовать:</b>
Просто задавайте вопросы на естественном языке, и бот автоматически сгенерирует SQL-запросы и предоставит аналитику!

🔍 <b>ОБЯЗАТЕЛЬНО: Для SQL запросов к базе данных используйте префикс:</b>
• <b>sql:</b> - например: <code>sql: Показать транзакции за сегодня</code>

⚠️ <b>Без префикса</b> бот может неправильно определить тип запроса и ответить как в обычном чате, а не выполнить SQL запрос к базе данных.

📊 <b>Примеры вопросов (с префиксом sql:):</b>
• <code>sql:</code> Сколько транзакций было сегодня?
• <code>sql:</code> Топ 10 городов по объему транзакций
• <code>sql:</code> Средний чек для карт Halyk Bank
• <code>sql:</code> Объем транзакций по категориям за месяц
• <code>sql:</code> Распределение транзакций по валютам

📋 <b>Указание формата вывода:</b>
Вы можете явно указать желаемый формат вывода в запросе:
• <b>Таблица:</b> добавьте слова "таблица", "table", "таблицу" в запрос
  Пример: "Покажи топ категорий таблица"
• <b>Диаграмма:</b> добавьте слова "диаграмма", "chart", "график", "визуализация" в запрос
  Пример: "Распределение по валютам диаграмма"
• <b>Автоматически:</b> если не указано, бот сам выберет подходящий формат

✨ <b>Особенности:</b>
• Автоматическая генерация SQL из вопросов
• Детальная аналитика с инсайтами
• Экспорт данных в CSV
• Генерация диаграмм
• Поддержка русского, английского и казахского языков
• Контекстная память ваших запросов

Используйте конкретные вопросы для лучших результатов. Бот понимает естественный язык и автоматически оптимизирует запросы к базе данных."#
        .to_string()
}

pub fn create_suggestions_keyboard(questions: &[String]) -> teloxide::types::ReplyMarkup {
    use teloxide::types::InlineKeyboardButton;
    
    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    
    // Размещаем кнопки по одной в ряд для лучшей читаемости
    for question in questions.iter().take(6) {
        // Обрезаем текст кнопки до 40 символов для лучшей читаемости
        // Telegram позволяет до 64 символов, но лучше сделать короче для читаемости
        let button_text = if question.chars().count() > 40 {
            let truncated: String = question.chars().take(37).collect();
            format!("{}...", truncated)
        } else {
            question.to_string()
        };
        
        // Создаем callback данные, ограничивая их до 64 байт (лимит Telegram)
        // Telegram ограничивает callback_data до 64 байт
        let max_callback_len = 64;
        let prefix = "query:";
        let max_question_len = max_callback_len - prefix.len();
        
        // Обрезаем вопрос до максимальной длины (с учетом UTF-8)
        let truncated_question = if question.as_bytes().len() > max_question_len {
            // Безопасно обрезаем по байтам, но не разрываем UTF-8 символы
            let bytes = question.as_bytes();
            let mut len = max_question_len;
            while len > 0 && !std::str::from_utf8(&bytes[..len]).is_ok() {
                len -= 1;
            }
            std::str::from_utf8(&bytes[..len]).unwrap_or("").to_string()
        } else {
            question.to_string()
        };
        
        let callback_data = format!("{}{}", prefix, truncated_question);
        
        // Финальная проверка - если все еще слишком длинный, обрезаем еще больше
        let callback_data = if callback_data.as_bytes().len() > max_callback_len {
            let bytes = callback_data.as_bytes();
            let mut len = max_callback_len;
            while len > 0 && !std::str::from_utf8(&bytes[..len]).is_ok() {
                len -= 1;
            }
            std::str::from_utf8(&bytes[..len]).unwrap_or("").to_string()
        } else {
            callback_data
        };
        
        keyboard.push(vec![InlineKeyboardButton::callback(button_text, callback_data)]);
    }
    
    teloxide::types::ReplyMarkup::InlineKeyboard(teloxide::types::InlineKeyboardMarkup::new(keyboard))
}

fn escape_html(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}
