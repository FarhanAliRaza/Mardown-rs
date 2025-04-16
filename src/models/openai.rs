use super::{AppError, ContentBlock, Message, Model, ModelResponse, Tool, ToolSchema};
use async_trait::async_trait;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;

// --- OpenAI Specific API Structures ---

// Request structures
#[derive(Serialize, Debug)]
struct OpenAIChatCompletionRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>, // Can be "none", "auto", or {"type": "function", "function": {"name": "my_function"}}
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    // Add other optional parameters like top_p, frequency_penalty etc. if needed
    #[serde(default)]
    stream: bool, // Set to false for non-streaming
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAIMessage {
    role: String,            // "system", "user", "assistant", or "tool"
    content: Option<String>, // Make content optional to handle null
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>, // For assistant messages requesting tool use
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>, // For tool messages providing results
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>, // Optional name for the tool call function
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String, // Currently only "function" is supported
    function: OpenAIFunction,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: Value, // JSON Schema object
}

// Response structures
#[derive(Deserialize, Debug)]
struct OpenAIChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<OpenAIChoice>,
    // usage: Option<OpenAIUsage>, // Add usage if needed
    // system_fingerprint: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenAIChoice {
    index: u32,
    message: OpenAIMessage,
    // logprobs: Option<Value>, // Add logprobs if needed
    finish_reason: String, // e.g., "stop", "length", "tool_calls"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String, // Always "function" for now
    function: OpenAIFunctionCall,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String, // JSON string of arguments
}

// --- OpenAI Model Implementation ---

pub struct OpenAIModel {
    client: Client,
    model_name: String,
    api_key: String,
    // enable_tools: bool, // OpenAI tools are generally enabled if provided
}

