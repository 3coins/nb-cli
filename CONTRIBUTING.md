# Contributing to nb-cli

Thank you for your interest in contributing to nb-cli! This guide will help you get started with development.

## Development Setup

### Prerequisites

1. **Rust** (1.70 or later)
   ```bash
   # Install Rust using rustup
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Python** (3.11 or later) - For local execution features
   ```bash
   python3 --version
   ```

### Getting Started

1. **Clone the repository**
   ```bash
   git clone https://github.com/jupyter-ai-contrib/nb-cli.git
   cd nb-cli
   ```

2. **Build the project**
   ```bash
   cargo build --release
   ```

   The binary will be available at `target/release/nb`.

3. **Run during development**
   ```bash
   # Use cargo run to build and run without installing
   cargo run -- --help
   cargo run -- notebook create test.ipynb
   cargo run -- cell add test.ipynb --source "print('hello')"
   ```

4. **Install Python dependencies** (optional, for local execution)
   ```bash
   pip install -r requirements.txt
   ```

### Project Structure

```
nb-cli/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── commands/            # Command implementations
│   └── tests/               # Unit tests
├── tests/                   # Integration tests & fixtures
├── examples/                # Example notebooks
└── Cargo.toml              # Dependencies
```

## Testing

For comprehensive testing instructions, see **[tests/README.md](tests/README.md)**.

### Quick Commands

```bash
# Run all tests (test environment auto-setup)
cargo test

# Run specific test suites
cargo test --test integration_local_mode
cargo test --test integration_execution

# Run with output
cargo test -- --nocapture
```

**Note**: Execution tests automatically set up the Python environment if needed. See [tests/README.md](tests/README.md) for details.

## Code Quality

```bash
# Format code
cargo fmt

# Run linter
cargo clippy -- -D warnings

# Run tests
cargo test
```

Follow standard Rust conventions and write tests for new features.

## Submitting Changes

1. **Fork** the repository and create a feature branch
2. **Make changes** and ensure tests pass
3. **Run** `cargo fmt` and `cargo clippy`
4. **Commit** with clear, descriptive messages
5. **Push** to your fork and open a pull request

## Reporting Issues

Open an [issue](https://github.com/jupyter-ai-contrib/nb-cli/issues) to report bugs or request features. Include:
- Version and OS
- Steps to reproduce
- Error messages or logs

## Getting Help

- Main documentation: [README.md](README.md)
- Testing guide: [tests/README.md](tests/README.md)
- Questions: Open an issue

Please be respectful and constructive in all interactions.
