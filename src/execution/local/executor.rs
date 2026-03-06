use super::discovery::find_kernel;
use crate::execution::types::{ExecutionConfig, ExecutionError, ExecutionResult};
use crate::execution::ExecutionBackend;
use anyhow::{Context, Result};
use std::process::Command;

/// Local execution backend using Python subprocess
///
/// This implementation uses Python's jupyter_client library to execute code.
/// While not as efficient as direct ZMQ communication, it's more reliable and
/// doesn't require complex ZMQ bindings.
pub struct LocalExecutor {
    config: ExecutionConfig,
    kernel_name: String,
}

impl LocalExecutor {
    /// Create a new local executor
    pub fn new(config: ExecutionConfig) -> Result<Self> {
        Ok(Self {
            config,
            kernel_name: String::new(),
        })
    }

    /// Execute Python code to run a cell
    fn execute_cell_via_python(&self, code: &str) -> Result<ExecutionResult> {
        // Create a temporary Python script to execute the code
        let python_script = format!(
            r#"
import sys
import json
from jupyter_client import KernelManager

# Start kernel
km = KernelManager(kernel_name='{}')
km.start_kernel()
kc = km.client()
kc.start_channels()
kc.wait_for_ready(timeout={})

# Execute code
msg_id = kc.execute('''{}''')

# Collect outputs
outputs = []
execution_count = None
error_info = None

while True:
    try:
        msg = kc.get_iopub_msg(timeout=1)
        msg_type = msg['header']['msg_type']
        content = msg['content']

        # Check if this message is for our execution
        if msg.get('parent_header', {{}}).get('msg_id') != msg_id:
            continue

        if msg_type == 'status':
            if content['execution_state'] == 'idle':
                break
        elif msg_type == 'stream':
            outputs.append({{
                'output_type': 'stream',
                'name': content['name'],
                'text': content['text']
            }})
        elif msg_type == 'display_data':
            outputs.append({{
                'output_type': 'display_data',
                'data': content['data'],
                'metadata': content.get('metadata', {{}})
            }})
        elif msg_type == 'execute_result':
            execution_count = content['execution_count']
            outputs.append({{
                'output_type': 'execute_result',
                'execution_count': execution_count,
                'data': content['data'],
                'metadata': content.get('metadata', {{}})
            }})
        elif msg_type == 'error':
            error_info = {{
                'ename': content['ename'],
                'evalue': content['evalue'],
                'traceback': content['traceback']
            }}
            outputs.append({{
                'output_type': 'error',
                'ename': content['ename'],
                'evalue': content['evalue'],
                'traceback': content['traceback']
            }})
    except Exception as e:
        break

# Shutdown kernel
kc.stop_channels()
km.shutdown_kernel()

# Output result as JSON
result = {{
    'outputs': outputs,
    'execution_count': execution_count,
    'error': error_info
}}
print(json.dumps(result))
"#,
            self.kernel_name,
            self.config.timeout.as_secs(),
            code.replace("'", "\\'").replace("\n", "\\n")
        );

        // Execute Python script
        let output = Command::new("python3")
            .arg("-c")
            .arg(&python_script)
            .output()
            .context("Failed to execute Python script")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Python execution failed: {}", stderr);
        }

        // Parse result
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value =
            serde_json::from_str(&stdout).context("Failed to parse Python output")?;

        // Convert to ExecutionResult
        let outputs_json = result["outputs"]
            .as_array()
            .context("Missing outputs in result")?;

        let mut outputs = Vec::new();
        for output_json in outputs_json {
            outputs.push(serde_json::from_value(output_json.clone())?);
        }

        let execution_count = result["execution_count"].as_i64();

        let error = if let Some(error_json) = result.get("error") {
            if !error_json.is_null() {
                Some(ExecutionError {
                    ename: error_json["ename"].as_str().unwrap_or("").to_string(),
                    evalue: error_json["evalue"].as_str().unwrap_or("").to_string(),
                    traceback: error_json["traceback"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            } else {
                None
            }
        } else {
            None
        };

        if let Some(error) = error {
            Ok(ExecutionResult::error(outputs, execution_count, error))
        } else {
            Ok(ExecutionResult::success(outputs, execution_count))
        }
    }
}

#[async_trait::async_trait]
impl ExecutionBackend for LocalExecutor {
    async fn start(&mut self) -> Result<()> {
        // Find kernel
        let (kernel_name, _kernel_spec_path) = find_kernel(
            self.config.kernel_name.as_deref(),
            None, // Notebook kernel will be passed from command
        )?;

        self.kernel_name = kernel_name;

        // Check that Python and jupyter_client are available
        let check = Command::new("python3")
            .arg("-c")
            .arg("import jupyter_client")
            .output()
            .context("Failed to check for jupyter_client")?;

        if !check.status.success() {
            anyhow::bail!("jupyter_client not found. Install it with: pip install jupyter_client");
        }

        Ok(())
    }

    async fn execute_code(
        &mut self,
        code: &str,
        _cell_id: Option<&str>,
    ) -> Result<ExecutionResult> {
        // Execute in a blocking task since we're using subprocess
        // Note: cell_id is not used in local execution mode
        let code = code.to_string();
        let kernel_name = self.kernel_name.clone();
        let timeout = self.config.timeout;
        let allow_errors = self.config.allow_errors;

        tokio::task::spawn_blocking(move || {
            let executor = LocalExecutor {
                config: ExecutionConfig {
                    mode: crate::execution::types::ExecutionMode::Local,
                    timeout,
                    kernel_name: Some(kernel_name.clone()),
                    allow_errors,
                    notebook_path: None,
                },
                kernel_name,
            };
            executor.execute_cell_via_python(&code)
        })
        .await
        .context("Task join error")?
    }

    async fn stop(&mut self) -> Result<()> {
        // Nothing to cleanup with subprocess approach
        Ok(())
    }
}
