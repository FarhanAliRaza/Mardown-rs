use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::models::claude::default_claude;
use crate::models::deepseek::default_deepseek;
use crate::models::google::default_google;
use crate::models::openai::default_openai;
use crate::models::{
    AppError, ContentBlock, Message, Model, ModelResponse, ModelType, Tool, ToolSchema,
    ToolSchemaProperty,
};

type Result<T> = std::result::Result<T, AppError>;

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

pub struct Agent {
    model: Box<dyn Model>,
    tools: Vec<ToolDefinition>,
}

impl Agent {
    pub fn new(model_type: ModelType) -> Result<Self> {
        let model: Box<dyn Model> = match model_type {
            ModelType::Claude => Box::new(default_claude()?),
            ModelType::Google => Box::new(default_google()?),
            ModelType::DeepSeek => Box::new(default_deepseek()?),
            ModelType::OpenAI => Box::new(default_openai()?),
        };

        println!("Initialized Agent with {} model.", model.name());

        Ok(Agent {
            model,
            tools: vec![
                read_file_definition(),
                list_files_definition(),
                edit_file_definition(),
            ],
        })
    }

    pub async fn run(&self) -> Result<()> {
        let mut conversation: Vec<Message> = Vec::new();
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut buffer = String::new();

        println!("Chat with {} (use 'ctrl-c' to quit)", self.model.name());

        let mut read_user_input = true;
        loop {
            if read_user_input {
                print!("\x1b[94mYou\x1b[0m: ");
                io::stdout().flush().map_err(|e| AppError(e.to_string()))?;

                buffer.clear();
                if reader
                    .read_line(&mut buffer)
                    .map_err(|e| AppError(e.to_string()))?
                    == 0
                {
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

            let response = self.run_inference(&conversation).await?;

            let assistant_content = response.content.clone();
            let mut assistant_message = Message {
                role: "assistant".to_string(),
                content: assistant_content,
            };

            let mut tool_results = Vec::new();

            for content in response.content {
                match content {
                    ContentBlock::Text { text } => {
                        println!("\x1b[93m{}\x1b[0m: {}", self.model.name(), text);
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        if !self.model.supports_tools() {
                            println!(
                                "\x1b[91mWarning:\x1b[0m Model {} reported tool use, but implementation indicates no tool support. Skipping.",
                                self.model.name()
                            );
                            assistant_message.content.retain(|c| match c {
                                ContentBlock::ToolUse { id: msg_id, .. } => msg_id != &id,
                                _ => true,
                            });
                            continue;
                        }

                        println!("\x1b[92mtool\x1b[0m: {}({})", name, input);

                        let tool_result = self.execute_tool(&id, &name, &input);

                        match tool_result {
                            Ok(result_content) => {
                                println!("\x1b[32mtool_output[0m: {}", result_content);
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: result_content,
                                    error: None,
                                });
                            }
                            Err(err) => {
                                eprintln!(
                                    "\x1b[91mError executing tool '{}': {}\x1b[0m",
                                    name, err
                                );
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: err.to_string(),
                                    error: Some(true),
                                });
                            }
                        }
                    }
                    ContentBlock::ToolResult { .. } => {
                        eprintln!(
                            "\x1b[91mWarning:\x1b[0m Unexpected ToolResult block received directly from model response. Ignoring."
                        );
                        assistant_message
                            .content
                            .retain(|c| !matches!(c, ContentBlock::ToolResult { .. }));
                    }
                }
            }

            if !assistant_message.content.is_empty() {
                conversation.push(assistant_message);
            }

            if tool_results.is_empty() {
                read_user_input = true;
                continue;
            } else {
                // Determine the correct role based on the model
                // For OpenAI, we need to use "tool" role for tool responses
                let role = if self.model.name() == "OpenAI" {
                    "tool"
                } else {
                    "user" // Default for other models like Claude
                };

                // For OpenAI, we need to add individual tool messages for each tool result
                if self.model.name() == "OpenAI" {
                    for tool_result in tool_results {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            error,
                        } = tool_result
                        {
                            conversation.push(Message {
                                role: role.to_string(),
                                content: vec![ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    error,
                                }],
                            });
                        }
                    }
                } else {
                    // Bundle all tool results in one message for other models
                    conversation.push(Message {
                        role: role.to_string(),
                        content: tool_results,
                    });
                }

                read_user_input = false;
            }
        }

        Ok(())
    }

    async fn run_inference(&self, conversation: &[Message]) -> Result<ModelResponse> {
        let api_tools = if self.model.supports_tools() {
            Some(
                self.tools
                    .iter()
                    .map(|t| t.to_api_tool())
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };

        self.model
            .run_inference(conversation, api_tools.as_deref())
            .await
    }

    fn execute_tool(&self, _id: &str, name: &str, input: &Value) -> Result<String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| AppError(format!("Tool '{}' not found.", name)))?;

        (tool.function)(input.clone())
    }
}

fn read_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path of the file to read.".to_string(),
        },
    );
    let required = vec!["path".to_string()];

    ToolDefinition {
        name: "read_file".to_string(),
        description: "Read the entire contents of a given relative file path. Use this when you want to see what's inside a file.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(required),
        },
        function: read_file_function,
    }
}

fn read_file_function(input: Value) -> Result<String> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'path' parameter for read_file".to_string()))?;

    fs::read_to_string(path).map_err(|e| AppError(format!("Failed to read file '{}': {}", path, e)))
}

fn list_files_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "Optional relative directory path to list files from. Defaults to current directory ('.') if not provided.".to_string(),
        },
    );

    ToolDefinition {
        name: "list_files".to_string(),
        description: "List files and directories recursively starting from a given path. If the path is a file, lists only that file. If no path is provided, lists files in the current directory.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(Vec::new()),
        },
        function: list_files_function,
    }
}

