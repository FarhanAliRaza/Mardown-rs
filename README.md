# 📝 MDRS (Markdown Recursive Scanner)

A fast and efficient command-line tool that recursively scans directories and creates a single markdown file containing the contents of all files. Perfect for adding folders to LLM context

## ✨ Features

- 🚀 Fast recursive directory scanning
- 📁 Process all files or filter by specific extensions
- 📄 Customizable output markdown file location
- 🔍 Relative path preservation in output
- 💪 Written in Rust for maximum performance

## 🛠️ Installation

### Prerequisites
- Rust toolchain (1.70.0 or later)
- Cargo (comes with Rust)

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

### Basic Usage
Process all files in the current directory:
```bash
mdrs
```

### Specify Input Directory
Process files in a specific directory:
```bash
mdrs /path/to/directory
```

### Custom Output File
Specify a custom output markdown file:
```bash
mdrs -o output.md
```

### Filter by Extensions
Process only files with specific extensions:
```bash
mdrs -e py,js,ts
```

### Combine Options
Process specific directory, with custom output and extensions:
```bash
mdrs /path/to/directory -o output.md -e py,js
```

## 📋 Command Line Arguments

| Argument | Short | Long | Description | Default |
|----------|-------|------|-------------|---------|
| `input_dir` | - | - | Directory to scan | Current directory (.) |
| `output` | `-o` | `--output` | Output markdown file path | llm.md |
| `extensions` | `-e` | `--extensions` | Comma-separated file extensions to include | None (all files) |

## 📄 Output Format

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

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🙏 Acknowledgments

- [walkdir](https://github.com/BurntSushi/walkdir) for efficient directory traversal
- [clap](https://github.com/clap-rs/clap) for command-line argument parsing
- [anyhow](https://github.com/dtolnay/anyhow) for error handling 