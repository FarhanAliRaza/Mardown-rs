# 📝 MDRS (Markdown Rust)

A versatile command-line tool with dual functionality:
1. Recursively scan directories and create a single markdown file containing the contents of all files
2. Act as a code-editing agent powered by Claude AI to help you manage and edit your codebase

## ✨ Features

### Markdown Generator
- 🚀 Fast recursive directory scanning
- 📁 Process all files or filter by specific extensions
- 📄 Customizable output markdown file location
- 🔍 Relative path preservation in output
- 🎯 Skip output file to prevent recursion
- 🚫 Skip binary files automatically
- 🛡️ Ignore specific files or extensions
- 📦 Skip common build and dependency directories

### Code Agent
- 🤖 Powered by Claude AI through the Anthropic API
- 📝 Read and edit files with natural language instructions
- 📂 List directory contents
- 🔄 Create new files from scratch
- 💬 Chat interface for interactive assistance

## 🛠️ Installation

### Prerequisites
- Rust toolchain (1.70.0 or later)
- Cargo (comes with Rust)
- Anthropic API key (for code agent functionality)

### Building from Source

```bash
# Clone the repository
git clone git@github.com:FarhanAliRaza/Mardown-rs.git
cd Mardown-rs

# Build the project
cargo build --release

# The executable will be in target/release/mdrs
```

### Installing via Cargo

```bash
cargo install --path . 
```

## 🚀 Usage

### Markdown Generator

#### Basic Usage
Process all files in the current directory:
```bash
mdrs
```

#### Specify Input Directory
Process files in a specific directory:
```bash
mdrs /path/to/directory
```

#### Custom Output File
Specify a custom output markdown file:
```bash
mdrs -o output.md
```

#### Filter by Extensions
Process only files with specific extensions:
```bash
mdrs -e py,js,ts
```

#### Ignore Files or Extensions
Skip specific files or extensions:
```bash
mdrs -i "Cargo.lock,.gitignore,.env,.lock"
```

#### Combine Options
Process specific directory, with custom output, extensions, and ignore patterns:
```bash
mdrs /path/to/directory -o output.md -e py,js -i ".lock,.env"
```

### Code Agent

#### Setup
First, set your Anthropic API key as an environment variable:
```bash
export ANTHROPIC_API_KEY=your_api_key_here
```

#### Start the Code Agent
Launch the interactive code agent:
```bash
mdrs code
```

#### Examples of Code Agent Use
Once the agent is running, you can interact with it through natural language:

- **Read a file**: "What's in src/main.rs?"
- **List files**: "What files are in the src directory?"
- **Edit a file**: "Update main.rs to add better error handling"
- **Create a new file**: "Create a new file called utils.rs with a function to parse JSON"

## 📋 Command Line Arguments

| Argument | Short | Long | Description | Default |
|----------|-------|------|-------------|---------|
| `input_dir` | - | - | Directory to scan or 'code' to run the code agent | Current directory (.) |
| `output` | `-o` | `--output` | Output markdown file path (for markdown generator) | llm.md |
| `extensions` | `-e` | `--extensions` | Comma-separated file extensions to include | None (all files) |
| `ignore` | `-i` | `--ignore` | Comma-separated files or extensions to ignore | None |

## 🔍 File Filtering (Markdown Generator)

The tool automatically:
- Skips binary files (images, executables, etc.)
- Skips hidden files and directories (starting with `.`)
- Skips common build and dependency directories:
  - `target`, `build`, `dist`
  - `node_modules`, `.git`, `.venv`
  - `__pycache__`, `.pytest_cache`
  - `.idea`, `.vscode`
  - `.next`, `.nuxt`, `.docusaurus`
  - `.cargo`, `.rustup`

## 📄 Markdown Output Format

The generated markdown file will have the following format for each file:

```markdown
filename.py
```
[file content]
```

foldername/new.py
```
[file content]
```
```

## 🤖 Code Agent Capabilities

The code agent provides three main tools:

1. **read_file**: Reads the contents of a file
2. **list_files**: Lists all files in a directory
3. **edit_file**: Makes edits to a file or creates new files

The agent uses Claude AI to understand your instructions in natural language and perform the appropriate actions on your codebase.

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🙏 Acknowledgments

- [walkdir](https://github.com/BurntSushi/walkdir) for efficient directory traversal
- [clap](https://github.com/clap-rs/clap) for command-line argument parsing
- [anyhow](https://github.com/dtolnay/anyhow) for error handling
- [Anthropic](https://www.anthropic.com/) for Claude AI capabilities
- [reqwest](https://github.com/seanmonstar/reqwest) for HTTP requests
- [tokio](https://github.com/tokio-rs/tokio) for async runtime
- [serde](https://github.com/serde-rs/serde) for serialization/deserialization 
