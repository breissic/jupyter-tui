use anyhow::{Context, Result, bail};
use jupyter_protocol::ConnectionInfo;
use runtimelib::{KernelspecDir, list_kernelspecs, peek_ports, read_kernelspec_jsons};
use std::net::IpAddr;
use std::path::PathBuf;
use tokio::process::Child;

/// Manages the lifecycle of a Jupyter kernel subprocess.
///
/// Handles kernelspec discovery, connection file generation,
/// process spawning, and graceful shutdown.
pub struct KernelManager {
    /// The spawned kernel process
    process: Child,
    /// Connection info for communicating with the kernel
    connection_info: ConnectionInfo,
    /// Path to the connection file on disk
    connection_file_path: PathBuf,
}

impl KernelManager {
    /// Discover available kernelspecs, pick one (defaulting to python3),
    /// generate a connection file, and spawn the kernel process.
    pub async fn start(kernel_name: Option<&str>) -> Result<Self> {
        let kernel_name = kernel_name.unwrap_or("python3");

        // Discover available kernelspecs
        // First try ask_jupyter() to get data dirs (handles pyenv, conda, etc.),
        // then fall back to runtimelib's built-in list_kernelspecs().
        let kernelspecs = discover_kernelspecs().await;
        if kernelspecs.is_empty() {
            bail!("No Jupyter kernelspecs found. Is Jupyter installed?");
        }

        let kernelspec = find_kernelspec(&kernelspecs, kernel_name)?;

        // Generate connection info with random open ports
        let ip: IpAddr = "127.0.0.1".parse()?;
        let ports = peek_ports(ip, 5)
            .await
            .context("Failed to find open ports for kernel")?;

        let key = uuid::Uuid::new_v4().to_string();

        let connection_info = ConnectionInfo {
            ip: "127.0.0.1".to_string(),
            transport: jupyter_protocol::connection_info::Transport::TCP,
            shell_port: ports[0],
            iopub_port: ports[1],
            stdin_port: ports[2],
            control_port: ports[3],
            hb_port: ports[4],
            key,
            signature_scheme: "hmac-sha256".to_string(),
            kernel_name: Some(kernel_name.to_string()),
        };

        // Write connection file to Jupyter runtime directory
        let runtime_dir = runtimelib::dirs::runtime_dir();
        tokio::fs::create_dir_all(&runtime_dir).await?;

        let connection_file_path =
            runtime_dir.join(format!("kernel-{}.json", uuid::Uuid::new_v4()));

        let connection_json = serde_json::to_string_pretty(&connection_info)?;
        tokio::fs::write(&connection_file_path, &connection_json).await?;

        // Spawn the kernel process
        let mut cmd = kernelspec.command(
            &connection_file_path,
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
        )?;

        let process = cmd
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn kernel process")?;

        // Give the kernel a moment to start and bind its ports
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(Self {
            process,
            connection_info,
            connection_file_path,
        })
    }

    /// Returns a reference to the connection info for this kernel.
    pub fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    /// Attempt graceful shutdown, then force-kill if needed.
    pub async fn shutdown(&mut self) -> Result<()> {
        // Try SIGTERM first (kill_on_drop handles this, but let's be explicit)
        let _ = self.process.kill().await;

        // Clean up the connection file
        let _ = tokio::fs::remove_file(&self.connection_file_path).await;

        Ok(())
    }

    /// Restart the kernel: shut down the current process, start a new one
    /// using the same kernelspec and connection info.
    pub async fn restart(&mut self) -> Result<()> {
        // Kill the existing process
        let _ = self.process.kill().await;

        // Determine kernel name
        let kernel_name = self
            .connection_info
            .kernel_name
            .as_deref()
            .unwrap_or("python3");

        // Discover kernelspec again
        let kernelspecs = discover_kernelspecs().await;
        let kernelspec = find_kernelspec(&kernelspecs, kernel_name)?;

        // Rewrite the connection file (ports stay the same)
        let connection_json = serde_json::to_string_pretty(&self.connection_info)?;
        tokio::fs::write(&self.connection_file_path, &connection_json).await?;

        // Spawn the new kernel process
        let mut cmd = kernelspec.command(
            &self.connection_file_path,
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
        )?;

        self.process = cmd
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn kernel process")?;

        // Give the kernel a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(())
    }

    /// Returns available kernelspec names for display/selection.
    pub async fn available_kernels() -> Result<Vec<String>> {
        let specs = discover_kernelspecs().await;
        Ok(specs.into_iter().map(|s| s.kernel_name).collect())
    }
}

