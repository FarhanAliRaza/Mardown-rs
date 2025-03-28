use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use clap::Parser;
use anyhow::{Result, Context};

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
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Parse extensions if provided
    let extensions: Vec<String> = args
        .extensions
        .as_deref()
        .map(|exts| exts.split(',').map(String::from).collect())
        .unwrap_or_default();

    // Create output file
    let mut output_file = File::create(&args.output)
        .with_context(|| format!("Failed to create output file: {}", args.output))?;

    // Walk through all files in the directory
    for entry in WalkDir::new(&args.input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        
        // Skip the output file itself
        if path.to_string_lossy() == args.output {
            continue;
        }

        // Check extension if specified
        if !extensions.is_empty() {
            if let Some(ext) = path.extension() {
                if !extensions.contains(&ext.to_string_lossy().to_string()) {
                    continue;
                }
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
    }

    println!("Successfully created markdown file at: {}", args.output);
    Ok(())
}
