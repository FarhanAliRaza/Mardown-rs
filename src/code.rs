use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct AppError(String);

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for AppError {}

// Anthropic API types
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum ContentBlock {
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

#[derive(Serialize, Deserialize, Debug)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct MessagesResponse {
    id: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Tool {
    name: String,
    description: String,
    input_schema: ToolSchema,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ToolSchema {
    #[serde(rename = "type")]
    schema_type: String,
    properties: HashMap<String, ToolSchemaProperty>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ToolSchemaProperty {
    #[serde(rename = "type")]
    property_type: String,
    description: String,
}

// Tool definitions
type ToolFunction = fn(Value) -> Result<String>;

struct ToolDefinition {
    name: String,
    description: String,
    schema: ToolSchema,
    function: ToolFunction,
}

impl ToolDefinition {
    fn to_api_tool(&self) -> Tool {
        Tool {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.schema.clone(),
        }
    }
}

// Agent implementation
struct Agent {
    client: Client,
    tools: Vec<ToolDefinition>,
}

impl Agent {
    fn new() -> Result<Self> {
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

        Ok(Agent {
            client,
            tools: vec![
                read_file_definition(),
                list_files_definition(),
                edit_file_definition(),
            ],
        })
    }

    async fn run(&self) -> Result<()> {
        let mut conversation: Vec<Message> = Vec::new();
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut buffer = String::new();

        println!("Chat with Claude (use 'ctrl-c' to quit)");

        let mut read_user_input = true;
        loop {
            if read_user_input {
                print!("\x1b[94mYou\x1b[0m: ");
                io::stdout().flush()?;

                buffer.clear();
                if reader.read_line(&mut buffer)? == 0 {
                    break;
                }

                let user_input = buffer.trim().to_string();
                if user_input.is_empty() {
                    continue;
                }

                conversation.push(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text { text: user_input }],
                });
            }

            let message = self.run_inference(&conversation).await?;

            // Add assistant's response to conversation
            let mut assistant_message = Message {
                role: "assistant".to_string(),
                content: Vec::new(),
            };

            let mut tool_results = Vec::new();

            for content in &message.content {
                match content {
                    ContentBlock::Text { text } => {
                        println!("\x1b[93mClaude\x1b[0m: {}", text);
                        assistant_message
                            .content
                            .push(ContentBlock::Text { text: text.clone() });
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        println!("\x1b[92mtool\x1b[0m: {}({})", name, input);

                        // Find the tool and execute it
                        let tool_result = self.execute_tool(id, name, input);

                        // Add tool use to assistant message
                        assistant_message.content.push(ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        });

                        // Add tool result to the list
                        match tool_result {
                            Ok(content) => {
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content,
                                    error: None,
                                });
                            }
                            Err(err) => {
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: err.to_string(),
                                    error: Some(true),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }

            conversation.push(assistant_message);

            if tool_results.is_empty() {
                read_user_input = true;
                continue;
            }

            // Add tool results as a user message
            conversation.push(Message {
                role: "user".to_string(),
                content: tool_results,
            });

            read_user_input = false;
        }

        Ok(())
    }

    async fn run_inference(&self, conversation: &[Message]) -> Result<MessagesResponse> {
        let api_tools = self
            .tools
            .iter()
            .map(|tool| tool.to_api_tool())
            .collect::<Vec<_>>();

        let request = MessagesRequest {
            model: "claude-3-7-sonnet-20250219".to_string(),
            max_tokens: 1024,
            messages: conversation.to_vec(),
            tools: Some(api_tools),
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError(format!("API request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to get error details".to_string());
            return Err(Box::new(AppError(format!("API error: {}", error_text))));
        }

        let message: MessagesResponse = response
            .json()
            .await
            .map_err(|e| AppError(format!("Failed to parse API response: {}", e)))?;

        Ok(message)
    }

    fn execute_tool(&self, id: &str, name: &str, input: &Value) -> Result<String> {
        // Find the tool with the given name
        let tool = self
            .tools
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| AppError(format!("Tool not found: {}", name)))?;

        // Execute the tool function
        (tool.function)(input.clone())
    }
}

// Tool implementations
fn read_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path of a file in the working directory.".to_string(),
        },
    );

    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read the contents of a given relative file path. Use this when you want to see what's inside a file. Do not use this with directory names.".to_string(),
        schema: ToolSchema { 
            schema_type: "object".to_string(),
            properties 
        },
        function: read_file_function,
    }
}

