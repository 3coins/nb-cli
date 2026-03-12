# Agent Instructions for Notebook Operations

**IMPORTANT**: When working with Jupyter notebook files (`.ipynb`), use the `nb` command-line tool instead of built-in Read/Write/Edit functions.

## Quick Reference

If the `nb` command is not available in your PATH, it should be available at ./target/debug/nb. Use it for all notebook operations:

```bash
nb notebook create|read|execute|search <file.ipynb>
nb cell add|update|delete|execute <file.ipynb>
nb output clear <file.ipynb>
```

## When to Use

- Reading notebook structure or cell content
- Creating or modifying notebooks
- Adding, updating, or deleting cells
- Executing notebooks or individual cells
- Searching within notebooks

## Skill Reference

For detailed command documentation, refer to the `nb` skill in `.agents/skills/notebook-cli/SKILL.md`.

## Key Points

- Use `nb` instead of Read tool for `.ipynb` files
- Use `nb` instead of Write/Edit tools for notebook modifications
- Commands output JSON by default (use `-f text` for human-readable)
- Cell indexing starts at 0, supports negative indices (e.g., `-1` for last cell)
