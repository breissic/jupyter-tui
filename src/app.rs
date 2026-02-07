use crate::event::AppEvent;
use crate::input::handler;
use crate::kernel::client::{KernelClient, KernelMessage};
use crate::kernel::manager::KernelManager;
use crate::notebook::model::{Cell, CellOutput, CellType, ExecutionState, Notebook};
use crate::ui;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jupyter_protocol::JupyterMessageContent;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

/// Vim-style mode for the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// Navigate between cells, cell-level operations
    Normal,
    /// Editing inside a cell with vim keybindings
    Insert,
    /// Visual selection (future use)
    Visual,
    /// Command line input (:w, :q, :3c, etc.)
    Command,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Normal => write!(f, "NORMAL"),
            Mode::Insert => write!(f, "INSERT"),
            Mode::Visual => write!(f, "VISUAL"),
            Mode::Command => write!(f, "COMMAND"),
        }
    }
}

/// The main application state.
pub struct App {
    pub mode: Mode,
    pub notebook: Notebook,
    pub selected_cell: usize,
    pub scroll_offset: usize,
    pub command_buffer: String,
    pub status_message: String,
    pub kernel_status: String,
    pub should_quit: bool,

    /// Active text editor for the selected cell (only Some when in Insert mode)
    pub editor: Option<TextArea<'static>>,

    // Kernel communication
    kernel_manager: KernelManager,
    kernel_client: KernelClient,
}

impl App {
    /// Initialize the application: start kernel, connect, load/create notebook.
    pub async fn new(
        file_path: Option<&str>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<KernelMessage>)> {
        // Load or create notebook
        let notebook = if let Some(path) = file_path {
            let path = std::path::Path::new(path);
            if path.exists() {
                Notebook::load(path).context("Failed to load notebook")?
            } else {
                let mut nb = Notebook::new();
                nb.file_path = Some(path.to_path_buf());
                nb
            }
        } else {
            Notebook::new()
        };

        // Determine kernel name from notebook metadata
        let kernel_name = notebook
            .metadata
            .kernel_name
            .as_deref()
            .unwrap_or("python3");

        // Start kernel
        let kernel_manager = KernelManager::start(Some(kernel_name))
            .await
            .context("Failed to start kernel")?;

        // Connect to kernel
        let (kernel_client, kernel_rx) = KernelClient::connect(kernel_manager.connection_info())
            .await
            .context("Failed to connect to kernel")?;

        let app = Self {
            mode: Mode::Normal,
            notebook,
            selected_cell: 0,
            scroll_offset: 0,
            command_buffer: String::new(),
            status_message: String::from("Kernel starting..."),
            kernel_status: String::from("starting"),
            should_quit: false,
            editor: None,
            kernel_manager,
            kernel_client,
        };

        Ok((app, kernel_rx))
    }

    /// Handle an incoming application event.
    pub async fn handle_event(&mut self, event: AppEvent) -> Result<()> {
        match event {
            AppEvent::Key(key) => self.handle_key(key).await?,
            AppEvent::Kernel(msg) => self.handle_kernel_message(msg),
            AppEvent::Resize(_, _) => {} // ratatui handles this
            AppEvent::Tick => {}
        }
        Ok(())
    }

