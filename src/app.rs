use crate::event::AppEvent;
use crate::input::handler;
use crate::input::vim::CellVim;
use crate::kernel::client::{KernelClient, KernelMessage};
use crate::kernel::manager::KernelManager;
use crate::notebook::model::{CellOutput, CellType, ExecutionState, Notebook};
use crate::ui;
use crate::ui::highlight::Highlighter;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jupyter_protocol::JupyterMessageContent;
use ratatui::DefaultTerminal;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tui_textarea::TextArea;

/// Direction for search (/ = forward, ? = backward).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// Vim-style mode for the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// Navigate between cells, cell-level operations
    Normal,
    /// Inside a cell, vim Normal mode (hjkl, motions, operators)
    CellNormal,
    /// Inside a cell, typing text
    CellInsert,
    /// Inside a cell, visual selection
    CellVisual,
    /// Command line input (:w, :q, :3c, :3, etc.)
    Command,
    /// Search input (/ or ?)
    Search,
}

impl Mode {
    /// Whether the mode is inside a cell (editor is active).
    pub fn is_in_cell(&self) -> bool {
        matches!(self, Mode::CellNormal | Mode::CellInsert | Mode::CellVisual)
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Normal => write!(f, "NORMAL"),
            Mode::CellNormal => write!(f, "CELL:NORMAL"),
            Mode::CellInsert => write!(f, "CELL:INSERT"),
            Mode::CellVisual => write!(f, "CELL:VISUAL"),
            Mode::Command => write!(f, "COMMAND"),
            Mode::Search => write!(f, "SEARCH"),
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

    /// Active text editor for the selected cell (only Some when in a cell mode)
    pub editor: Option<TextArea<'static>>,

    /// Vim state machine for in-cell editing (pending input, operator state)
    pub cell_vim: CellVim,

    /// Count prefix accumulator for Normal mode (cell-level navigation)
    pub normal_count: Option<usize>,

    /// Search direction (/ = Forward, ? = Backward)
    pub search_direction: SearchDirection,

    /// Buffer for search input while typing in Search mode
    pub search_buffer: String,

    /// Last search pattern (for n/N repeat)
    pub last_search: Option<String>,

    /// Whether search was initiated from inside a cell (to know where to return)
    pub search_from_cell: bool,

    /// Syntax highlighter (syntect-based) for code cells
    pub highlighter: Highlighter,

    /// Maps kernel execute_request msg_id -> cell index for correlating IOPub responses
    executing_cells: HashMap<String, usize>,

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

        let mut app = Self {
            mode: Mode::Normal,
            notebook,
            selected_cell: 0,
            scroll_offset: 0,
            command_buffer: String::new(),
            status_message: String::from("Kernel starting..."),
            kernel_status: String::from("starting"),
            should_quit: false,
            editor: None,
            cell_vim: CellVim::new(),
            normal_count: None,
            search_direction: SearchDirection::Forward,
            search_buffer: String::new(),
            last_search: None,
            search_from_cell: false,
            highlighter: Highlighter::new(),
            executing_cells: HashMap::new(),
            kernel_manager,
            kernel_client,
        };

        // Send kernel_info_request to trigger a status: idle message on IOPub,
        // so the status bar updates once the kernel is actually ready.
        let _ = app.kernel_client.request_kernel_info().await;

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
        // Ctrl+C always interrupts kernel or exits cell
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if self.mode.is_in_cell() {
                self.exit_cell();
            } else {
                let _ = self.kernel_client.interrupt().await;
                self.status_message = "Interrupt sent to kernel".to_string();
            }
            return Ok(());
        }

        match &self.mode {
            Mode::Normal => handler::handle_normal_mode(self, key).await?,
            Mode::CellNormal => handler::handle_cell_normal_mode(self, key).await?,
            Mode::CellInsert => handler::handle_cell_insert_mode(self, key),
            Mode::CellVisual => handler::handle_cell_visual_mode(self, key),
            Mode::Command => handler::handle_command_mode(self, key).await?,
            Mode::Search => handler::handle_search_mode(self, key),
        }

