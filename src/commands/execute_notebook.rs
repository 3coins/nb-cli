use crate::execution::remote::ydoc::YDocClient;
use crate::execution::{create_backend, types::ExecutionConfig, types::ExecutionMode};
use crate::notebook::{read_notebook, write_notebook_atomic};
use anyhow::{Context, Result};
use clap::Args;
use nbformat::v4::Cell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Args)]
pub struct ExecuteNotebookArgs {
    /// Path to the notebook file
    pub file: String,

    /// Kernel to use (overrides notebook metadata)
    #[arg(short, long)]
    pub kernel: Option<String>,

    /// Timeout in seconds per cell (default: 30)
    #[arg(short, long, default_value = "30")]
    pub timeout: u64,

    /// Continue despite errors
    #[arg(long)]
    pub allow_errors: bool,

    /// Start cell index (inclusive)
    #[arg(long)]
    pub start: Option<i32>,

    /// End cell index (inclusive)
    #[arg(long)]
    pub end: Option<i32>,

    /// Remote server URL (enables remote mode)
    #[arg(long)]
    pub server: Option<String>,

    /// Authentication token for remote server
    #[arg(long)]
    pub token: Option<String>,

    /// Output format: json or text
    #[arg(long, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Clone, Debug)]
pub enum OutputFormat {
    Json,
    Text,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "text" => Ok(OutputFormat::Text),
            _ => anyhow::bail!("Invalid format: '{}'. Must be 'json' or 'text'", s),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ExecuteNotebookResult {
    success: bool,
    total_cells: usize,
    executed_cells: usize,
    failed_cells: usize,
}

pub fn execute(args: ExecuteNotebookArgs) -> Result<()> {
    // Create Tokio runtime for async execution
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(execute_async(args))
}

async fn execute_async(args: ExecuteNotebookArgs) -> Result<()> {
    // Read notebook
    let mut notebook = read_notebook(&args.file)
        .context("Failed to read notebook")?;

    // Determine cell range
    let start_idx = if let Some(start) = args.start {
        crate::commands::common::normalize_index(start, notebook.cells.len())?
    } else {
        0
    };

    let end_idx = if let Some(end) = args.end {
        crate::commands::common::normalize_index(end, notebook.cells.len())?
    } else {
        notebook.cells.len().saturating_sub(1)
    };

    if start_idx > end_idx {
        anyhow::bail!("Start index {} is greater than end index {}", start_idx, end_idx);
    }

    // Determine execution mode
    let mode = if let Some(server_url) = args.server {
        let token = args.token.context(
            "Must specify --token when using --server"
        )?;
        ExecutionMode::Remote { server_url, token }
    } else {
        ExecutionMode::Local
    };

    // Get kernel from notebook metadata if not specified
    let notebook_kernel = notebook.metadata.kernelspec.as_ref()
        .map(|ks| ks.name.as_str());

    // Extract notebook filename for remote session matching
    let notebook_filename = std::path::Path::new(&args.file)
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from);

    // Create execution config
    let config = ExecutionConfig {
        mode: mode.clone(),
        timeout: Duration::from_secs(args.timeout),
        kernel_name: args.kernel.or_else(|| notebook_kernel.map(String::from)),
        allow_errors: args.allow_errors,
        notebook_path: notebook_filename.clone(),
    };

    // Create and start backend (reuse kernel for all cells)
    let mut backend = create_backend(config)?;
    backend.start().await
        .context("Failed to start execution backend")?;

    // Execute cells in range and collect results
    let mut executed_count = 0;
    let mut failed_count = 0;
    let total_cells = notebook.cells.len();
    let mut execution_results: HashMap<usize, crate::execution::types::ExecutionResult> =
        HashMap::new();

