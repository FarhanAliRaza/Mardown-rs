use anyhow::{Context, Result};
use clap::Parser;
use std::fs::{self, File};
use std::io::Write;
use walkdir::{DirEntry, WalkDir};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory to scan for files
    #[arg(default_value = ".")]
    input_dir: String,

    /// Output markdown file path
    #[arg(short, long, default_value = "llm.md")]
    output: String,

    /// File extensions to include (comma-separated). If not specified, includes all files.
    #[arg(short, long)]
    extensions: Option<String>,

    /// Files or extensions to ignore (comma-separated). Can be full filenames or extensions (e.g., "Cargo.lock,.gitignore,.env")
    #[arg(short, long)]
    ignore: Option<String>,
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with("."))
        .unwrap_or(false)
}

fn is_binary_file(path: &str) -> Result<bool> {
    let content = fs::read(path)?;

    // Check for null bytes
    if content.contains(&0) {
        return Ok(true);
    }

    // Check for high percentage of non-printable characters
    let non_printable_count = content
        .iter()
        .filter(|&&c| c < 32 && c != 9 && c != 10 && c != 13) // 9=tab, 10=LF, 13=CR
        .count();

    let total_bytes = content.len();
    let non_printable_ratio = non_printable_count as f64 / total_bytes as f64;

    // If more than 10% of bytes are non-printable, consider it binary
    Ok(non_printable_ratio > 0.1)
}

fn should_skip_path(path: &str, ignore_patterns: &[String]) -> bool {
    let components: Vec<&str> = path.split('/').collect();
    println!("Checking path: {}", path);

    // Skip the first component if it's "." or ".."
    let components_to_check = if components[0] == "." || components[0] == ".." {
        &components[1..]
    } else {
        &components[..]
    };

    // Check if the full filename matches any ignore pattern
    if let Some(filename) = components_to_check.last() {
        if ignore_patterns.iter().any(|pattern| filename == pattern) {
            println!("Skipping ignored file: {}", filename);
            return true;
        }
    }

    for component in components_to_check {
        if component.starts_with(".") {
            println!("Skipping hidden component: {}", component);
            return true;
        }

        if [
            "target",
            "build",
            "dist",
            "node_modules",
            ".git",
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
        ]
        .contains(&component)
        {
            println!("Skipping build directory: {}", component);
            return true;
        }
    }

    false
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse extensions if provided
    let extensions: Vec<String> = args
        .extensions
        .as_deref()
        .map(|exts| exts.split(',').map(String::from).collect())
        .unwrap_or_default();

    // Parse ignore patterns if provided
    let ignore_patterns: Vec<String> = args
        .ignore
        .as_deref()
        .map(|patterns| patterns.split(',').map(String::from).collect())
        .unwrap_or_default();

    // Create output file
    let mut output_file = File::create(&args.output)
        .with_context(|| format!("Failed to create output file: {}", args.output))?;

    let mut file_count = 0;
    // Walk through all files in the directory
    for entry in WalkDir::new(&args.input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy();

        println!("Processing file: {}", path_str);

        // Skip the output file itself
        if path_str == args.output {
            println!("Skipping output file");
            continue;
        }

        // Check if path should be skipped
        if should_skip_path(&path_str, &ignore_patterns) {
            println!("Skipping file due to path rules");
            continue;
        }

        // Check if file is binary
        if let Ok(true) = is_binary_file(&path_str) {
            println!("Skipping binary file: {}", path_str);
            continue;
        }

        // Check extension if specified
        if !extensions.is_empty() {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_string();
                if !extensions.contains(&ext_str) {
                    println!("Skipping file due to extension: {}", ext_str);
                    continue;
                }
            }
        }

        // Check if file extension is in ignore patterns
        if let Some(ext) = path.extension() {
            let ext_str = format!(".{}", ext.to_string_lossy());
            if ignore_patterns.contains(&ext_str) {
                println!("Skipping file with ignored extension: {}", ext_str);
                continue;
            }
        }

        // Get relative path
        let relative_path = path
            .strip_prefix(&args.input_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Read file content
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        // Write to markdown file
        writeln!(output_file, "{}", relative_path)?;
        writeln!(output_file, "```")?;
        writeln!(output_file, "{}", content)?;
        writeln!(output_file, "```\n")?;

        file_count += 1;
        println!("Successfully processed file: {}", relative_path);
    }

    println!("Successfully created markdown file at: {}", args.output);
    println!("Total files processed: {}", file_count);
    Ok(())
}
