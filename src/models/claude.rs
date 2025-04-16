// src/models/claude.rs
use super::{AppError, ContentBlock, Message, Model, ModelResponse, Tool}; // Use types from parent mod
use async_trait::async_trait;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
// Value might not be needed here if not used directly
use std::env; // HashMap might not be needed here if not used directly

// --- Claude Specific API Structures ---

#[derive(Serialize, Deserialize, Debug)]
struct ClaudeMessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<Message>, // Reusing common Message struct
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>, // Reusing common Tool struct
}

// We map Claude's response directly to the common ModelResponse
// If Claude's response structure changes or has more fields, adjust ModelResponse in mod.rs
// or create a specific ClaudeMessagesResponse and map it.
type ClaudeMessagesResponse = ModelResponse;

// --- Claude Model Implementation ---

pub struct ClaudeModel {
    client: Client,
    model_name: String, // e.g., "claude-3-haiku-20240307"
}

impl ClaudeModel {
    pub fn new(model_name: String) -> Result<Self, AppError> {
        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| {
            AppError("Please set ANTHROPIC_API_KEY environment variable".to_string())
        })?;

        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            header::HeaderValue::from_str(&api_key)
                .map_err(|e| AppError(format!("Invalid API key: {}", e)))?,
        );
        headers.insert(
            "anthropic-version",
            // Consider making this configurable or updating it
            header::HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| AppError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(ClaudeModel { client, model_name })
    }
}

#[async_trait]
impl Model for ClaudeModel {
    async fn run_inference(
        &self,
        conversation: &[Message],
        tools: Option<&[Tool]>,
        system_prompt: Option<&str>,
    ) -> Result<ModelResponse, AppError> {
        let filtered_conversation: Vec<Message> = conversation
            .iter()
            .filter(|msg| {
                !msg.content.iter().any(|block| match block {
                    ContentBlock::ToolResult { content, .. } => content.is_empty(),
                    _ => false,
                })
            })
            .cloned()
            .collect();

        let request = ClaudeMessagesRequest {
            model: self.model_name.clone(),
            max_tokens: 4096,
            system: system_prompt.map(|s| s.to_string()),
            messages: filtered_conversation,
            tools: tools.map(|t| t.to_vec()),
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError(format!("Claude API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to get error details".to_string());
            return Err(AppError(format!(
                "Claude API error {}: {}",
                status, error_text
            )));
        }

        let message: ClaudeMessagesResponse = response
            .json()
            .await
            .map_err(|e| AppError(format!("Failed to parse Claude API response: {}", e)))?;

        Ok(message)
    }

    fn supports_tools(&self) -> bool {
        true // Claude supports tools
    }

    fn name(&self) -> &'static str {
        "Claude"
    }
}

// Helper function to create a default Claude model instance
pub fn default_claude() -> Result<ClaudeModel, AppError> {
    // You might want to make the model name configurable via env var or config file
    ClaudeModel::new("claude-3-sonnet-20240229".to_string())
}