    for (i, cell) in notebook.cells.iter().enumerate() {
        // Skip cells outside range
        if i < start_idx || i > end_idx {
            continue;
        }

        // Skip non-code cells
        if !matches!(cell, Cell::Code { .. }) {
            continue;
        }

        // Get cell source and cell_id
        let source = crate::commands::common::cell_to_string(cell);
        let cell_id = crate::commands::common::cell_id_to_string(cell);

        // Print progress (only in text mode)
        if matches!(args.format, OutputFormat::Text) {
            eprintln!("Executing cell {} of {}...", i + 1, total_cells);
        }

        // Execute cell
        match backend.execute_code(&source, Some(&cell_id)).await {
            Ok(result) => {
                let success = result.success;

                // Store result for later processing
                execution_results.insert(i, result);
                executed_count += 1;

                if !success {
                    failed_count += 1;

                    if matches!(args.format, OutputFormat::Text) {
                        eprintln!("  ✗ Cell {} failed", i);
                        if let Some(error) = execution_results.get(&i).and_then(|r| r.error.as_ref())
                        {
                            eprintln!("    Error: {}: {}", error.ename, error.evalue);
                        }
                    }

                    // Stop on error unless --allow-errors
                    if !args.allow_errors {
                        backend.stop().await?;
                        anyhow::bail!("Execution stopped at cell {} due to error", i);
                    }
                } else if matches!(args.format, OutputFormat::Text) {
                    eprintln!("  ✓ Cell {} completed", i);
                }
            }
            Err(e) => {
                backend.stop().await?;
                return Err(e).context(format!("Failed to execute cell {}", i));
            }
        }
    }

    // Stop backend
    backend.stop().await?;

    // Update notebook cells with execution results
    for (i, result) in &execution_results {
        if let Cell::Code {
            ref mut outputs,
            ref mut execution_count,
            ..
        } = notebook.cells[*i]
        {
            *outputs = result.outputs.clone();
            *execution_count = result.execution_count.map(|c| c as i32);
        }
    }

    // Persist changes based on mode
    match mode {
        ExecutionMode::Local => {
            // Write notebook to file
            write_notebook_atomic(&args.file, &notebook)
                .context("Failed to write notebook")?;
        }
        ExecutionMode::Remote { server_url: ref server_url, token: ref token } => {
            // Sync outputs to JupyterLab via Y.js
            let notebook_path = notebook_filename.context("No notebook filename for Y.js sync")?;

            match YDocClient::connect(server_url.clone(), token.clone(), notebook_path).await {
                Ok(mut ydoc_client) => {
                    eprintln!("\nSyncing outputs to JupyterLab via Y.js...");

                    // Update each executed cell's outputs and execution_count
                    for (i, result) in &execution_results {
                        // Update outputs
                        if let Err(e) = ydoc_client.update_cell_outputs(*i, result.outputs.clone())
                        {
                            eprintln!(
                                "  Warning: Failed to update outputs for cell {}: {}",
                                i, e
                            );
                        }

                        // Update execution_count
                        if let Err(e) =
                            ydoc_client.update_cell_execution_count(*i, result.execution_count)
                        {
                            eprintln!(
                                "  Warning: Failed to update execution count for cell {}: {}",
                                i, e
                            );
                        }
                    }

                    // Sync changes to server
                    match ydoc_client.sync().await {
                        Ok(_) => {
                            eprintln!("✓ Outputs synced to JupyterLab in real-time");
                        }
                        Err(e) => {
                            eprintln!("  Warning: Failed to sync Y.js updates: {}", e);
                        }
                    }

                    // Close connection
                    let _ = ydoc_client.close().await;
                }
                Err(e) => {
                    eprintln!(
                        "\nWarning: Could not connect to Y.js document: {}",
                        e
                    );
                    eprintln!("  Outputs will not appear in JupyterLab UI automatically.");
                    eprintln!(
                        "  Make sure jupyter-server-documents is installed: pip install jupyter-server-documents"
                    );
                }
            }
        }
    }

    // Output result
    let output_result = ExecuteNotebookResult {
        success: failed_count == 0,
        total_cells: end_idx - start_idx + 1,
        executed_cells: executed_count,
        failed_cells: failed_count,
    };

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&output_result)?);
        }
        OutputFormat::Text => {
            println!("\n{}", "=".repeat(50));
            if output_result.success {
                println!("✓ Notebook executed successfully");
            } else {
                println!("✗ Notebook execution completed with errors");
            }
            println!("Total cells in range: {}", output_result.total_cells);
            println!("Executed: {}", output_result.executed_cells);
            println!("Failed: {}", output_result.failed_cells);

            if matches!(mode, ExecutionMode::Local) {
                println!("\nNotebook updated: {}", args.file);
            } else {
                println!("\n(Executed via Jupyter Server)");
            }
        }
    }

    if !output_result.success {
        std::process::exit(1);
    }

    Ok(())
}
