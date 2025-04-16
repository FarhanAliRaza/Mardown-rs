use super::{AppError, ContentBlock, Message, Model, ModelResponse, Tool};
use async_trait::async_trait;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;

// --- DeepSeek Specific API Structures ---

#[derive(Serialize, Debug)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<DeepSeekTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(default)]
    stream: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DeepSeekMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<DeepSeekToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DeepSeekToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String, // Usually "function"
    function: DeepSeekFunctionCall,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DeepSeekFunctionCall {
    name: String,
    arguments: String, // JSON string of arguments
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DeepSeekTool {
    #[serde(rename = "type")]
    tool_type: String, // Usually "function"
    function: DeepSeekFunction,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DeepSeekFunction {
    name: String,
    description: String,
    parameters: Value, // JSON schema
}

#[derive(Deserialize, Debug)]
struct DeepSeekResponse {
    id: String,
    choices: Vec<DeepSeekChoice>,
    // Other fields like usage, etc.
}

#[derive(Deserialize, Debug)]
struct DeepSeekChoice {
    index: u32,
    message: DeepSeekMessage,
    finish_reason: String,
}

// --- DeepSeek Model Implementation ---

pub struct DeepSeekModel {
    client: Client,
    model_name: String,
    api_key: String,
    enable_tools: bool,
}

impl DeepSeekModel {
    pub fn new(model_name: String) -> Result<Self, AppError> {
        let api_key = env::var("DEEPSEEK_API_KEY").map_err(|_| {
            AppError("Please set DEEPSEEK_API_KEY environment variable".to_string())
        })?;

        // Check for environment variable to enable tools
        let enable_tools = env::var("DEEPSEEK_ENABLE_TOOLS")
            .map(|val| val.to_lowercase() == "true" || val == "1")
            .unwrap_or(true); // Enable by default

        let client = Client::builder()
            .build()
            .map_err(|e| AppError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(DeepSeekModel {
            client,
            model_name,
            api_key,
            enable_tools,
        })
    }

    // --- Conversion Logic ---

    /// Convert Tool to DeepSeek Tool format
    fn convert_to_deepseek_tools(tools: &[Tool]) -> Vec<DeepSeekTool> {
        tools
            .iter()
            .map(|tool| {
                // Convert ToolSchema to DeepSeek function parameters
                let parameters = json!({
                    "type": tool.input_schema.schema_type,
                    "properties": tool.input_schema.properties.iter()
                        .map(|(name, prop)| {
                            (name.clone(), json!({
                                "type": prop.property_type,
                                "description": prop.description
                            }))
                        })
                        .collect::<HashMap<String, Value>>(),
                    "required": tool.input_schema.required
                });

                DeepSeekTool {
                    tool_type: "function".to_string(),
                    function: DeepSeekFunction {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters,
                    },
                }
            })
            .collect()
    }

    /// Convert our Message format to DeepSeek message format
    fn convert_to_deepseek_messages(
        conversation: &[Message],
        system_prompt: Option<&str>,
    ) -> Vec<DeepSeekMessage> {
        let mut deepseek_messages: Vec<DeepSeekMessage> = Vec::new();

        // Add the system prompt if provided
        if let Some(prompt) = system_prompt {
            deepseek_messages.push(DeepSeekMessage {
                role: "system".to_string(),
                content: prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        } else {
            // Add a default system message if no specific prompt is given
            let has_system = conversation.iter().any(|msg| msg.role == "system");
            if !has_system {
                deepseek_messages.push(DeepSeekMessage {
                    role: "system".to_string(),
                    content: "You are a helpful assistant.".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }
        }

        // Process each message
        for msg in conversation {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "assistant",
                "system" => "system",
                _ => continue, // Skip unknown roles
            };

            // Handle different message types
            match &msg.content[..] {
                // If the message has only one text block, convert directly
                [ContentBlock::Text { text }] => {
                    deepseek_messages.push(DeepSeekMessage {
                        role: role.to_string(),
                        content: text.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                // If the message has tool use blocks from the assistant
                _ if role == "assistant" => {
                    // Extract text content and tool calls separately
                    let text_content: String = msg
                        .content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::Text { text } = block {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<&str>>()
                        .join("\n");

                    // Extract tool calls
                    let tool_calls: Vec<DeepSeekToolCall> = msg
                        .content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::ToolUse { id, name, input } = block {
                                Some(DeepSeekToolCall {
                                    id: id.clone(),
                                    call_type: "function".to_string(),
                                    function: DeepSeekFunctionCall {
                                        name: name.clone(),
                                        arguments: input.to_string(),
                                    },
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !text_content.is_empty() || !tool_calls.is_empty() {
                        deepseek_messages.push(DeepSeekMessage {
                            role: "assistant".to_string(),
                            content: text_content,
                            tool_calls: if tool_calls.is_empty() {
                                None
                            } else {
                                Some(tool_calls)
                            },
                            tool_call_id: None,
                            name: None,
                        });
                    }
                }
                // Handle tool results from the user
                _ if role == "user" => {
                    // Extract text content
                    let mut text_content = String::new();

                    for block in &msg.content {
                        match block {
                            ContentBlock::Text { text } => {
                                if !text_content.is_empty() {
                                    text_content.push_str("\n");
                                }
                                text_content.push_str(text);
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => {
                                // Add tool result as a separate message
                                deepseek_messages.push(DeepSeekMessage {
                                    role: "tool".to_string(), // DeepSeek uses "tool" role for results
                                    content: content.clone(),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_use_id.clone()),
                                    name: None, // Tool name could be extracted if needed
                                });
                            }
                            _ => {}
                        }
                    }

                    // Add the user message if it has text content
                    if !text_content.is_empty() {
                        deepseek_messages.push(DeepSeekMessage {
                            role: "user".to_string(),
                            content: text_content,
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });
                    }
                }
                // System messages or other roles with multiple content blocks
                _ => {
                    // Combine all text blocks
                    let content = msg
                        .content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::Text { text } = block {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<&str>>()
                        .join("\n");

                    if !content.is_empty() {
                        deepseek_messages.push(DeepSeekMessage {
                            role: role.to_string(),
                            content,
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });
                    }
                }
            }
        }

        deepseek_messages
    }

    /// Convert DeepSeek response to our ModelResponse format
    fn convert_from_deepseek_response(
        deepseek_response: DeepSeekResponse,
    ) -> Result<ModelResponse, AppError> {
        // Get the first choice or return error
        let first_choice = deepseek_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError("DeepSeek API returned no choices".to_string()))?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        // Add text content if not empty
        if !first_choice.message.content.is_empty() {
            content_blocks.push(ContentBlock::Text {
                text: first_choice.message.content,
            });
        }

        // Add tool calls if present
        if let Some(tool_calls) = first_choice.message.tool_calls {
            for tool_call in tool_calls {
                // Parse arguments from string to Value
                let args = serde_json::from_str::<Value>(&tool_call.function.arguments)
                    .unwrap_or_else(|_| json!({}));

                content_blocks.push(ContentBlock::ToolUse {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    input: args,
                });
            }
        }

        Ok(ModelResponse {
            id: Some(deepseek_response.id),
            content: content_blocks,
        })
    }
}

#[async_trait]
impl Model for DeepSeekModel {
    async fn run_inference(
        &self,
        conversation: &[Message],
        tools: Option<&[Tool]>,
        system_prompt: Option<&str>,
    ) -> Result<ModelResponse, AppError> {
        // Convert to DeepSeek format, passing the system prompt
        let deepseek_messages = Self::convert_to_deepseek_messages(conversation, system_prompt);

        if deepseek_messages.is_empty() {
            return Err(AppError(
                "No valid messages to send to DeepSeek API".to_string(),
            ));
        }

        // Handle tools if supported and provided
        let deepseek_tools = if self.supports_tools() && tools.is_some() {
            let tool_defs = Self::convert_to_deepseek_tools(tools.unwrap());
            if !tool_defs.is_empty() {
                Some(tool_defs)
            } else {
                None
            }
        } else {
            None
        };

        // Check if tools exist before moving the value
        let has_tools = deepseek_tools.is_some();

        // Build request
        let request = DeepSeekRequest {
            model: self.model_name.clone(),
            messages: deepseek_messages,
            tools: deepseek_tools,
            tool_choice: if has_tools {
                Some("auto".to_string())
            } else {
                None
            },
            temperature: Some(0.7), // Default temperature
            max_tokens: Some(1000), // Reasonable default max tokens
            stream: false,          // Don't use streaming
        };

        // Send request to DeepSeek API
        let response = self
            .client
            .post("https://api.deepseek.com/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError(format!("DeepSeek API request failed: {}", e)))?;

        // Check for errors
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to get error details".to_string());
            return Err(AppError(format!(
                "DeepSeek API error {}: {}",
                status, error_text
            )));
        }

        // Parse and convert response
        let deepseek_response: DeepSeekResponse = response
            .json()
            .await
            .map_err(|e| AppError(format!("Failed to parse DeepSeek API response: {}", e)))?;

        Self::convert_from_deepseek_response(deepseek_response)
    }

    fn supports_tools(&self) -> bool {
        self.enable_tools
    }

    fn name(&self) -> &'static str {
        "DeepSeek"
    }
}

// Helper function to create a default DeepSeek model instance
pub fn default_deepseek() -> Result<DeepSeekModel, AppError> {
    // Get model name from env or use default
    let model_name =
        env::var("DEEPSEEK_MODEL_NAME").unwrap_or_else(|_| "deepseek-chat".to_string());
    DeepSeekModel::new(model_name)
}
