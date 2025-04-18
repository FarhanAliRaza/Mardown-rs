#[cfg(test)]
mod tests {
    // Import specific items needed using crate-relative paths
    use crate::code::{
        Result, // Import the Result type alias from crate::code
        create_file_function,
        delete_file_function,
        fuzzy_ends_with,
        fuzzy_starts_with,
        list_files_function,
        replace_block_verified_function,
        should_skip_tool_path,
    };
    use crate::models::AppError; // Import AppError specifically from its correct module
    use serde_json::Value;
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir; // Ensure HashSet is imported here

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

    // Test the fuzzy helpers with tolerant logic
    #[test]
    fn test_fuzzy_ends_with() {
        assert!(fuzzy_ends_with("abc", "abc")); // Exact
        assert!(fuzzy_ends_with("x abc", "abc")); // Exact suffix
        assert!(fuzzy_ends_with("x abc ", "abc")); // Trim actual end
        assert!(fuzzy_ends_with("x abc", "abc ")); // Trim expected end
        assert!(fuzzy_ends_with("x  abc  ", " abc ")); // Trim both
        assert!(!fuzzy_ends_with("x abc", "def")); // Different suffix
        assert!(!fuzzy_ends_with("x abc", "abcd")); // Suffix mismatch
        // This case should now be TRUE with tolerant (trim both) logic
        assert!(fuzzy_ends_with("xabc", " abc")); // Trim both makes this match
    }

    #[test]
    fn test_fuzzy_starts_with() {
        assert!(fuzzy_starts_with("abc", "abc")); // Exact
        assert!(fuzzy_starts_with("abc x", "abc")); // Exact prefix
        assert!(fuzzy_starts_with(" abc x", "abc")); // Trim actual start
        assert!(fuzzy_starts_with("abc x", " abc")); // Trim expected start
        assert!(fuzzy_starts_with("  abc  x", " abc ")); // Trim both
        assert!(!fuzzy_starts_with("abc x", "def")); // Different prefix
        assert!(!fuzzy_starts_with("abc x", "abcd")); // Prefix mismatch
        // This case should now be TRUE with tolerant (trim both) logic
        assert!(fuzzy_starts_with("abcx", "abc ")); // Trim both makes this match
    }

    // --- Tests for replace_block_verified_function ---

    // Helper to create test file and setup common JSON input
    fn setup_verified_test(
        initial_content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf, Value) {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        fs::write(&file_path, initial_content).unwrap();

        let input_json = serde_json::json!({
            "path": file_path.to_str().unwrap(),
            "start_marker": "// START BLOCK\n",
            "end_marker": "\n    // END BLOCK",
            "pre_context": "println!(\"Hello\");\n", // Expected context before start marker
            "post_context": "\n\n    println!(\"World\");", // Expected context after end marker
            "new_content": "    let y = 10;\n    println!(\"New block: {}\", y);\n"
        });
        (dir, file_path, input_json)
    }

    #[test]
    fn test_replace_block_verified_success() -> Result<()> {
        let initial_content = r#"
println!("Hello");

    // START BLOCK
    let x = 5;
    println!("Old block: {}", x);
    // END BLOCK

    println!("World");
}
"#;
        let (_dir, file_path, input_json) = setup_verified_test(initial_content);

        replace_block_verified_function(input_json)?;

        let final_content = fs::read_to_string(&file_path)
            .map_err(|e| AppError(format!("Failed to read test file after replace: {}", e)))?;
        let expected_content = r#"
println!("Hello");

    // START BLOCK
    let y = 10;
    println!("New block: {}", y);

    // END BLOCK

    println!("World");
}
"#;
        assert_eq!(
            final_content.replace("\r\n", "\n"),
            expected_content.replace("\r\n", "\n")
        );
        Ok(())
    }

    #[test]
    fn test_replace_block_verified_success_fuzzy_context() -> Result<()> {
        let initial_content = r#"
println!("Hello");    

    // START BLOCK
    let x = 5;
    println!("Old block: {}", x);
    // END BLOCK   

    println!("World"); 
}
"#; // Added trailing spaces to context lines
        let (_dir, file_path, mut input_json) = setup_verified_test(initial_content);
        // Keep expected context *without* spaces in JSON, rely on fuzzy match

        replace_block_verified_function(input_json)?;

        let final_content = fs::read_to_string(&file_path).map_err(|e| {
            AppError(format!(
                "Failed to read test file after fuzzy replace: {}",
                e
            ))
        })?;
        // Expected output still doesn't have the extra spaces
        let expected_content = r#"
