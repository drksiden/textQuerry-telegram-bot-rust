use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Clone)]
pub enum OutputType {
    #[serde(rename = "table")]
    Table,
    #[serde(rename = "chart")]
    Chart,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "auto")]
    Auto,
}

impl Default for OutputType {
    fn default() -> Self {
        OutputType::Auto
    }
}

#[derive(Debug, Serialize)]
pub struct QueryRequest {
    pub question: String,
    #[serde(default)]
    pub include_analysis: bool,
    #[serde(default)]
    pub use_cache: bool,
    #[serde(default)]
    pub include_sql: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default)]
    pub output_type: OutputType,
}

#[derive(Debug, Deserialize)]
pub struct QueryResponse {
    pub question: String,
    #[serde(default)]
    pub sql: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_response: Option<String>,
    pub data: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_data: Option<ChartData>,
    pub execution_time_ms: u64,
    pub row_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<AnalysisResult>,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChartData {
    pub chart_type: String,
    pub labels: Vec<String>,
    pub datasets: Vec<ChartDataset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChartDataset {
    pub label: String,
    pub data: Vec<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnalysisResult {
    pub headline: String,
    pub insights: Vec<Insight>,
    pub explanation: String,
    pub suggested_questions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Insight {
    pub title: String,
    pub description: String,
    pub significance: String,
}

#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub response_time_ms: u64,
}

pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    pub async fn query(&self, request: QueryRequest) -> Result<QueryResponse> {
        let url = format!("{}/api/query", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request to backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Backend error ({}): {}", status, text);
        }

        let query_response: QueryResponse = response
            .json()
            .await
            .context("Failed to parse backend response")?;

        Ok(query_response)
    }

    pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request to backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Backend error ({}): {}", status, text);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse backend response")?;

        Ok(chat_response)
    }

    pub async fn clear_context(&self, user_id: &str) -> Result<()> {
        let url = format!("{}/api/context/clear", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "user_id": user_id }))
            .send()
            .await
            .context("Failed to send request to backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Backend error ({}): {}", status, text);
        }

        Ok(())
    }

    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/health", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to backend")?;

        Ok(response.status().is_success())
    }
}

