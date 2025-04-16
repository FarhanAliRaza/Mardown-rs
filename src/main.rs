use anyhow::{Context, Result};
use clap::Parser;
use std::env;

// Import modules
mod code;
mod md;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory to scan for files or 'code' to run the code agent
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Check if the first argument is "code"
    if args.input_dir == "code" {
        // Run the code agent
        println!("Starting code agent...");
        code::main().await?;
    } else {
        // Run the markdown generator
        println!("Starting markdown generator...");
        let md_args = md::MdrsArgs {
            input_dir: args.input_dir,
            output: args.output,
            extensions: args.extensions,
            ignore: args.ignore,
        };
        md::generate_markdown(md_args)?;
    }

    Ok(())
}