        Ok(())
    }

    /// Process a message from the kernel's IOPub channel.
    fn handle_kernel_message(&mut self, msg: KernelMessage) {
        match msg {
            KernelMessage::IoPub(jupyter_msg) => {
                // Extract the parent msg_id to correlate with executing cells
                let parent_msg_id = jupyter_msg
                    .parent_header
                    .as_ref()
                    .map(|h| h.msg_id.as_str());

                match &jupyter_msg.content {
                    JupyterMessageContent::Status(status) => {
                        self.kernel_status = format!("{:?}", status.execution_state).to_lowercase();
                        if self.status_message == "Kernel starting..." {
                            self.status_message = String::new();
                        }

                        // When idle arrives with a matching parent_header, mark cell as Done
                        if self.kernel_status == "idle" {
                            if let Some(msg_id) = parent_msg_id {
                                if let Some(cell_idx) = self.executing_cells.remove(msg_id) {
                                    if cell_idx < self.notebook.cells.len() {
                                        let cell = &mut self.notebook.cells[cell_idx];
                                        // Only set Done if not already Error
                                        if cell.execution_state == ExecutionState::Running {
                                            cell.execution_state = ExecutionState::Done;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    JupyterMessageContent::StreamContent(stream) => {
                        if let Some(cell_idx) =
                            parent_msg_id.and_then(|id| self.executing_cells.get(id).copied())
                        {
                            if cell_idx < self.notebook.cells.len() {
                                let cell = &mut self.notebook.cells[cell_idx];
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
                    }
                    JupyterMessageContent::ExecuteResult(result) => {
                        if let Some(cell_idx) =
                            parent_msg_id.and_then(|id| self.executing_cells.get(id).copied())
                        {
                            if cell_idx < self.notebook.cells.len() {
                                let cell = &mut self.notebook.cells[cell_idx];
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
                            }
                        }
                    }
                    JupyterMessageContent::ErrorOutput(error) => {
                        if let Some(cell_idx) =
                            parent_msg_id.and_then(|id| self.executing_cells.get(id).copied())
                        {
                            if cell_idx < self.notebook.cells.len() {
                                let cell = &mut self.notebook.cells[cell_idx];
                                cell.outputs.push(CellOutput::Error {
                                    ename: error.ename.clone(),
                                    evalue: error.evalue.clone(),
                                    traceback: error.traceback.clone(),
                                });
                                cell.execution_state = ExecutionState::Error;
                            }
                        }
                    }
                    JupyterMessageContent::DisplayData(display) => {
                        if let Some(cell_idx) =
                            parent_msg_id.and_then(|id| self.executing_cells.get(id).copied())
                        {
                            if cell_idx < self.notebook.cells.len() {
                                let cell = &mut self.notebook.cells[cell_idx];
                                let mut data = std::collections::HashMap::new();
                                for mt in &display.data.content {
                                    let (mime, val) =
                                        crate::notebook::model::media_type_to_pair_pub(mt);
                                    data.insert(mime, val);
                                }
                                cell.outputs.push(CellOutput::DisplayData { data });
                            }
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

        let msg_id = self.kernel_client.execute(&code).await?;
        self.executing_cells.insert(msg_id, self.selected_cell);

        Ok(())
    }

    /// Enter the cell in CellNormal mode: create a TextArea from the current cell's source.
    pub fn enter_cell(&mut self) {
        let cell = &self.notebook.cells[self.selected_cell];
        let lines: Vec<String> = if cell.source.is_empty() {
            vec![String::new()]
        } else {
            cell.source.lines().map(|l| l.to_string()).collect()
        };
        let mut textarea = TextArea::new(lines);

        // Style for CellNormal mode -- block cursor
        use ratatui::style::{Color, Modifier, Style};
        textarea.set_cursor_line_style(Style::default());
        textarea.set_cursor_style(
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::REVERSED),
        );

        // Disable built-in line numbers (we render relative ones ourselves)
        // Note: not calling set_line_number_style means no line numbers from textarea

        self.editor = Some(textarea);
        self.cell_vim = CellVim::new();
        self.mode = Mode::CellNormal;
        self.status_message = String::new();
    }

    /// Enter CellInsert mode from CellNormal (editor already exists).
    pub fn enter_cell_insert(&mut self) {
        use ratatui::style::{Color, Style};
        if let Some(editor) = &mut self.editor {
            editor.set_cursor_style(Style::default().fg(Color::Reset).bg(Color::White));
        }
        self.mode = Mode::CellInsert;
        self.status_message = String::new();
    }

    /// Enter CellVisual mode from CellNormal.
    pub fn enter_cell_visual(&mut self) {
        use ratatui::style::{Color, Modifier, Style};
        if let Some(editor) = &mut self.editor {
            editor.set_cursor_style(
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::REVERSED),
            );
            editor.start_selection();
        }
        self.mode = Mode::CellVisual;
        self.status_message = String::new();
    }

    /// Return to CellNormal mode from CellInsert or CellVisual.
    pub fn return_to_cell_normal(&mut self) {
        use ratatui::style::{Color, Modifier, Style};
        if let Some(editor) = &mut self.editor {
            editor.set_cursor_style(
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::REVERSED),
            );
            editor.cancel_selection();
        }
        self.cell_vim = CellVim::new();
        self.mode = Mode::CellNormal;
        self.status_message = String::new();
    }

    /// Exit the cell entirely: sync TextArea content back and return to Normal mode.
    pub fn exit_cell(&mut self) {
        self.sync_editor_to_cell();
        self.editor = None;
        self.cell_vim = CellVim::new();
        self.mode = Mode::Normal;
        self.status_message = String::new();
    }

    /// Sync the current editor content back to the selected cell's source.
    pub fn sync_editor_to_cell(&mut self) {
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