println!("Hello");    

    // START BLOCK
    let y = 10;
    println!("New block: {}", y);

    // END BLOCK   

    println!("World"); 
}
"#;
        // Note: The *expected* content for assertion should reflect the *new* content inserted into the original with spaces
        let expected_after_replace = r#"
println!("Hello");    

    // START BLOCK
    let y = 10;
    println!("New block: {}", y);

    // END BLOCK   

    println!("World"); 
}
"#;
        assert_eq!(
            final_content.replace("\r\n", "\n"),
            expected_after_replace.replace("\r\n", "\n")
        );
        Ok(())
    }

    #[test]
    fn test_replace_block_verified_pre_context_mismatch() {
        let initial_content = r#"
println!("DIFFERENT Hello");

    // START BLOCK
    let x = 5;
    // END BLOCK

    println!("World");
"#;
        let (_dir, _file_path, input_json) = setup_verified_test(initial_content);

        let result = replace_block_verified_function(input_json);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .0
                .contains("Pre-marker context mismatch")
        );
    }

    #[test]
    fn test_replace_block_verified_post_context_mismatch() {
        let initial_content = r#"
println!("Hello");

    // START BLOCK
    let x = 5;
    // END BLOCK

    println!("DIFFERENT World");
"#;
        let (_dir, _file_path, input_json) = setup_verified_test(initial_content);

        let result = replace_block_verified_function(input_json);
        assert!(result.is_err());
        // Check the specific error content
        let err_msg = result.unwrap_err().0;
        assert!(
            err_msg.contains("Post-marker context mismatch"),
            "Error message was: {}",
            err_msg
        );
        // Correctly escaped assertion strings:
        assert!(
            err_msg.contains("expected \"\\n\\n    println!(\\\"World\\\");\""),
            "Error message was: {}",
            err_msg
        );
        assert!(
            err_msg.contains("after marker \"\\n    // END BLOCK\""),
            "Error message was: {}",
            err_msg
        );
    }

    // Add tests for marker errors (not found, not unique) - similar to previous replace_block tests
    #[test]
    fn test_replace_block_verified_start_marker_not_found() {
        let (_dir, _file_path, mut input_json) = setup_verified_test("content");
        input_json["start_marker"] = serde_json::json!("NOT_REAL");
        let result = replace_block_verified_function(input_json);
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("Start marker not found"));
    }

    // TODO: Add more tests (end marker not found, markers not unique, etc.)

    // --- Tests for create_file_function ---
    #[test]
    fn test_create_file_success() -> Result<()> {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("new_test_file.txt");
        let path_str = file_path
            .strip_prefix(dir.path())
            .unwrap()
            .to_str()
            .unwrap(); // Use relative path
        let content = "Hello, world!";

        let input_json = serde_json::json!({
            "path": path_str,
            "content": content
        });

        // Run function relative to temp dir
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = create_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        result?; // Check for potential errors from create_file_function

        assert!(file_path.exists());
        assert!(file_path.is_file());
        let read_content = fs::read_to_string(&file_path)
            .map_err(|e| AppError(format!("Failed to read created test file: {}", e)))?; // Add error handling
        assert_eq!(read_content, content);

        Ok(())
    }

    #[test]
    fn test_create_file_already_exists() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("existing.txt");
        fs::write(&file_path, "initial").unwrap();
        let path_str = file_path
            .strip_prefix(dir.path())
            .unwrap()
            .to_str()
            .unwrap();

        let input_json = serde_json::json!({
            "path": path_str,
            "content": "new content"
        });

        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = create_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("already exists"));
    }

    #[test]
    fn test_create_file_in_new_subdir() -> Result<()> {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("new_subdir");
        let file_path = sub_dir.join("sub_file.txt");
        let path_str = file_path
            .strip_prefix(dir.path())
            .unwrap()
            .to_str()
            .unwrap();
        let content = "Subdir content";

        let input_json = serde_json::json!({
            "path": path_str,
            "content": content
        });

        // Run function relative to temp dir
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = create_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        result?; // Check for potential errors

        assert!(sub_dir.exists());
        assert!(sub_dir.is_dir());
        assert!(file_path.exists());
        assert!(file_path.is_file());
        let read_content = fs::read_to_string(&file_path)
            .map_err(|e| AppError(format!("Failed to read created subdir test file: {}", e)))?; // Corrected this line
        assert_eq!(read_content, content);

        Ok(())
    }

    // --- Tests for delete_file_function ---
    #[test]
    fn test_delete_file_success() -> Result<()> {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        fs::write(&file_path, "delete me").unwrap();
        let path_str = file_path
            .strip_prefix(dir.path())
            .unwrap()
            .to_str()
            .unwrap();

        assert!(file_path.exists());

        let input_json = serde_json::json!({
            "path": path_str
        });

        // Run function relative to temp dir
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = delete_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        result?; // Check for potential errors

        assert!(!file_path.exists());

        Ok(())
    }

    #[test]
    fn test_delete_file_does_not_exist() {
        let dir = tempdir().unwrap();
        let path_str = "non_existent_file.txt";

        let input_json = serde_json::json!({
            "path": path_str
        });

        // Run function relative to temp dir
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = delete_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("does not exist"));
    }

    #[test]
    fn test_delete_file_is_directory() {
        let dir = tempdir().unwrap();
        let sub_dir_path = dir.path().join("a_directory");
        fs::create_dir(&sub_dir_path).unwrap();
        let path_str = sub_dir_path
            .strip_prefix(dir.path())
            .unwrap()
            .to_str()
            .unwrap();

        let input_json = serde_json::json!({
            "path": path_str
        });

        // Run function relative to temp dir
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = delete_file_function(input_json);
        std::env::set_current_dir(current_dir).unwrap();

        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("is not a file"));
    }

    // TODO: Add tests for read_file_function using temp files
    // TODO: Add tests for Agent::execute_tool

    #[test]
    fn test_list_files_current_dir() -> Result<()> {
        let dir = tempdir().unwrap();
        let base_path = dir.path();

        // Create test files and directory structure
        fs::write(base_path.join("file1.txt"), "content1")
            .map_err(|e| AppError(format!("Test setup failed (write file1): {}", e)))?;
        fs::write(base_path.join("file2.rs"), "content2")
            .map_err(|e| AppError(format!("Test setup failed (write file2): {}", e)))?;
        fs::create_dir(base_path.join("subdir"))
            .map_err(|e| AppError(format!("Test setup failed (create subdir): {}", e)))?;
        fs::write(base_path.join("subdir/nested.txt"), "nested")
            .map_err(|e| AppError(format!("Test setup failed (write nested): {}", e)))?;
        fs::write(base_path.join(".hiddenfile"), "hidden")
            .map_err(|e| AppError(format!("Test setup failed (write hiddenfile): {}", e)))?;
        fs::create_dir(base_path.join(".hiddendir"))
            .map_err(|e| AppError(format!("Test setup failed (create hiddendir): {}", e)))?;

        let input_json = serde_json::json!({}); // Test default path (.)

        // Run function with CWD set to the temp dir
        let current_dir =
            std::env::current_dir().map_err(|e| AppError(format!("Failed to get CWD: {}", e)))?;
        std::env::set_current_dir(base_path)
            .map_err(|e| AppError(format!("Failed to set CWD to temp dir: {}", e)))?;
        let result_json_str = list_files_function(input_json)?;
        std::env::set_current_dir(&current_dir) // Restore CWD - pass reference
            .map_err(|e| AppError(format!("Failed to restore CWD: {}", e)))?;

        // Parse the JSON result
        let result_list: Vec<String> = serde_json::from_str(&result_json_str)
            .map_err(|e| AppError(format!("Failed to parse list_files output: {}", e)))?;

        // Use HashSet for order-independent comparison
        let result_set: HashSet<_> = result_list.into_iter().collect();

        // Adjusted expected set to include the './' prefix
        let expected_set: HashSet<String> = [
            "./file1.txt".to_string(),
            "./file2.rs".to_string(),
            "./subdir/".to_string(),
            "./subdir/nested.txt".to_string(),
        ]
        .into_iter()
        .collect();

        assert_eq!(result_set, expected_set);

        Ok(())
    }
}