    /// Route a key event to the appropriate handler based on current mode.
    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Ctrl+C always interrupts kernel or quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if self.mode == Mode::Insert {
                self.exit_insert_mode();
            } else {
                let _ = self.kernel_client.interrupt().await;
                self.status_message = "Interrupt sent to kernel".to_string();
            }
            return Ok(());
        }

        match &self.mode {
            Mode::Normal => handler::handle_normal_mode(self, key).await?,
            Mode::Insert => handler::handle_insert_mode(self, key),
            Mode::Command => handler::handle_command_mode(self, key).await?,
            Mode::Visual => {} // TODO
        }

        Ok(())
    }

    /// Process a message from the kernel's IOPub channel.
    fn handle_kernel_message(&mut self, msg: KernelMessage) {
        match msg {
            KernelMessage::IoPub(jupyter_msg) => {
                match &jupyter_msg.content {
                    JupyterMessageContent::Status(status) => {
                        self.kernel_status = format!("{:?}", status.execution_state).to_lowercase();
                        if self.status_message == "Kernel starting..." {
                            self.status_message = String::new();
                        }
                    }
                    JupyterMessageContent::StreamContent(stream) => {
                        // Find the cell that triggered this output
                        if let Some(cell) = self.find_running_cell() {
                            let stream_name = format!("{:?}", stream.name).to_lowercase();
                            // Append to existing stream output if same name, else new entry
                            let appended = cell.outputs.iter_mut().any(|o| {
                                if let CellOutput::Stream { name, text } = o {
                                    if *name == stream_name {
                                        text.push_str(&stream.text);
                                        return true;
                                    }
                                }
                                false
                            });
                            if !appended {
                                cell.outputs.push(CellOutput::Stream {
                                    name: stream_name,
                                    text: stream.text.clone(),
                                });
                            }
                        }
                    }
                    JupyterMessageContent::ExecuteResult(result) => {
                        if let Some(cell) = self.find_running_cell() {
                            let mut data = std::collections::HashMap::new();
                            for mt in &result.data.content {
                                let (mime, val) =
                                    crate::notebook::model::media_type_to_pair_pub(mt);
                                data.insert(mime, val);
                            }
                            cell.outputs.push(CellOutput::ExecuteResult {
                                execution_count: result.execution_count.value(),
                                data,
                            });
                            cell.execution_count = Some(result.execution_count.value());
                            cell.execution_state = ExecutionState::Done;
                        }
                    }
                    JupyterMessageContent::ErrorOutput(error) => {
                        if let Some(cell) = self.find_running_cell() {
                            cell.outputs.push(CellOutput::Error {
                                ename: error.ename.clone(),
                                evalue: error.evalue.clone(),
                                traceback: error.traceback.clone(),
                            });
                            cell.execution_state = ExecutionState::Error;
                        }
                    }
                    JupyterMessageContent::DisplayData(display) => {
                        if let Some(cell) = self.find_running_cell() {
                            let mut data = std::collections::HashMap::new();
                            for mt in &display.data.content {
                                let (mime, val) =
                                    crate::notebook::model::media_type_to_pair_pub(mt);
                                data.insert(mime, val);
                            }
                            cell.outputs.push(CellOutput::DisplayData { data });
                        }
                    }
                    JupyterMessageContent::ExecuteInput(_) => {
                        // Kernel echoes the input -- we can ignore this
                    }
                    _ => {}
                }
            }
            KernelMessage::ShellReply(_) => {}
            KernelMessage::IoPubError(e) => {
                self.status_message = format!("IOPub error: {}", e);
            }
        }
    }

    /// Find the first cell in the Running state (the one currently executing).
    fn find_running_cell(&mut self) -> Option<&mut Cell> {
        self.notebook
            .cells
            .iter_mut()
            .find(|c| c.execution_state == ExecutionState::Running)
    }

    /// Execute the currently selected cell.
    pub async fn execute_selected_cell(&mut self) -> Result<()> {
        // If we're in insert mode, sync the editor content first
        self.sync_editor_to_cell();

        let cell = &mut self.notebook.cells[self.selected_cell];
        if cell.cell_type != CellType::Code {
            self.status_message = "Can only execute code cells".to_string();
            return Ok(());
        }

        let code = cell.source.clone();
        cell.clear_outputs();
        cell.execution_state = ExecutionState::Running;

        self.kernel_client.execute(&code).await?;

        Ok(())
    }

    /// Enter insert mode: create a TextArea from the current cell's source.
    pub fn enter_insert_mode(&mut self) {
        let cell = &self.notebook.cells[self.selected_cell];
        let lines: Vec<String> = if cell.source.is_empty() {
            vec![String::new()]
        } else {
            cell.source.lines().map(|l| l.to_string()).collect()
        };
        let mut textarea = TextArea::new(lines);

        // Style the editor
        use ratatui::style::{Color, Style};
        textarea.set_cursor_line_style(Style::default());
        textarea.set_cursor_style(Style::default().fg(Color::Reset).bg(Color::White));

        // Show line numbers
        textarea.set_line_number_style(Style::default().fg(Color::DarkGray));

        self.editor = Some(textarea);
        self.mode = Mode::Insert;
        self.status_message = String::new();
    }

    /// Exit insert mode: sync TextArea content back to the cell.
    pub fn exit_insert_mode(&mut self) {
        self.sync_editor_to_cell();
        self.editor = None;
        self.mode = Mode::Normal;
        self.status_message = String::new();
    }

    /// Sync the current editor content back to the selected cell's source.
    fn sync_editor_to_cell(&mut self) {
        if let Some(editor) = &self.editor {
            let lines = editor.lines();
            let source = lines.join("\n");
            self.notebook.cells[self.selected_cell].source = source;
            self.notebook.dirty = true;
        }
    }

    /// Draw the UI.
    pub fn draw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.draw(|frame| {
            ui::layout::render(frame, self);
        })?;
        Ok(())
    }

    /// Graceful shutdown.
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.kernel_client.shutdown(false).await;
        self.kernel_manager.shutdown().await?;
        Ok(())
    }
}
