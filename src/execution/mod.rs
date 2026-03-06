//! Execution module for running notebook cells
//!
//! Supports two execution modes:
//! - **Local**: Direct kernel connection using runtimelib + ZMQ
//! - **Remote**: Jupyter Server API using HTTP + WebSocket

pub mod types;
pub mod local;
pub mod remote;

use anyhow::Result;
use types::{ExecutionConfig, ExecutionMode, ExecutionResult};

/// Backend for executing code
///
/// Implementations provide either local (direct kernel) or remote (Jupyter server) execution
#[async_trait::async_trait]
pub trait ExecutionBackend: Send {
    /// Start the backend (spawn kernel or create session)
    async fn start(&mut self) -> Result<()>;

    /// Execute code and return result with outputs
    ///
    /// # Arguments
    /// * `code` - The code to execute
    /// * `cell_id` - Optional cell ID for remote execution (used by Jupyter Server)
    async fn execute_code(&mut self, code: &str, cell_id: Option<&str>) -> Result<ExecutionResult>;

    /// Stop the backend (cleanup kernel or close session)
    async fn stop(&mut self) -> Result<()>;
}

/// Create an execution backend based on configuration
pub fn create_backend(config: ExecutionConfig) -> Result<Box<dyn ExecutionBackend>> {
    match config.mode.clone() {
        ExecutionMode::Local => {
            Ok(Box::new(local::LocalExecutor::new(config)?))
        }
        ExecutionMode::Remote { server_url, token } => {
            Ok(Box::new(remote::RemoteExecutor::new(config, server_url, token)?))
        }
    }
}
