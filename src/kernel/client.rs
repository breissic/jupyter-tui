use anyhow::{Context, Result};
use jupyter_protocol::{
    CompleteReply, CompleteRequest, ConnectionInfo, ExecuteRequest, JupyterMessage,
    JupyterMessageContent, KernelInfoRequest, ShutdownRequest,
};
use runtimelib::{
    ClientControlConnection, ClientShellConnection, ClientStdinConnection,
    create_client_control_connection, create_client_iopub_connection,
    create_client_shell_connection, create_client_stdin_connection,
};
use tokio::sync::mpsc;

/// Messages sent from the kernel client to the application.
#[derive(Debug)]
pub enum KernelMessage {
    /// A message received on the IOPub channel
    IoPub(JupyterMessage),
    /// A reply received on the shell channel
    ShellReply(JupyterMessage),
    /// The IOPub listener encountered an error
    IoPubError(String),
}

/// Async client for communicating with a Jupyter kernel over ZMQ.
///
/// Owns the shell and control connections for sending requests.
/// Spawns a background task to listen on IOPub and forward messages
/// through an mpsc channel.
pub struct KernelClient {
    shell: ClientShellConnection,
    control: ClientControlConnection,
    #[allow(dead_code)]
    stdin: ClientStdinConnection,
}

impl KernelClient {
    /// Connect to all kernel channels and start the IOPub listener.
    ///
    /// Returns the client and a receiver for IOPub/shell messages.
    pub async fn connect(
        connection_info: &ConnectionInfo,
    ) -> Result<(Self, mpsc::UnboundedReceiver<KernelMessage>)> {
        let session_id = uuid::Uuid::new_v4().to_string();

        let shell = create_client_shell_connection(connection_info, &session_id)
            .await
            .context("Failed to connect to shell channel")?;

        let mut iopub = create_client_iopub_connection(connection_info, "", &session_id)
            .await
            .context("Failed to connect to IOPub channel")?;

        let control = create_client_control_connection(connection_info, &session_id)
            .await
            .context("Failed to connect to control channel")?;

        let stdin = create_client_stdin_connection(connection_info, &session_id)
            .await
            .context("Failed to connect to stdin channel")?;

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn IOPub listener as a background task
        tokio::spawn(async move {
            loop {
                match iopub.read().await {
                    Ok(msg) => {
                        if tx.send(KernelMessage::IoPub(msg)).is_err() {
                            break; // Receiver dropped, shut down
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(KernelMessage::IoPubError(e.to_string()));
                        break;
                    }
                }
            }
        });

        Ok((
            Self {
                shell,
                control,
                stdin,
            },
            rx,
        ))
    }

    /// Send an execute_request to the kernel.
    /// Returns the msg_id of the sent message for correlating IOPub responses.
    pub async fn execute(&mut self, code: &str) -> Result<String> {
        let request = ExecuteRequest::new(code.to_string());
        let message: JupyterMessage = request.into();
        let msg_id = message.header.msg_id.clone();
        self.shell
            .send(message)
            .await
            .context("Failed to send execute request")?;
        Ok(msg_id)
    }

    /// Send a kernel_info_request.
    pub async fn request_kernel_info(&mut self) -> Result<()> {
        let request = KernelInfoRequest {};
        let message: JupyterMessage = request.into();
        self.shell
            .send(message)
            .await
            .context("Failed to send kernel_info_request")?;
        Ok(())
    }

    /// Read a reply from the shell channel (blocking until one arrives).
    pub async fn read_shell_reply(&mut self) -> Result<JupyterMessage> {
        self.shell
            .read()
            .await
            .context("Failed to read shell reply")
    }

    /// Send a shutdown request on the control channel.
    pub async fn shutdown(&mut self, restart: bool) -> Result<()> {
        let request = ShutdownRequest { restart };
        let message: JupyterMessage = request.into();
        self.control
            .send(message)
            .await
            .context("Failed to send shutdown request")?;
        Ok(())
    }

    /// Send an interrupt request on the control channel.
    pub async fn interrupt(&mut self) -> Result<()> {
        let request = jupyter_protocol::InterruptRequest {};
        let message: JupyterMessage = request.into();
        self.control
            .send(message)
            .await
            .context("Failed to send interrupt request")?;
        Ok(())
    }

    /// Send a complete_request and read back the reply.
    /// Returns the CompleteReply with match suggestions, or an error on timeout.
    pub async fn complete(&mut self, code: &str, cursor_pos: usize) -> Result<CompleteReply> {
        let request = CompleteRequest {
            code: code.to_string(),
            cursor_pos,
        };
        let message: JupyterMessage = request.into();
        let msg_id = message.header.msg_id.clone();
        self.shell
            .send(message)
            .await
            .context("Failed to send complete request")?;

        // Read shell replies until we get our complete_reply (with timeout)
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                anyhow::bail!("Timeout waiting for complete_reply");
            }

            match tokio::time::timeout(remaining, self.shell.read()).await {
                Ok(Ok(reply)) => {
                    // Check if this is our complete_reply
                    let is_ours = reply
                        .parent_header
                        .as_ref()
                        .map(|h| h.msg_id == msg_id)
                        .unwrap_or(false);

                    if is_ours {
                        if let JupyterMessageContent::CompleteReply(complete_reply) = reply.content
                        {
                            return Ok(complete_reply);
                        }
                    }
                    // Not our reply (e.g., an execute_reply); discard and keep reading
                }
                Ok(Err(e)) => {
                    anyhow::bail!("Shell read error during completion: {}", e);
                }
                Err(_) => {
                    anyhow::bail!("Timeout waiting for complete_reply");
                }
            }
        }
    }
}