impl Drop for KernelManager {
    fn drop(&mut self) {
        // Best-effort cleanup -- connection file removal
        let _ = std::fs::remove_file(&self.connection_file_path);
    }
}

/// Find a kernelspec by name from the discovered list.
fn find_kernelspec(kernelspecs: &[KernelspecDir], name: &str) -> Result<KernelspecDir> {
    kernelspecs
        .iter()
        .find(|k| k.kernel_name == name)
        .cloned()
        .with_context(|| {
            let available: Vec<&str> = kernelspecs.iter().map(|k| k.kernel_name.as_str()).collect();
            format!(
                "Kernelspec '{}' not found. Available: {:?}",
                name, available
            )
        })
}

/// Discover kernelspecs by querying `jupyter --paths --json` first,
/// which correctly reports data dirs for pyenv, conda, virtualenvs, etc.
/// Falls back to runtimelib's built-in `list_kernelspecs()` if the
/// jupyter command is unavailable.
async fn discover_kernelspecs() -> Vec<KernelspecDir> {
    // Try ask_jupyter() to get the real data dirs
    if let Ok(paths) = runtimelib::dirs::ask_jupyter().await {
        if let Some(data_dirs) = paths.get("data").and_then(|v| v.as_array()) {
            let mut kernelspecs = Vec::new();
            let mut seen = std::collections::HashSet::new();

            for dir_value in data_dirs {
                if let Some(dir_str) = dir_value.as_str() {
                    let data_dir = PathBuf::from(dir_str);
                    let specs = read_kernelspec_jsons(&data_dir).await;
                    for spec in specs {
                        if seen.insert(spec.kernel_name.clone()) {
                            kernelspecs.push(spec);
                        }
                    }
                }
            }

            if !kernelspecs.is_empty() {
                return kernelspecs;
            }
        }
    }

    // Fallback to runtimelib's default discovery
    list_kernelspecs().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_kernelspecs_finds_python3() {
        let specs = discover_kernelspecs().await;
        let names: Vec<&str> = specs.iter().map(|s| s.kernel_name.as_str()).collect();
        assert!(
            names.contains(&"python3"),
            "Expected to find python3 kernelspec, found: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn test_start_kernel_and_execute() {
        use crate::kernel::client::{KernelClient, KernelMessage};
        use jupyter_protocol::JupyterMessageContent;

        // Start kernel (give it more time to start up)
        let mut manager = KernelManager::start(Some("python3"))
            .await
            .expect("Failed to start kernel");

        // Connect
        let (mut client, mut rx) = KernelClient::connect(manager.connection_info())
            .await
            .expect("Failed to connect to kernel");

        // Request kernel info to trigger a status message
        client
            .request_kernel_info()
            .await
            .expect("Failed to request kernel info");

        // Wait for kernel to be idle
        let mut kernel_ready = false;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(tokio::time::Duration::from_secs(3), rx.recv()).await {
                Ok(Some(KernelMessage::IoPub(msg))) => {
                    eprintln!(
                        "Got iopub message: {:?}",
                        std::mem::discriminant(&msg.content)
                    );
                    if let JupyterMessageContent::Status(s) = &msg.content {
                        let state = format!("{:?}", s.execution_state).to_lowercase();
                        eprintln!("  Kernel status: {}", state);
                        if state.contains("idle") {
                            kernel_ready = true;
                            break;
                        }
                    }
                }
                Ok(Some(other)) => {
                    eprintln!("Got other message: {:?}", other);
                }
                Ok(None) => {
                    eprintln!("Channel closed");
                    break;
                }
                Err(_) => {
                    eprintln!("Timeout waiting for message");
                    break;
                }
            }
        }
        assert!(kernel_ready, "Kernel did not become idle");

        // Execute code
        client
            .execute("print('hello from test')")
            .await
            .expect("Failed to execute");

        // Collect output
        let mut got_stream_output = false;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(KernelMessage::IoPub(msg))) => {
                    eprintln!("Exec iopub: {:?}", std::mem::discriminant(&msg.content));
                    if let JupyterMessageContent::StreamContent(stream) = &msg.content {
                        eprintln!("  Stream: {}", stream.text);
                        if stream.text.contains("hello from test") {
                            got_stream_output = true;
                            break;
                        }
                    }
                }
                _ => break,
            }
        }

        assert!(got_stream_output, "Did not receive expected stream output");

        // Shutdown
        let _ = client.shutdown(false).await;
        manager.shutdown().await.expect("Failed to shutdown");
    }
}
