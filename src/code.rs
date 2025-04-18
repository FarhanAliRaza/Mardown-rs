use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
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

// Make the type alias crate-public so tests can access it
pub(crate) type Result<T> = std::result::Result<T, AppError>;

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
    system_prompt: String,
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

        // Load system prompt by embedding it at compile time
        let system_prompt: String = include_str!("system_prompt.txt").to_string();

        Ok(Agent {
            model,
            tools: vec![
                read_file_definition(),
                list_files_definition(),
                replace_block_verified_definition(),
                create_file_definition(),
                delete_file_definition(),
            ],
            system_prompt,
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
            .run_inference(
                conversation,
                api_tools.as_deref(),
                Some(&self.system_prompt),
            )
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

pub(crate) fn read_file_function(input: Value) -> Result<String> {
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

pub(crate) fn list_files_function(input: Value) -> Result<String> {
    let start_path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let start_path = Path::new(start_path_str);

    if !start_path.exists() {
        // Return a user-friendly message instead of an error
        return Ok(serde_json::to_string(&format!(
            "No such folder or file found: {}",
            start_path.display()
        ))
        .unwrap_or_else(|_| format!("No such folder or file found: {}", start_path.display())));
    }

    let mut files = Vec::new();

    visit_dirs_recursive(start_path, start_path, &mut files)?;

    serde_json::to_string(&files)
        .map_err(|e| AppError(format!("Failed to serialize file list: {}", e)))
}

// Helper function adapted from md.rs to check if a path should be skipped by tools
pub(crate) fn should_skip_tool_path(path: &Path) -> bool {
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
    ];
    const SKIP_DOTFILES: &[&str] = &[".git", ".env", ".DS_Store"];
    const ALLOW_DOTDIRS: &[&str] = &[".github"];

    path.components().any(|component| {
        // Ignore the current directory component (".") during checks
        if component.as_os_str() == "." {
            return false;
        }

        if let Some(name) = component.as_os_str().to_str() {
            let is_specifically_skipped =
                SKIP_DOTFILES.contains(&name) || SKIP_DIRS.contains(&name);
            let is_generally_hidden_and_not_allowed =
                name.starts_with('.') && !ALLOW_DOTDIRS.contains(&name);

            is_specifically_skipped || is_generally_hidden_and_not_allowed
        } else {
            false // Ignore non-UTF8 components
        }
    })
}

pub(crate) fn visit_dirs_recursive(
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

fn replace_block_verified_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path to the file to modify.".to_string(),
        },
    );
    properties.insert(
        "start_marker".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "A unique string from the original file content that immediately precedes the block to be replaced.".to_string(),
        },
    );
    properties.insert(
        "end_marker".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "A unique string from the original file content that immediately follows the block to be replaced.".to_string(),
        },
    );
    properties.insert(
        "pre_context".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "A short snippet (e.g., 1-2 lines) of the expected original file content immediately preceding the start_marker for verification.".to_string(),
        },
    );
    properties.insert(
        "post_context".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "A short snippet (e.g., 1-2 lines) of the expected original file content immediately following the end_marker for verification.".to_string(),
        },
    );
    properties.insert(
        "new_content".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The full new content for the code block.".to_string(),
        },
    );
    let required = vec![
        "path".to_string(),
        "start_marker".to_string(),
        "end_marker".to_string(),
        "pre_context".to_string(),
        "post_context".to_string(),
        "new_content".to_string(),
    ];

    ToolDefinition {
        name: "replace_block_verified".to_string(),
        description: "Replaces a block of code identified by unique start/end markers, verifying surrounding context fuzzily before applying. Use this for robust code modification.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(required),
        },
        function: replace_block_verified_function,
    }
}

