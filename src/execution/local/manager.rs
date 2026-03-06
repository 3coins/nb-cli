use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

/// Connection information for a Jupyter kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConnection {
    pub transport: String,
    pub ip: String,
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    pub signature_scheme: String,
    pub key: String,
}

impl KernelConnection {
    /// Create new connection info with allocated ports
    pub fn new() -> Result<Self> {
        // Allocate 5 unique ports
        let ports = allocate_ports(5)?;

        Ok(Self {
            transport: "tcp".to_string(),
            ip: "127.0.0.1".to_string(),
            shell_port: ports[0],
            iopub_port: ports[1],
            stdin_port: ports[2],
            control_port: ports[3],
            hb_port: ports[4],
            signature_scheme: "hmac-sha256".to_string(),
            key: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// Get ZMQ endpoint for a given port
    pub fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }

    /// Get shell channel endpoint
    pub fn shell_endpoint(&self) -> String {
        self.endpoint(self.shell_port)
    }

    /// Get iopub channel endpoint
    pub fn iopub_endpoint(&self) -> String {
        self.endpoint(self.iopub_port)
    }
}

/// Manages a local Jupyter kernel process
pub struct LocalKernelManager {
    kernel_spec_path: PathBuf,
    connection_file: PathBuf,
    connection_info: KernelConnection,
    process: Option<Child>,
}

impl LocalKernelManager {
    /// Create a new kernel manager
    pub fn new(kernel_spec_path: PathBuf) -> Result<Self> {
        let connection_info = KernelConnection::new()?;

        // Create temporary connection file
        let temp_dir = std::env::temp_dir();
        let connection_file = temp_dir.join(format!(
            "jupyter_kernel_{}.json",
            uuid::Uuid::new_v4()
        ));

        Ok(Self {
            kernel_spec_path,
            connection_file,
            connection_info,
            process: None,
        })
    }

    /// Start the kernel process
    pub async fn start(&mut self) -> Result<()> {
        // Write connection file
        let connection_json = serde_json::to_string_pretty(&self.connection_info)?;
        fs::write(&self.connection_file, connection_json)
            .context("Failed to write kernel connection file")?;

        // Read kernel spec to get command
        let kernel_json_path = self.kernel_spec_path.join("kernel.json");
        let kernel_json = fs::read_to_string(&kernel_json_path)
            .context("Failed to read kernel.json")?;
        let kernel_spec: serde_json::Value = serde_json::from_str(&kernel_json)?;

        let argv = kernel_spec["argv"]
            .as_array()
            .context("Invalid kernel spec: missing 'argv'")?;

        // Build command, replacing {connection_file} placeholder
        let connection_file_str = self.connection_file.to_string_lossy();
        let mut cmd_parts: Vec<String> = argv
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.replace("{connection_file}", &connection_file_str))
            .collect();

        if cmd_parts.is_empty() {
            anyhow::bail!("Invalid kernel spec: empty argv");
        }

        let program = cmd_parts.remove(0);
        let args = cmd_parts;

        // Spawn kernel process
        let child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn kernel process")?;

        self.process = Some(child);

        // Wait for kernel to initialize
        sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    /// Stop the kernel process
    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            // Try graceful shutdown first
            let _ = process.kill();
            let _ = process.wait();
        }

        // Clean up connection file
        if self.connection_file.exists() {
            let _ = fs::remove_file(&self.connection_file);
        }

        Ok(())
    }

    /// Get connection information
    pub fn connection_info(&self) -> &KernelConnection {
        &self.connection_info
    }

    /// Check if kernel process is running
    pub fn is_alive(&mut self) -> bool {
        if let Some(ref mut process) = self.process {
            process.try_wait().ok().flatten().is_none()
        } else {
            false
        }
    }
}

impl Drop for LocalKernelManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Allocate n available TCP ports
fn allocate_ports(n: usize) -> Result<Vec<u16>> {
    use std::net::TcpListener;

    let mut ports = Vec::new();
    for _ in 0..n {
        let listener = TcpListener::bind("127.0.0.1:0")
            .context("Failed to allocate port")?;
        let port = listener.local_addr()?.port();
        ports.push(port);
        // Listener is dropped here, freeing the port
    }

    Ok(ports)
}