fn list_files_function(input: Value) -> Result<String> {
    let start_path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let start_path = Path::new(start_path_str);

    let mut files = Vec::new();

    visit_dirs_recursive(start_path, start_path, &mut files)?;

    serde_json::to_string(&files)
        .map_err(|e| AppError(format!("Failed to serialize file list: {}", e)))
}

// Helper function adapted from md.rs to check if a path should be skipped by tools
fn should_skip_tool_path(path: &Path) -> bool {
    const SKIP_DIRS: &[&str] = &[
        "target",
        "build",
        "dist",
        "node_modules",
        ".venv",
        "venv",
        "__pycache__",
        ".pytest_cache",
        ".idea",
        ".vscode",
        ".next",
        ".nuxt",
        ".docusaurus",
        ".cargo",
        ".rustup",
        // Add other common build/dependency directories if needed
    ];

    // Define specific dotfiles/dotdirs to skip
    const SKIP_DOTFILES: &[&str] = &[
        ".git",
        ".env",
        ".DS_Store", // Example common dotfile
                     // Add other specific dotfiles/dotdirs to skip
    ];

    path.components().any(|component| {
        if let Some(name) = component.as_os_str().to_str() {
            // Skip if it's a specific dotfile/dotdir OR in the general skip list
            SKIP_DOTFILES.contains(&name) || SKIP_DIRS.contains(&name)
        } else {
            false // Ignore non-UTF8 components if any
        }
    })
}

fn visit_dirs_recursive(
    current_path: &Path,
    base_path: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    if !current_path.exists() {
        return Err(AppError(format!(
            "Path does not exist: {}",
            current_path.display()
        )));
    }

    let display_path = current_path
        .strip_prefix(base_path.parent().unwrap_or(base_path))
        .unwrap_or(current_path);

    if current_path.is_dir() {
        if current_path != base_path && !should_skip_tool_path(current_path) {
            files.push(format!("{}/", display_path.to_string_lossy()));
        }

        match fs::read_dir(current_path) {
            Ok(entries) => {
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let path = entry.path();
                            if !should_skip_tool_path(&path) {
                                visit_dirs_recursive(&path, base_path, files)?;
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to read entry in '{}': {}. Skipping.",
                                current_path.display(),
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                return Err(AppError(format!(
                    "Failed to read directory '{}': {}",
                    current_path.display(),
                    e
                )));
            }
        }
    } else if current_path.is_file() {
        if !should_skip_tool_path(current_path) {
            files.push(display_path.to_string_lossy().to_string());
        }
    } else {
        eprintln!(
            "Warning: Skipping non-directory/non-file path: {}",
            current_path.display()
        );
    }
    Ok(())
}

fn edit_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path to the file to write or create.".to_string(),
        },
    );
    properties.insert(
        "content".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The full new content for the file. If the file exists, its entire content will be replaced. If it doesn't exist, it will be created with this content.".to_string(),
        },
    );
    let required = vec!["path".to_string(), "content".to_string()];

    ToolDefinition {
        name: "edit_file".to_string(),
        description: "Writes or overwrites a file with the provided content. If the file path doesn't exist, it (and any necessary parent directories) will be created. If the file exists, its content will be completely replaced.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(required),
        },
        function: write_or_create_file_function,
    }
}

fn write_or_create_file_function(input: Value) -> Result<String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError("Missing or empty required 'path' parameter for edit_file".to_string())
        })?;

    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError("Missing required 'content' parameter for edit_file".to_string())
        })?;

    let path = Path::new(path_str);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    AppError(format!(
                        "Failed to create directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            } else if !parent.is_dir() {
                return Err(AppError(format!(
                    "Cannot create directory because path '{}' exists and is not a directory.",
                    parent.display()
                )));
            }
        }
    }

    fs::write(path, content).map_err(|e| {
        AppError(format!(
            "Failed to write to file '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(format!("Successfully wrote content to {}", path_str))
}

#[cfg(test)]
mod tests {
    use super::*; // Import items from the outer module
    use std::path::Path;

    #[test]
    fn test_should_skip_tool_path_hidden() {
        assert!(should_skip_tool_path(Path::new(".git")));
        assert!(should_skip_tool_path(Path::new(".env")));
        assert!(should_skip_tool_path(Path::new("src/.hidden_file")));
        assert!(should_skip_tool_path(Path::new(".config/settings.toml")));
    }

    #[test]
    fn test_should_skip_tool_path_build_dirs() {
        assert!(should_skip_tool_path(Path::new("target")));
        assert!(should_skip_tool_path(Path::new("node_modules")));
        assert!(should_skip_tool_path(Path::new("project/target/debug")));
        assert!(should_skip_tool_path(Path::new("app/node_modules/package")));
        assert!(should_skip_tool_path(Path::new("venv/lib/python")));
    }

    #[test]
    fn test_should_skip_tool_path_valid() {
        assert!(!should_skip_tool_path(Path::new("src/main.rs")));
        assert!(!should_skip_tool_path(Path::new("README.md")));
        assert!(!should_skip_tool_path(Path::new("scripts/build.sh")));
        assert!(!should_skip_tool_path(Path::new("docs/api.html")));
        assert!(!should_skip_tool_path(Path::new(
            ".github/workflows/ci.yml"
        ))); // .github is allowed
    }

    // TODO: Add tests for list_files_function using temp directories
    // TODO: Add tests for read_file_function using temp files
    // TODO: Add tests for write_or_create_file_function using temp files
    // TODO: Add tests for Agent::execute_tool
}