pub(crate) fn replace_block_verified_function(input: Value) -> Result<String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError("Missing or empty required 'path' parameter".to_string()))?;

    let start_marker = input
        .get("start_marker")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'start_marker' parameter".to_string()))?;

    let end_marker = input
        .get("end_marker")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'end_marker' parameter".to_string()))?;

    let pre_context = input
        .get("pre_context")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'pre_context' parameter".to_string()))?;

    let post_context = input
        .get("post_context")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'post_context' parameter".to_string()))?;

    let new_content = input
        .get("new_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError("Missing required 'new_content' parameter".to_string()))?;

    if start_marker.is_empty() || end_marker.is_empty() {
        return Err(AppError(
            "Start and end markers cannot be empty.".to_string(),
        ));
    }

    let path = Path::new(path_str);

    // Parent directory check
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
                    "Path '{}' exists and is not a directory.",
                    parent.display()
                )));
            }
        }
    }

    // Read the original file content
    let original_content = fs::read_to_string(path)
        .map_err(|e| AppError(format!("Failed to read file '{}': {}", path.display(), e)))?;

    // --- Step 1: Find Exact Markers ---
    let start_match: Vec<_> = original_content.match_indices(start_marker).collect();
    if start_match.is_empty() {
        return Err(AppError(format!(
            "Start marker not found: {:?}",
            start_marker
        )));
    }
    if start_match.len() > 1 {
        return Err(AppError(format!(
            "Start marker not unique ({} matches): {:?}",
            start_match.len(),
            start_marker
        )));
    }
    let marker_start_byte_index = start_match[0].0;
    let content_start_byte_index = marker_start_byte_index + start_marker.len(); // Index *after* the start marker

    let search_area = &original_content[content_start_byte_index..];
    // Find ALL matches for the end marker in the search area
    let end_matches: Vec<_> = search_area.match_indices(end_marker).collect();
    if end_matches.is_empty() {
        return Err(AppError(format!(
            "End marker not found after start marker: {:?}",
            end_marker
        )));
    }
    // Use the LAST match as the definitive end marker position
    let last_end_match = end_matches.last().unwrap(); // Safe because we checked is_empty()
    let content_end_byte_index = content_start_byte_index + last_end_match.0; // Index *before* the last end marker
    let marker_end_byte_index = content_end_byte_index + end_marker.len(); // Index *after* the last end marker

    if content_start_byte_index > content_end_byte_index {
        // This check might be necessary if the end marker could somehow precede the start marker
        // despite the search area starting after the start marker. Or if markers overlap?
        // Keeping it for safety, though it might be redundant now.
        return Err(AppError(format!("Start marker appears after end marker.")));
    }

    // --- Step 2: Verify Context Fuzzily ---
    // Extract the relevant slices from the original content
    let actual_content_before_marker = &original_content[..marker_start_byte_index];
    let actual_content_after_marker = &original_content[marker_end_byte_index..];

    // Fuzzy check if the content *before* the start marker *ends with* the pre_context
    if !fuzzy_ends_with(actual_content_before_marker, pre_context) {
        return Err(AppError(format!(
            "Pre-marker context mismatch. Text before marker {:?} did not fuzzily end with expected {:?}",
            start_marker, pre_context
        )));
    }

    // Fuzzy check if the content *after* the end marker *starts with* the post_context
    if !fuzzy_starts_with(actual_content_after_marker, post_context) {
        return Err(AppError(format!(
            "Post-marker context mismatch. Text after marker {:?} did not fuzzily start with expected {:?}",
            end_marker, post_context
        )));
    }

    // --- Step 3: Construct and Write ---
    let mut result = String::with_capacity(
        original_content.len() - (content_end_byte_index - content_start_byte_index)
            + new_content.len(),
    );
    result.push_str(&original_content[..content_start_byte_index]);
    result.push_str(new_content);
    result.push_str(&original_content[content_end_byte_index..]);

    fs::write(path, result).map_err(|e| {
        AppError(format!(
            "Failed to write verified replaced content to file '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(format!(
        "Successfully replaced block in {} after context verification",
        path_str
    ))
}

// Helper function for fuzzy context matching (check ends_with)
pub(crate) fn fuzzy_ends_with(actual: &str, expected_suffix: &str) -> bool {
    if actual.ends_with(expected_suffix) {
        return true; // Exact match
    }
    // Fuzzy: Trim both actual and expected ends
    let trimmed_actual = actual.trim_end();
    let trimmed_expected = expected_suffix.trim_end();
    if trimmed_actual.ends_with(trimmed_expected) {
        return true;
    }
    // Fuzzy: Trim both completely (handles leading/trailing on both)
    let fully_trimmed_actual = actual.trim();
    let fully_trimmed_expected = expected_suffix.trim();
    if !fully_trimmed_expected.is_empty() && fully_trimmed_actual.ends_with(fully_trimmed_expected)
    {
        return true;
    }
    false
}

// Helper function for fuzzy context matching (check starts_with)
pub(crate) fn fuzzy_starts_with(actual: &str, expected_prefix: &str) -> bool {
    if actual.starts_with(expected_prefix) {
        return true; // Exact match
    }
    // Fuzzy: Trim both actual and expected starts
    let trimmed_actual = actual.trim_start();
    let trimmed_expected = expected_prefix.trim_start();
    if trimmed_actual.starts_with(trimmed_expected) {
        return true;
    }
    // Fuzzy: Trim both completely (handles leading/trailing on both)
    let fully_trimmed_actual = actual.trim();
    let fully_trimmed_expected = expected_prefix.trim();
    if !fully_trimmed_expected.is_empty()
        && fully_trimmed_actual.starts_with(fully_trimmed_expected)
    {
        return true;
    }
    false
}

// --- Create File Tool ---

fn create_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path of the file to create.".to_string(),
        },
    );
    properties.insert(
        "content".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The initial content for the new file.".to_string(),
        },
    );
    let required = vec!["path".to_string(), "content".to_string()];

    ToolDefinition {
        name: "create_file".to_string(),
        description: "Creates a new file with the provided content. IMPORTANT: This tool fails if the file already exists.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(required),
        },
        function: create_file_function,
    }
}

