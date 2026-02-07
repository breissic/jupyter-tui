use anyhow::{Context, Result};
use jupyter_protocol::{
    ConnectionInfo, ExecuteRequest, JupyterMessage, KernelInfoRequest, ShutdownRequest,
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
    pub async fn execute(&mut self, code: &str) -> Result<()> {
        let request = ExecuteRequest::new(code.to_string());
        let message: JupyterMessage = request.into();
        self.shell
            .send(message)
            .await
            .context("Failed to send execute request")?;
        Ok(())
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
}