impl OpenAIModel {
    pub fn new(model_name: String) -> Result<Self, AppError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AppError("Please set OPENAI_API_KEY environment variable".to_string()))?;

        let client = Client::builder()
            .build()
            .map_err(|e| AppError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(OpenAIModel {
            client,
            model_name,
            api_key,
        })
    }

    // --- Conversion Logic ---

    // TODO: Implement message/tool conversion functions
    /// Convert Tool to OpenAI Tool format
    fn convert_to_openai_tools(tools: &[Tool]) -> Vec<OpenAITool> {
        tools
            .iter()
            .map(|tool| {
                // Convert ToolSchema to OpenAI function parameters
                let parameters = json!({
                    "type": tool.input_schema.schema_type, // Should be "object"
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

                OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunction {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters,
                    },
                }
            })
            .collect()
    }

    /// Convert our Message format to OpenAI message format
    fn convert_to_openai_messages(conversation: &[Message]) -> Vec<OpenAIMessage> {
        let mut openai_messages: Vec<OpenAIMessage> = Vec::new();

        // Add a default system message if not present
        let has_system = conversation.iter().any(|msg| msg.role == "system");
        if !has_system {
            openai_messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some("You are a helpful assistant.".to_string()), // Use Some() rather than None for requests
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }

        // Process each message
        for msg in conversation {
            let role = match msg.role.as_str() {
                "user" => "user",
                "assistant" => "assistant",
                "system" => "system",
                "tool" => "tool", // Added for handling tool results explicitly
                _ => continue,    // Skip unknown roles
            };

            // Handle different message types and content blocks
            match role {
                "user" | "system" => {
                    // Combine all text blocks into a single string for user/system
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

                    openai_messages.push(OpenAIMessage {
                        role: role.to_string(),
                        content: Some(content), // Always use Some() even for empty strings
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                "assistant" => {
                    // Handle potential text and tool calls from assistant
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

                    let tool_calls: Vec<OpenAIToolCall> = msg
                        .content
                        .iter()
                        .filter_map(|block| {
                            if let ContentBlock::ToolUse { id, name, input } = block {
                                Some(OpenAIToolCall {
                                    id: id.clone(),
                                    call_type: "function".to_string(),
                                    function: OpenAIFunctionCall {
                                        name: name.clone(),
                                        arguments: input.to_string(), // Arguments need to be a JSON string
                                    },
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    openai_messages.push(OpenAIMessage {
                        role: "assistant".to_string(),
                        content: Some(text_content), // Always use Some() even for empty strings
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                        name: None,
                    });
                }
                "tool" => {
                    // Handle tool results, expecting one ToolResult block per message
                    for block in &msg.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = block
                        {
                            openai_messages.push(OpenAIMessage {
                                role: "tool".to_string(),
                                content: Some(content.clone()), // Always use Some() for content in requests
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id.clone()),
                                name: None, // OpenAI doesn't seem to use name here, but associate via tool_call_id
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Filter out any messages with empty content and no tool calls/results
        // to avoid sending useless messages
        openai_messages.retain(|msg| {
            let has_content = msg.content.as_ref().map_or(false, |c| !c.is_empty());
            let has_tool_calls = msg.tool_calls.is_some();
            let has_tool_call_id = msg.tool_call_id.is_some();

            has_content || has_tool_calls || has_tool_call_id
        });

        openai_messages
    }

    /// Convert OpenAI response to our ModelResponse format
    fn convert_from_openai_response(
        openai_response: OpenAIChatCompletionResponse,
    ) -> Result<ModelResponse, AppError> {
        // Get the first choice or return error
        let first_choice = openai_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError("OpenAI API returned no choices".to_string()))?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        let message = first_choice.message;

        // Add text content only if present and not empty
        if let Some(text_content) = message.content {
            if !text_content.is_empty() {
                content_blocks.push(ContentBlock::Text { text: text_content });
            }
        }

        // Add tool calls if present
        if let Some(tool_calls) = message.tool_calls {
            for tool_call in tool_calls {
                // Parse arguments from string to Value
                let args = serde_json::from_str::<Value>(&tool_call.function.arguments)
                    .map_err(|e| AppError(format!("Failed to parse tool arguments: {}", e)))?;

                content_blocks.push(ContentBlock::ToolUse {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    input: args,
                });
            }
        }

        Ok(ModelResponse {
            id: Some(openai_response.id),
            content: content_blocks,
        })
    }
}

// TODO: Implement Model trait for OpenAIModel
#[async_trait]
impl Model for OpenAIModel {
    async fn run_inference(
        &self,
        conversation: &[Message],
        tools: Option<&[Tool]>,
    ) -> Result<ModelResponse, AppError> {
        // Convert to OpenAI format
        let openai_messages = Self::convert_to_openai_messages(conversation);

        if openai_messages.is_empty() {
            return Err(AppError(
                "No valid messages to send to OpenAI API".to_string(),
            ));
        }

        // Handle tools
        let openai_tools = tools.map(Self::convert_to_openai_tools);

        // Determine tool_choice based on whether tools are provided
        let tool_choice = if openai_tools.is_some() && !openai_tools.as_ref().unwrap().is_empty() {
            Some(json!("auto")) // or "none", or specific function
        } else {
            None
        };

        // Build request
        let request = OpenAIChatCompletionRequest {
            model: self.model_name.clone(),
            messages: openai_messages,
            tools: openai_tools,
            tool_choice,
            temperature: Some(0.7), // Example temperature
            max_tokens: Some(1000), // Example max tokens
            stream: false,
        };

        // Send request to OpenAI API
        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError(format!("OpenAI API request failed: {}", e)))?;

        // Check for errors
        let status = response.status(); // Store status first
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to get error details".to_string());
            return Err(AppError(format!(
                "OpenAI API error {}: {}",
                status, error_text
            )));
        }

        // --- Remove Debugging: Print raw response body ---
        let raw_body = response
            .text()
            .await
            .map_err(|e| AppError(format!("Failed to read OpenAI API response body: {}", e)))?;
        // println!("--- OpenAI Raw Response ---");
        // println!("{}", raw_body);
        // println!("---------------------------");
        // --- End Debugging ---

        // Parse and convert response
        // Now parse from the captured raw_body string
        let openai_response: OpenAIChatCompletionResponse = serde_json::from_str(&raw_body)
            .map_err(|e| {
                AppError(format!(
                    "Failed to parse OpenAI API response: {} \nRaw body: {}",
                    e,
                    raw_body // Include raw body in error message
                ))
            })?;

        Self::convert_from_openai_response(openai_response)
    }

    fn supports_tools(&self) -> bool {
        // OpenAI generally supports tools if provided in the request
        true
    }

    fn name(&self) -> &'static str {
        "OpenAI"
    }
}

// TODO: Implement default_openai() helper function
// Helper function to create a default OpenAI model instance
pub fn default_openai() -> Result<OpenAIModel, AppError> {
    // Get model name from env or use default (e.g., gpt-4o)
    let model_name = env::var("OPENAI_MODEL_NAME").unwrap_or_else(|_| "gpt-4.1".to_string());
    OpenAIModel::new(model_name)
}
