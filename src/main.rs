pub mod code;
pub mod md;
pub mod models;

use clap::{Parser, Subcommand};
use code::Agent;
use md::{MdrsArgs, generate_markdown};
use models::{AppError, ModelType};
use std::process;

// Define Result type alias specific to main, or import if generally needed
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError(format!("{:?}", err))
    }
}

type Result<T> = std::result::Result<T, AppError>;

// Top-level CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

// Define the subcommands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the code generation agent
    Code(CodeArgs),
    /// Generate a Markdown file from code files
    Md(MdrsArgs),
}

// Arguments for the `code` subcommand
#[derive(Parser, Debug)]
struct CodeArgs {
    /// The large language model to use.
    #[arg(short, long, value_parser = clap::value_parser!(String), default_value = "claude")]
    model: String,
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Code(args) => {
            let model_type = match args.model.to_lowercase().as_str() {
                "google" => ModelType::Google,
                "claude" => ModelType::Claude,
                "deepseek" => ModelType::DeepSeek,
                "openai" => ModelType::OpenAI,
                _ => {
                    eprintln!(
                        "\x1b[91mError: Invalid model '{}'. Choose 'claude', 'google', 'deepseek', or 'openai'.\x1b[0m",
                        args.model
                    );
                    process::exit(1);
                }
            };

            // Use the public Agent::new function
            match Agent::new(model_type) {
                Ok(agent) => agent.run().await?,
                Err(err) => {
                    eprintln!("\x1b[91mError: Failed to initialize agent: {}\x1b[0m", err);
                    process::exit(1);
                }
            }
        }
        Commands::Md(args) => {
            println!(
                "Generating Markdown from '{}' to '{}'...",
                args.input_dir, args.output
            );
            generate_markdown(args)?;
            println!("Markdown generation complete.");
        }
    }

    Ok(())
}