pub(crate) fn create_file_function(input: Value) -> Result<String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError("Missing or empty required 'path' parameter for create_file".to_string())
        })?;

    let content = input
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError("Missing required 'content' parameter for create_file".to_string())
        })?;

    let path = Path::new(path_str);

    // Check if file already exists
    if path.exists() {
        return Err(AppError(format!(
            "Cannot create file because path '{}' already exists.",
            path.display()
        )));
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                AppError(format!(
                    "Failed to create directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
            if parent.is_file() {
                return Err(AppError(format!(
                    "Cannot create directory because parent path '{}' exists and is a file.",
                    parent.display()
                )));
            }
        }
    }

    // Write the new file content
    fs::write(path, content).map_err(|e| {
        AppError(format!(
            "Failed to create file '{}' (Error: {}). Does parent directory exist?",
            path.display(),
            e
        ))
    })?;

    Ok(format!("Successfully created file {}", path_str))
}

// --- Delete File Tool ---

fn delete_file_definition() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "path".to_string(),
        ToolSchemaProperty {
            property_type: "string".to_string(),
            description: "The relative path of the file to delete.".to_string(),
        },
    );
    let required = vec!["path".to_string()];

    ToolDefinition {
        name: "delete_file".to_string(),
        description: "Deletes the specified file. Fails if the path is a directory or the file does not exist.".to_string(),
        schema: ToolSchema {
            schema_type: "object".to_string(),
            properties,
            required: Some(required),
        },
        function: delete_file_function,
    }
}

pub(crate) fn delete_file_function(input: Value) -> Result<String> {
    let path_str = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError("Missing or empty required 'path' parameter for delete_file".to_string())
        })?;

    let path = Path::new(path_str);

    // Check if path exists
    if !path.exists() {
        return Err(AppError(format!(
            "Cannot delete because path '{}' does not exist.",
            path.display()
        )));
    }

    // Check if it's a file (not a directory)
    if !path.is_file() {
        return Err(AppError(format!(
            "Cannot delete because path '{}' is not a file (it might be a directory).",
            path.display()
        )));
    }

    // Delete the file
    fs::remove_file(path)
        .map_err(|e| AppError(format!("Failed to delete file '{}': {}", path.display(), e)))?;

    Ok(format!("Successfully deleted file {}", path_str))
}
