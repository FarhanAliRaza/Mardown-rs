// src/models/google.rs
use super::{AppError, ContentBlock, Message, Model, ModelResponse, Tool}; // Use types from parent mod
use async_trait::async_trait;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json}; // Value/json might not be needed here
use std::collections::HashMap;
use std::env;

// --- Google Specific API Structures ---

#[derive(Serialize, Debug)]
struct GoogleGenerateContentRequest {
    contents: Vec<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GoogleContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GoogleTool>>,
    // Add other configs later if needed
    // generation_config: Option<GenerationConfig>,
    // safety_settings: Option<Vec<SafetySetting>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GoogleContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>, // "user" or "model"
    parts: Vec<GooglePart>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum GooglePart {
    Text {
        text: String,
    },
    FunctionCall {
        function_call: GoogleFunctionCall,
    },
    FunctionResponse {
        function_response: GoogleFunctionResponse,
    },
}

// Function call from model
#[derive(Serialize, Deserialize, Debug, Clone)]
struct GoogleFunctionCall {
    name: String,
    args: Value, // JSON object
}

// Function response from client
#[derive(Serialize, Deserialize, Debug, Clone)]
struct GoogleFunctionResponse {
    name: String,
    response: Value, // JSON object
}

// Tools configuration
#[derive(Serialize, Debug)]
struct GoogleTool {
    function_declarations: Vec<GoogleFunctionDeclaration>,
}

#[derive(Serialize, Debug)]
struct GoogleFunctionDeclaration {
    name: String,
    description: String,
    parameters: GoogleFunctionParameters,
}

#[derive(Serialize, Debug)]
struct GoogleFunctionParameters {
    #[serde(rename = "type")]
    parameter_type: String, // Always "OBJECT" for now
    properties: HashMap<String, GoogleParameterProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required: Option<Vec<String>>,
}

#[derive(Serialize, Debug)]
struct GoogleParameterProperty {
    #[serde(rename = "type")]
    property_type: String, // "STRING", "NUMBER", etc.
    description: String,
}

#[derive(Deserialize, Debug)]
struct GoogleGenerateContentResponse {
    #[serde(default)]
    candidates: Vec<GoogleCandidate>,
    // Add other fields if needed
}

#[derive(Deserialize, Debug)]
struct GoogleCandidate {
    content: GoogleContent,
    // Add other fields if needed
}

// --- Google Model Implementation ---

pub struct GoogleModel {
    client: Client,
    model_name: String, // e.g., "gemini-2.5-pro-preview-03-25"
    api_key: String,
    enable_tools: bool, // Flag to control tool support
}

impl GoogleModel {
    pub fn new(model_name: String) -> Result<Self, AppError> {
        let api_key = env::var("GOOGLE_API_KEY")
            .map_err(|_| AppError("Please set GOOGLE_API_KEY environment variable".to_string()))?;

        // Check for environment variable to enable tools
        let enable_tools = env::var("GOOGLE_ENABLE_TOOLS")
            .map(|val| val.to_lowercase() == "true" || val == "1")
            .unwrap_or(true); // Enable by default if var not set

        let client = Client::builder()
            .build()
            .map_err(|e| AppError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(GoogleModel {
            client,
            model_name,
            api_key,
            enable_tools,
        })
    }

    // --- Conversion Logic ---

    /// Converts our common Tool format to Google's FunctionDeclaration format
    fn convert_to_google_functions(tools: &[Tool]) -> Vec<GoogleFunctionDeclaration> {
        tools
            .iter()
            .map(|tool| {
                // Convert properties to Google format
                let properties = tool
                    .input_schema
                    .properties
                    .iter()
                    .map(|(name, prop)| {
                        // Convert property type to Google's uppercase format
                        let google_type = match prop.property_type.to_uppercase().as_str() {
                            "STRING" | "NUMBER" | "BOOLEAN" | "ARRAY" | "OBJECT" => {
                                prop.property_type.to_uppercase()
                            }
                            // Default to STRING for simple types
                            "string" => "STRING".to_string(),
                            "integer" | "number" => "NUMBER".to_string(),
                            "boolean" => "BOOLEAN".to_string(),
                            "array" => "ARRAY".to_string(),
                            "object" => "OBJECT".to_string(),
                            _ => "STRING".to_string(), // Default fallback
                        };

                        (
                            name.clone(),
                            GoogleParameterProperty {
                                property_type: google_type,
                                description: prop.description.clone(),
                            },
                        )
                    })
                    .collect();

                // Convert required fields if present
                let required = tool.input_schema.required.clone();

                GoogleFunctionDeclaration {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: GoogleFunctionParameters {
                        parameter_type: "OBJECT".to_string(), // Always OBJECT per Google spec
                        properties,
                        required,
                    },
                }
            })
            .collect()
    }

    /// Converts common Message format to Google's Content format.
    fn convert_to_google_contents(conversation: &[Message]) -> Vec<GoogleContent> {
        conversation
            .iter()
            .filter_map(|msg| {
                let role = match msg.role.as_str() {
                    "user" => Some("user".to_string()),
                    "assistant" => Some("model".to_string()),
                    _ => None,
                };

                // Process content blocks
                let parts: Vec<GooglePart> = msg
                    .content
                    .iter()
                    .filter_map(|block| {
                        match block {
                            ContentBlock::Text { text } => {
                                Some(GooglePart::Text { text: text.clone() })
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                // Convert to Google's function_call format
                                Some(GooglePart::FunctionCall {
                                    function_call: GoogleFunctionCall {
                                        name: name.clone(),
                                        args: input.clone(),
                                    },
                                })
                            }
                            ContentBlock::ToolResult {
                                tool_use_id: _,
                                content,
                                error,
                            } => {
                                // Check the response for tool/function name (expected to be in the preceding message)
                                // For simplicity, we'll include the entire error status in the response
                                let response_value = if let Some(true) = error {
                                    json!({
                                        "result": content,
                                        "error": true
                                    })
                                } else {
                                    json!(content)
                                };

                                // In a real implementation, we would need to look up the function name
                                // from the previous tool_use_id, but here we'll use a placeholder
                                Some(GooglePart::FunctionResponse {
                                    function_response: GoogleFunctionResponse {
                                        name: "unknown_function".to_string(), // Placeholder
                                        response: response_value,
                                    },
                                })
                            }
                        }
                    })
                    .collect();

                // Only include messages with valid parts
                if !parts.is_empty() {
                    Some(GoogleContent { role, parts })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Converts Google's response back to the common ModelResponse format.
    fn convert_from_google_response(
        google_response: GoogleGenerateContentResponse,
    ) -> Result<ModelResponse, AppError> {
        // Get the first candidate or return error
        let first_candidate = google_response
            .candidates
            .into_iter()
            .next()
            .ok_or_else(|| AppError("Google API returned no candidates".to_string()))?;

        // Convert content blocks
        let mut content: Vec<ContentBlock> = Vec::new();

        for part in first_candidate.content.parts {
            match part {
                GooglePart::Text { text } => {
                    content.push(ContentBlock::Text { text });
                }
                GooglePart::FunctionCall { function_call } => {
                    // Generate a unique ID for the tool use
                    let id = format!("google_function_{}", chrono::Utc::now().timestamp_millis());

                    content.push(ContentBlock::ToolUse {
                        id,
                        name: function_call.name,
                        input: function_call.args,
                    });
                }
                GooglePart::FunctionResponse { .. } => {
                    // Function responses shouldn't be in the model's output
                    println!("Warning: Unexpected function_response in model output");
                }
            }
        }

        // Return the response with converted content
        Ok(ModelResponse { id: None, content })
    }
}

#[async_trait]
impl Model for GoogleModel {
    async fn run_inference(
        &self,
        conversation: &[Message],
        tools: Option<&[Tool]>,
        system_prompt: Option<&str>,
    ) -> Result<ModelResponse, AppError> {
        // Handle tools if supported and provided
        let google_tools = if self.supports_tools() && tools.is_some() {
            let function_declarations = Self::convert_to_google_functions(tools.unwrap());
            if !function_declarations.is_empty() {
                Some(vec![GoogleTool {
                    function_declarations,
                }])
            } else {
                None
            }
        } else {
            None
        };

        // Convert conversation to Google format
        let google_contents = Self::convert_to_google_contents(conversation);

        // Convert system prompt to Google format if provided
        let system_instruction = system_prompt.map(|prompt| GoogleContent {
            role: None, // System instructions don't have a role in Google API
            parts: vec![GooglePart::Text {
                text: prompt.to_string(),
            }],
        });

        // Check if conversion resulted in empty contents
        if google_contents.is_empty() {
            return Err(AppError(
                "Conversation yielded no content compatible with the Google API format".to_string(),
            ));
        }

        // Build request
        let request = GoogleGenerateContentRequest {
            contents: google_contents,
            system_instruction,
            tools: google_tools,
        };

        // Create API URL with the model and API key
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model_name, self.api_key
        );

        // Send request to Google API
        let response = self
            .client
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError(format!("Google API request failed: {}", e)))?;

        // Check for errors
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to get error details".to_string());
            return Err(AppError(format!(
                "Google API error {}: {}",
                status, error_text
            )));
        }

        // Deserialize the response
        let google_response: GoogleGenerateContentResponse = response
            .json()
            .await
            .map_err(|e| AppError(format!("Failed to parse Google API response: {}", e)))?;

        // Convert and return
        Self::convert_from_google_response(google_response)
    }

    fn supports_tools(&self) -> bool {
        // Return the tool support flag
        self.enable_tools
    }

    fn name(&self) -> &'static str {
        "Google"
    }
}

// Helper function to create a default Google model instance
pub fn default_google() -> Result<GoogleModel, AppError> {
    // Get model name from env or use default
    let model_name = env::var("GOOGLE_MODEL_NAME")
        .unwrap_or_else(|_| "gemini-2.5-pro-preview-03-25".to_string());
    GoogleModel::new(model_name)
}
