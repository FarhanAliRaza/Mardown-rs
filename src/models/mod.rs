use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// --- Common Structures (Potentially refactor from claude.rs / code.rs later if needed) ---
// These are based on Anthropic's format for now, as the main loop uses them.
// Google model implementation will need to adapt to/from this format.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<bool>,
    },
}

// This response structure is also based on Anthropic for now.
#[derive(Serialize, Deserialize, Debug)]
pub struct ModelResponse {
    pub id: Option<String>, // Make optional as Google might not have it directly
    pub content: Vec<ContentBlock>,
    // Add other fields if needed, e.g., usage statistics
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: ToolSchema,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: HashMap<String, ToolSchemaProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>, // Often needed for tools
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolSchemaProperty {
    #[serde(rename = "type")]
    pub property_type: String,
    pub description: String,
}

#[derive(Debug)]
pub(crate) struct AppError(pub String);

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AppError {}

// --- Model Trait ---

#[async_trait]
pub trait Model: Send + Sync {
    /// Runs inference with the model.
    async fn run_inference(
        &self,
        conversation: &[Message],
        tools: Option<&[Tool]>,
        system_prompt: Option<&str>,
    ) -> Result<ModelResponse, AppError>;

    /// Indicates if the model implementation supports tool use.
    fn supports_tools(&self) -> bool;

    /// Gets the name of the model implementation.
    fn name(&self) -> &'static str;
}

// Enum to select model type
pub enum ModelType {
    Claude,
    Google,
    DeepSeek,
    OpenAI,
}

// Need to declare the submodules
pub mod claude;
pub mod deepseek;
pub mod google;
pub mod openai;
