---
name: jupyter-cli
description: Use the custom Rust-based jupyter-cli for working with Jupyter notebooks instead of built-in tools. Provides programmatic access to notebook operations (read, create, edit cells, execute, search) with JSON output for AI agents. Supports both local file-based and remote real-time collaboration modes. Invoke when working with .ipynb files in this project.
---

# Working with Jupyter Notebooks using jupyter-cli

Use the custom `jupyter-cli` tool (Rust-based CLI) for programmatic notebook manipulation instead of Claude Code's built-in notebook operations.

## Project Context

- **Location**: `/Users/pijain/projects/2026/jupyter-cli`
- **Binary**: `./target/debug/jupyter-cli` (build with `cargo build` if needed)
- **Output**: JSON by default (ideal for parsing), use `-f text` for human-readable format

## Command Structure

```bash
jupyter-cli notebook <command>  # create, read, execute, search
jupyter-cli cell <command>      # add, update, delete, execute
jupyter-cli output <command>    # clear
jupyter-cli connect/status/disconnect  # Connection management
```

Use `--help` with any command for detailed options.

## Operating Modes

### Local Mode (Default)
Direct file manipulation:
```bash
jupyter-cli cell add <file> --source "code"
```

### Remote Mode
Real-time sync with JupyterLab (use after `jupyter-cli connect` or with `--server`/`--token`):
```bash
jupyter-cli connect --server http://localhost:8888 --token <token>
jupyter-cli cell add <file> --source "code"  # Syncs instantly to open notebook
jupyter-cli status  # Check connection
jupyter-cli disconnect
```

## Essential Operations

### Reading
```bash
# Overview with all cells
jupyter-cli notebook read <file>

# Specific cell
jupyter-cli notebook read <file> --cell 0
jupyter-cli notebook read <file> --cell-id "my-cell"

# With execution outputs
jupyter-cli notebook read <file> -c 0 --with-outputs

# Filter by type
jupyter-cli notebook read <file> --only-code
jupyter-cli notebook read <file> --only-markdown
```

### Creating & Editing
```bash
# Create
jupyter-cli notebook create <file> [--template basic|markdown]

# Add cell
jupyter-cli cell add <file> --source "code" [--type code|markdown]

# Update cell
jupyter-cli cell update <file> --cell 0 --source "new content"
jupyter-cli cell update <file> --cell 0 --append "\nmore code"

# Delete
jupyter-cli cell delete <file> --cell 0
```

### Execution
```bash
# Execute single cell
jupyter-cli cell execute <file> --cell 0

# Execute notebook
jupyter-cli notebook execute <file> [--start N --end M]

# With options
jupyter-cli cell execute <file> -c 0 --timeout 60 --allow-errors
```

### Searching
```bash
# Search in source
jupyter-cli notebook search <file> <pattern>

# Find errors
jupyter-cli notebook search <file> --with-errors

# Search in outputs or all
jupyter-cli notebook search <file> <pattern> --scope output|all
```

### Output Management
```bash
# Clear all
jupyter-cli output clear <file> --all

# Clear specific cell
jupyter-cli output clear <file> --cell 0
```

## Cell Referencing

- **By index**: `--cell N` (0-based, supports `-1` for last cell)
- **By ID**: `--cell-id "id"` (stable, doesn't change when cells move)

## Typical Agent Workflows

**Analyze code**:
```bash
jupyter-cli notebook read <file> --only-code
```

**Debug**:
```bash
jupyter-cli notebook search <file> --with-errors
jupyter-cli notebook read <file> -c N --with-outputs
```

**Fix and verify**:
```bash
jupyter-cli cell update <file> -c N --source "fixed"
jupyter-cli cell execute <file> -c N
```

**Build notebook**:
```bash
jupyter-cli notebook create <file>
jupyter-cli cell add <file> --source "import pandas"
jupyter-cli cell add <file> --source "# Title" --type markdown
```

## Important Notes

- All commands output JSON following nbformat specification
- Escape sequences (`\n`, `\t`) automatically interpreted in `--source`/`--append`
- Use `connect` command to save server credentials for repeated operations
- Real-time sync via Y.js when working with open JupyterLab notebooks
