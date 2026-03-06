use crate::commands::common;
use crate::notebook;
use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use nbformat::v4::Cell;
use serde::Serialize;

#[derive(Clone, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum CellType {
    Code,
    Markdown,
    Raw,
}

#[derive(Clone, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Parser)]
pub struct UpdateCellArgs {
    /// Path to notebook file
    pub file: String,

    /// Cell index (supports negative)
    #[arg(short = 'c', long = "cell", value_name = "INDEX", conflicts_with = "cell_id")]
    pub cell: Option<i32>,

    /// Cell ID
    #[arg(short = 'i', long = "cell-id", value_name = "ID", conflicts_with = "cell")]
    pub cell_id: Option<String>,

    /// New source content (use '-' for stdin)
    #[arg(short = 's', long = "source", value_name = "TEXT", conflicts_with = "append")]
    pub source: Option<String>,

    /// Append to existing source (conflicts with --source)
    #[arg(short = 'a', long = "append", value_name = "TEXT", conflicts_with = "source")]
    pub append: Option<String>,

    /// Change cell type
    #[arg(short = 't', long = "type", value_name = "TYPE")]
    pub cell_type: Option<CellType>,

    /// Output format
    #[arg(short = 'f', long = "format", default_value = "json", value_name = "FORMAT")]
    pub format: OutputFormat,
}

#[derive(Serialize)]
struct UpdateCellResult {
    file: String,
    cell_id: String,
    index: usize,
    updated: Vec<String>,
}

pub fn execute(args: UpdateCellArgs) -> Result<()> {
    // Validate that at least one modification is specified
    if args.source.is_none() && args.append.is_none() && args.cell_type.is_none() {
        bail!("Must specify at least one of: --source, --append, or --type");
    }

    // Validate that cell selector is specified
    if args.cell.is_none() && args.cell_id.is_none() {
        bail!("Must specify --cell or --cell-id");
    }

    // Read notebook
    let original_content = std::fs::read_to_string(&args.file)
        .context("Failed to read original notebook file")?;
    println!("=== DEBUG: ORIGINAL FILE CONTENT ===");
    println!("{}", original_content);
    println!("=== END ORIGINAL ===\n");

    let mut notebook = notebook::read_notebook(&args.file)
        .context("Failed to read notebook")?;

    // Find the target cell
    let (index, cell_id) = if let Some(cell_index) = args.cell {
        let idx = common::normalize_index(cell_index, notebook.cells.len())?;
        let id = notebook.cells[idx].id().to_string();
        (idx, id)
    } else if let Some(ref id) = args.cell_id {
        let (idx, cell) = common::find_cell_by_id(&notebook.cells, id)?;
        (idx, cell.id().to_string())
    } else {
        unreachable!("Already validated cell selector");
    };

    let mut updates = Vec::new();

    // Apply modifications
    let cell = &mut notebook.cells[index];

    // Update source if specified
    if let Some(ref source_text) = args.source {
        let new_source = common::parse_source(source_text)?;
        match cell {
            Cell::Code { source, execution_count, .. } => {
                *source = new_source;
                *execution_count = None; // Reset execution count when modifying source
                updates.push("source replaced".to_string());
            }
            Cell::Markdown { source, .. } => {
                *source = new_source;
                updates.push("source replaced".to_string());
            }
            Cell::Raw { source, .. } => {
                *source = new_source;
                updates.push("source replaced".to_string());
            }
        }
    }

    // Append to source if specified
    if let Some(ref append_text) = args.append {
        let append_source = common::parse_source(append_text)?;
        match cell {
            Cell::Code { source, execution_count, .. } => {
                source.extend(append_source);
                *execution_count = None; // Reset execution count when modifying source
                updates.push("source appended".to_string());
            }
            Cell::Markdown { source, .. } => {
                source.extend(append_source);
                updates.push("source appended".to_string());
            }
            Cell::Raw { source, .. } => {
                source.extend(append_source);
                updates.push("source appended".to_string());
            }
        }
    }

    // Change cell type if specified
    if let Some(new_type) = args.cell_type {
        let old_cell = notebook.cells.remove(index);
        let (old_id, old_metadata, old_source) = match old_cell {
            Cell::Code { id, metadata, source, .. } => (id, metadata, source),
            Cell::Markdown { id, metadata, source, .. } => (id, metadata, source),
            Cell::Raw { id, metadata, source } => (id, metadata, source),
        };

        let new_cell = match new_type {
            CellType::Code => Cell::Code {
                id: old_id,
                metadata: old_metadata,
                execution_count: None,
                source: old_source,
                outputs: vec![],
            },
            CellType::Markdown => Cell::Markdown {
                id: old_id,
                metadata: old_metadata,
                source: old_source,
                attachments: None,
            },
            CellType::Raw => Cell::Raw {
                id: old_id,
                metadata: old_metadata,
                source: old_source,
            },
        };

        notebook.cells.insert(index, new_cell);
        let type_name = match new_type {
            CellType::Code => "code",
            CellType::Markdown => "markdown",
            CellType::Raw => "raw",
        };
        updates.push(format!("type changed to {}", type_name));
    }

    // Write notebook atomically
    notebook::write_notebook_atomic(&args.file, &notebook)
        .context("Failed to write notebook")?;

    // Read back the updated content for debugging
    let updated_content = std::fs::read_to_string(&args.file)
        .context("Failed to read updated notebook file")?;
    println!("=== DEBUG: UPDATED FILE CONTENT ===");
    println!("{}", updated_content);
    println!("=== END UPDATED ===\n");

    // Show the diff
    println!("=== DEBUG: FILE DIFF ===");
    show_diff(&original_content, &updated_content);
    println!("=== END DIFF ===\n");

    // Output result
    let result = UpdateCellResult {
        file: args.file.clone(),
        cell_id,
        index,
        updated: updates,
    };

    output_result(&result, &args.format)?;

    Ok(())
}

fn output_result(result: &UpdateCellResult, format: &OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            println!("Updated cell at index {}: {}", result.index, result.file);
            println!("Cell ID: {}", result.cell_id);
            println!("Changes: {}", result.updated.join(", "));
        }
    }
    Ok(())
}

fn show_diff(original: &str, updated: &str) {
    let original_lines: Vec<&str> = original.lines().collect();
    let updated_lines: Vec<&str> = updated.lines().collect();

    let max_lines = std::cmp::max(original_lines.len(), updated_lines.len());

    for i in 0..max_lines {
        let orig_line = original_lines.get(i).unwrap_or(&"");
        let upd_line = updated_lines.get(i).unwrap_or(&"");

        if orig_line != upd_line {
            if !orig_line.is_empty() {
                println!("- [Line {}]: {}", i + 1, orig_line);
            }
            if !upd_line.is_empty() {
                println!("+ [Line {}]: {}", i + 1, upd_line);
            }
        }
    }
}