fn read_file_function(input: Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing 'path' parameter".to_string()))?;

    let content = fs::read_to_string(path)
        .map_err(|e| AppError(format!("Failed to read file {}: {}", path, e)))?;

    Ok(content)
}

fn list_files_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "Optional relative path to list files from. Defaults to current directory if not provided.".to_string(),
        },
    );

    ToolDefinition {
        name: "list_files".to_string(),
        description: "List files and directories at a given path. If no path is provided, lists files in the current directory.".to_string(),
        schema: ToolSchema { 
            schema_type: "object".to_string(),
            properties 
        },
        function: list_files_function,
    }
}

fn list_files_function(input: Value) -> Result<String> {
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let mut files = Vec::new();

    visit_dirs(Path::new(path), &mut files)?;

    let json = serde_json::to_string(&files)
        .map_err(|e| AppError(format!("Failed to serialize file list: {}", e)))?;

    Ok(json)
}

fn visit_dirs(dir: &Path, files: &mut Vec<String>) -> Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            let relative_path = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            if path.is_dir() {
                files.push(format!("{}/", relative_path));
                // For a full recursive listing, uncomment the following line:
                // visit_dirs(&path, files)?;
            } else {
                files.push(relative_path);
            }
        }
    }

    Ok(())
}

fn edit_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The path to the file".to_string(),
        },
    );
    properties.insert(
        "old_str".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description:
                "Text to search for - must match exactly and must only have one match exactly"
                    .to_string(),
        },
    );
    properties.insert(
        "new_str".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "Text to replace old_str with".to_string(),
        },
    );

    ToolDefinition {
        name: "edit_file".to_string(),
        description: "Make edits to a text file. Replaces 'old_str' with 'new_str' in the given file. 'old_str' and 'new_str' MUST be different from each other. If the file specified with path doesn't exist, it will be created.".to_string(),
        schema: ToolSchema { 
            schema_type: "object".to_string(),
            properties 
        },
        function: edit_file_function,
    }
}

fn edit_file_function(input: Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing 'path' parameter".to_string()))?;

    let old_str = input
        .get("old_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing 'old_str' parameter".to_string()))?;

    let new_str = input
        .get("new_str")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing 'new_str' parameter".to_string()))?;

    if old_str == new_str {
        return Err(Box::new(AppError(
            "old_str and new_str must be different".to_string(),
        )));
    }

    if !Path::new(path).exists() && old_str.is_empty() {
        return create_new_file(path, new_str);
    }

    let content = fs::read_to_string(path)
        .map_err(|e| AppError(format!("Failed to read file {}: {}", path, e)))?;

    let new_content = content.replace(old_str, new_str);

    if content == new_content && !old_str.is_empty() {
        return Err(Box::new(AppError("old_str not found in file".to_string())));
    }

    fs::write(path, new_content)
        .map_err(|e| AppError(format!("Failed to write to file {}: {}", path, e)))?;

    Ok("OK".to_string())
}

fn create_new_file(file_path: &str, content: &str) -> Result<String> {
    let path = Path::new(file_path);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError(format!("Failed to create directory: {}", e)))?;
        }
    }

    fs::write(path, content).map_err(|e| AppError(format!("Failed to create file: {}", e)))?;

    Ok(format!("Successfully created file {}", file_path))
}

pub async fn main() -> Result<()> {
    match Agent::new() {
        Ok(agent) => agent.run().await,
        Err(err) => {
            eprintln!("Failed to initialize agent: {}", err);
            process::exit(1);
        }
    }
}
