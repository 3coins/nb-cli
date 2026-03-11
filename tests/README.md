# Integration Tests

This directory contains integration tests for the nb-cli tool's local mode operations.

## Test Structure

```
tests/
├── integration_local_mode.rs  # Main integration test suite
├── fixtures/                   # Test notebook fixtures
│   ├── empty.ipynb            # Empty notebook with no cells
│   ├── basic.ipynb            # Basic notebook with one empty cell
│   ├── with_code.ipynb        # Notebook with code cells
│   ├── mixed_cells.ipynb      # Notebook with mixed cell types
│   └── with_outputs.ipynb     # Notebook with cell outputs
└── README.md                   # This file
```

## Running Tests

Run all integration tests:
```bash
cargo test --test integration_local_mode
```

Run specific tests:
```bash
cargo test --test integration_local_mode test_create_empty_notebook
cargo test --test integration_local_mode test_add_cell
```

Run tests with output:
```bash
cargo test --test integration_local_mode -- --nocapture
```
