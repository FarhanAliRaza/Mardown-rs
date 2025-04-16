# 🚀 pai

**pai** is a fast and efficient command-line tool that recursively scans directories and creates a single markdown file containing the contents of all files. It also includes functionality for running a code generation agent using different large language models.

## ✨ Features

- **📝 Markdown Generation**: Generate a markdown file from code files.
  - Fast recursive directory scanning
  - Filter by specific extensions
  - Customizable output file location
  - Automatically skip binary files
  - Ignore specific files or patterns
- **🤖 Code Generation Agent**: Run a code generation agent using various large language models such as Claude, Google, DeepSeek, and OpenAI.
  - Read and edit files with natural language instructions
  - List directory contents
  - Create new files from scratch
  - Interactive chat interface

## 📥 Installation

### Prerequisites

- Rust toolchain (1.70.0 or later recommended)
- Cargo (comes with Rust)
- API keys for the language models you intend to use

### Installing from Source

```sh
# Clone the repository
git clone git@github.com:FarhanAliRaza/pai.git
cd pai

# Build the project
cargo build --release

# The executable will be in target/release/pai
```

### Installing via Cargo

```sh
# From the project directory
cargo install --path .

# Or directly from the repository (if published)
cargo install pai
```

## 🔑 Environment Setup

Set up the API keys for the language models you want to use:

```sh
# For Claude
export ANTHROPIC_API_KEY=your_api_key_here

# For OpenAI
export OPENAI_API_KEY=your_api_key_here

# For Google
export GOOGLE_API_KEY=your_api_key_here

# For DeepSeek
export DEEPSEEK_API_KEY=your_api_key_here
```

## 📚 Usage

### 📝 Markdown Generation

Generate a markdown file from code files:

```sh
# Basic usage with default settings (uses current directory and outputs to llm.md)
pai md

# Specify input directory and output file
pai md --input-dir /path/to/project --output documentation.md

# Filter by specific file extensions
pai md --extensions rs,toml,md

# Ignore specific files or patterns
pai md --ignore "target,.git,Cargo.lock"
```

#### Options

| Option | Description | Default |
|--------|-------------|---------|
| `--input-dir`, `-i` | Directory to scan | Current directory (.) |
| `--output`, `-o` | Output markdown file path | `llm.md` |
| `--extensions`, `-e` | Comma-separated file extensions to include | All files |
| `--ignore`, `-n` | Comma-separated files or patterns to ignore | None |

### 🤖 Code Generation Agent

Run the code generation agent:

```sh
# Run with default model (Claude)
pai code

# Specify a particular model
pai code --model claude
pai code --model openai
pai code --model google
pai code --model deepseek
```

#### Options

| Option | Description | Default |
|--------|-------------|---------|
| `--model`, `-m` | LLM model to use (claude, openai, google, deepseek) | `claude` |

#### 🧰 Code Agent Tools

The code agent provides the following tools:

1. **📄 read_file**: Reads the contents of a file
2. **📂 list_files**: Lists files in a directory (recursively)
3. **✏️ edit_file**: Creates or modifies files with specified content

#### 💬 Example Interactions

Once the agent is running, you can interact with it using natural language:

- "What files are in the src directory?"
- "Show me the content of main.rs"
- "Create a new file called utils.rs with a function to parse JSON"
- "Add error handling to the API request in network.rs"
- "Analyze and refactor this code to improve performance"


## 👨‍💻 Development

### 🛠️ Building

```sh
# Development build
cargo build

# Release build
cargo build --release
```

### 🧪 Testing

```sh
# Run all tests
cargo test

# Run specific tests
cargo test markdown_generator
```

## 📜 License

This project is licensed under the MIT License. See the `LICENSE` file for details.

## ✍️ Author

Farhan Ali Raza <farhanalirazaazeemi@gmail.com>

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
