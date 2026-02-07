use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Runtime representation of a notebook cell.
#[derive(Debug, Clone)]
pub struct Cell {
    pub id: String,
    pub cell_type: CellType,
    pub source: String,
    pub outputs: Vec<CellOutput>,
    pub execution_count: Option<usize>,
    pub execution_state: ExecutionState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CellType {
    Code,
    Markdown,
    Raw,
}

impl std::fmt::Display for CellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CellType::Code => write!(f, "Code"),
            CellType::Markdown => write!(f, "Markdown"),
            CellType::Raw => write!(f, "Raw"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionState {
    Idle,
    Running,
    Done,
    Error,
}

/// A single output from a cell execution.
#[derive(Debug, Clone)]
pub enum CellOutput {
    /// stdout/stderr stream
    Stream { name: String, text: String },
    /// Rich display data (can contain text/plain, image/png, etc.)
    DisplayData {
        data: std::collections::HashMap<String, String>,
    },
    /// The return value of an expression
    ExecuteResult {
        execution_count: usize,
        data: std::collections::HashMap<String, String>,
    },
    /// Error traceback
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

/// Runtime notebook model.
///
/// Wraps nbformat types but provides a simpler interface
/// for the TUI to work with.
pub struct Notebook {
    pub cells: Vec<Cell>,
    pub metadata: NotebookMetadata,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub struct NotebookMetadata {
    pub kernel_name: Option<String>,
    pub language: Option<String>,
}

impl Cell {
    pub fn new_code(source: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cell_type: CellType::Code,
            source: source.to_string(),
            outputs: Vec::new(),
            execution_count: None,
            execution_state: ExecutionState::Idle,
        }
    }

    pub fn new_markdown(source: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cell_type: CellType::Markdown,
            source: source.to_string(),
            outputs: Vec::new(),
            execution_count: None,
            execution_state: ExecutionState::Idle,
        }
    }

    pub fn new_raw(source: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            cell_type: CellType::Raw,
            source: source.to_string(),
            outputs: Vec::new(),
            execution_count: None,
            execution_state: ExecutionState::Idle,
        }
    }

    /// Clear outputs and reset execution state.
    pub fn clear_outputs(&mut self) {
        self.outputs.clear();
        self.execution_state = ExecutionState::Idle;
    }
}

impl Notebook {
    /// Create a new empty notebook with a single code cell.
    pub fn new() -> Self {
        Self {
            cells: vec![Cell::new_code("")],
            metadata: NotebookMetadata {
                kernel_name: Some("python3".to_string()),
                language: Some("python".to_string()),
            },
            file_path: None,
            dirty: false,
        }
    }

    /// Load a notebook from an .ipynb file.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).context("Failed to read notebook file")?;

        let nb = nbformat::parse_notebook(&contents).context("Failed to parse notebook")?;

        let (cells, metadata) = match nb {
            nbformat::Notebook::V4(v4) => {
                let cells = v4.cells.into_iter().map(convert_v4_cell).collect();
                let metadata = NotebookMetadata {
                    kernel_name: v4.metadata.kernelspec.as_ref().map(|k| k.name.clone()),
                    language: v4.metadata.language_info.as_ref().map(|l| l.name.clone()),
                };
                (cells, metadata)
            }
            nbformat::Notebook::Legacy(legacy) => {
                // Upgrade legacy notebooks to v4
                let v4 = nbformat::upgrade_legacy_notebook(legacy)
                    .context("Failed to upgrade legacy notebook")?;
                let cells = v4.cells.into_iter().map(convert_v4_cell).collect();
                let metadata = NotebookMetadata {
                    kernel_name: v4.metadata.kernelspec.as_ref().map(|k| k.name.clone()),
                    language: v4.metadata.language_info.as_ref().map(|l| l.name.clone()),
                };
                (cells, metadata)
            }
        };

        Ok(Self {
            cells,
            metadata,
            file_path: Some(path.to_path_buf()),
            dirty: false,
        })
    }

    /// Save the notebook to its file path (or a new path).
    pub fn save(&mut self, path: Option<&Path>) -> Result<()> {
        let save_path = path
            .or(self.file_path.as_deref())
            .context("No file path specified for save")?;

        let v4_notebook = self.to_v4();
        let nb = nbformat::Notebook::V4(v4_notebook);
        let json = nbformat::serialize_notebook(&nb).context("Failed to serialize notebook")?;

        std::fs::write(save_path, &json).context("Failed to write notebook file")?;

        if path.is_some() {
            self.file_path = path.map(|p| p.to_path_buf());
        }
        self.dirty = false;

        Ok(())
    }

    /// Convert our runtime model back to nbformat v4.
    fn to_v4(&self) -> nbformat::v4::Notebook {
        let cells = self.cells.iter().map(convert_to_v4_cell).collect();

        nbformat::v4::Notebook {
            cells,
            metadata: nbformat::v4::Metadata {
                kernelspec: self.metadata.kernel_name.as_ref().map(|name| {
                    nbformat::v4::KernelSpec {
                        display_name: name.clone(),
                        name: name.clone(),
                        language: self.metadata.language.clone(),
                        additional: Default::default(),
                    }
                }),
                language_info: self.metadata.language.as_ref().map(|lang| {
                    nbformat::v4::LanguageInfo {
                        name: lang.clone(),
                        version: None,
                        codemirror_mode: None,
                        additional: Default::default(),
                    }
                }),
                authors: None,
                additional: Default::default(),
            },
            nbformat: 4,
            nbformat_minor: 5,
        }
    }

    /// Insert a new cell after the given index.
    pub fn insert_cell_after(&mut self, index: usize, cell: Cell) {
        let insert_at = (index + 1).min(self.cells.len());
        self.cells.insert(insert_at, cell);
        self.dirty = true;
    }

    /// Insert a new cell before the given index.
    pub fn insert_cell_before(&mut self, index: usize, cell: Cell) {
        let insert_at = index.min(self.cells.len());
        self.cells.insert(insert_at, cell);
        self.dirty = true;
    }

    /// Delete the cell at the given index. Ensures at least one cell remains.
    pub fn delete_cell(&mut self, index: usize) -> Option<Cell> {
        if self.cells.len() <= 1 {
            return None;
        }
        if index < self.cells.len() {
            self.dirty = true;
            Some(self.cells.remove(index))
        } else {
            None
        }
    }
}

/// Convert an nbformat v4 Cell to our runtime Cell.
fn convert_v4_cell(cell: nbformat::v4::Cell) -> Cell {
    match cell {
        nbformat::v4::Cell::Code {
            id,
            source,
            outputs,
            execution_count,
            ..
        } => Cell {
            id: id.to_string(),
            cell_type: CellType::Code,
            source: source.join(""),
            outputs: outputs.into_iter().map(convert_v4_output).collect(),
            execution_count: execution_count.map(|n| n as usize),
            execution_state: ExecutionState::Idle,
        },
        nbformat::v4::Cell::Markdown { id, source, .. } => Cell {
            id: id.to_string(),
            cell_type: CellType::Markdown,
            source: source.join(""),
            outputs: Vec::new(),
            execution_count: None,
            execution_state: ExecutionState::Idle,
        },
        nbformat::v4::Cell::Raw { id, source, .. } => Cell {
            id: id.to_string(),
            cell_type: CellType::Raw,
            source: source.join(""),
            outputs: Vec::new(),
            execution_count: None,
            execution_state: ExecutionState::Idle,
        },
    }
}

/// Convert an nbformat v4 Output to our runtime CellOutput.
fn convert_v4_output(output: nbformat::v4::Output) -> CellOutput {
    match output {
        nbformat::v4::Output::Stream { name, text } => CellOutput::Stream { name, text: text.0 },
        nbformat::v4::Output::DisplayData(dd) => CellOutput::DisplayData {
            data: media_to_hashmap(&dd.data),
        },
        nbformat::v4::Output::ExecuteResult(er) => CellOutput::ExecuteResult {
            execution_count: er.execution_count.value(),
            data: media_to_hashmap(&er.data),
        },
        nbformat::v4::Output::Error(e) => CellOutput::Error {
            ename: e.ename,
            evalue: e.evalue,
            traceback: e.traceback,
        },
    }
}

/// Convert a jupyter_protocol Media bundle to a simple HashMap.
fn media_to_hashmap(media: &jupyter_protocol::Media) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for mt in &media.content {
        let (mime, value) = media_type_to_pair(mt);
        map.insert(mime, value);
    }
    map
}

/// Extract MIME type string and content from a MediaType (public for app.rs).
pub fn media_type_to_pair_pub(mt: &jupyter_protocol::MediaType) -> (String, String) {
    media_type_to_pair(mt)
}

/// Extract MIME type string and content from a MediaType.
fn media_type_to_pair(mt: &jupyter_protocol::MediaType) -> (String, String) {
    // MediaType has variants like Plain(String), Html(String), Png(String), etc.
    // We use the Debug representation to extract the MIME type string
    match mt {
        jupyter_protocol::MediaType::Plain(s) => ("text/plain".to_string(), s.clone()),
        jupyter_protocol::MediaType::Html(s) => ("text/html".to_string(), s.clone()),
        jupyter_protocol::MediaType::Latex(s) => ("text/latex".to_string(), s.clone()),
        jupyter_protocol::MediaType::Javascript(s) => {
            ("application/javascript".to_string(), s.clone())
        }
        jupyter_protocol::MediaType::Markdown(s) => ("text/markdown".to_string(), s.clone()),
        jupyter_protocol::MediaType::Svg(s) => ("image/svg+xml".to_string(), s.clone()),
        jupyter_protocol::MediaType::Png(s) => ("image/png".to_string(), s.clone()),
        jupyter_protocol::MediaType::Jpeg(s) => ("image/jpeg".to_string(), s.clone()),
        jupyter_protocol::MediaType::Json(v) => ("application/json".to_string(), v.to_string()),
        _ => ("application/octet-stream".to_string(), String::new()),
    }
}

/// Convert our runtime Cell back to nbformat v4 Cell.
fn convert_to_v4_cell(cell: &Cell) -> nbformat::v4::Cell {
    let source = source_to_lines(&cell.source);
    let id: nbformat::v4::CellId = cell
        .id
        .as_str()
        .try_into()
        .unwrap_or_else(|_| Uuid::new_v4().into());

    let default_metadata = nbformat::v4::CellMetadata {
        id: None,
        collapsed: None,
        scrolled: None,
        deletable: None,
        editable: None,
        format: None,
        name: None,
        tags: None,
        jupyter: None,
        execution: None,
        additional: Default::default(),
    };

    match cell.cell_type {
        CellType::Code => nbformat::v4::Cell::Code {
            id,
            metadata: default_metadata,
            execution_count: cell.execution_count.map(|n| n as i32),
            source,
            outputs: cell.outputs.iter().map(convert_to_v4_output).collect(),
        },
        CellType::Markdown => nbformat::v4::Cell::Markdown {
            id,
            metadata: default_metadata,
            source,
            attachments: None,
        },
        CellType::Raw => nbformat::v4::Cell::Raw {
            id,
            metadata: default_metadata,
            source,
        },
    }
}

/// Convert a source string to the Vec<String> format nbformat expects
/// (lines with trailing newlines).
fn source_to_lines(source: &str) -> Vec<String> {
    if source.is_empty() {
        return vec![];
    }
    let lines: Vec<&str> = source.split('\n').collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i < lines.len() - 1 {
                format!("{}\n", line)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// Convert a CellOutput back to nbformat v4 Output.
fn convert_to_v4_output(output: &CellOutput) -> nbformat::v4::Output {
    match output {
        CellOutput::Stream { name, text } => nbformat::v4::Output::Stream {
            name: name.clone(),
            text: nbformat::v4::MultilineString(text.clone()),
        },
        CellOutput::Error {
            ename,
            evalue,
            traceback,
        } => nbformat::v4::Output::Error(nbformat::v4::ErrorOutput {
            ename: ename.clone(),
            evalue: evalue.clone(),
            traceback: traceback.clone(),
        }),
        CellOutput::DisplayData { data } => {
            nbformat::v4::Output::DisplayData(nbformat::v4::DisplayData {
                data: hashmap_to_media(data),
                metadata: Default::default(),
            })
        }
        CellOutput::ExecuteResult {
            execution_count,
            data,
        } => nbformat::v4::Output::ExecuteResult(nbformat::v4::ExecuteResult {
            execution_count: jupyter_protocol::ExecutionCount::new(*execution_count),
            data: hashmap_to_media(data),
            metadata: Default::default(),
        }),
    }
}

/// Convert a HashMap back to jupyter_protocol Media.
fn hashmap_to_media(data: &std::collections::HashMap<String, String>) -> jupyter_protocol::Media {
    use jupyter_protocol::MediaType;

    let mut content = Vec::new();
    for (mime, value) in data {
        let mt = match mime.as_str() {
            "text/plain" => MediaType::Plain(value.clone()),
            "text/html" => MediaType::Html(value.clone()),
            "text/markdown" => MediaType::Markdown(value.clone()),
            "text/latex" => MediaType::Latex(value.clone()),
            "image/png" => MediaType::Png(value.clone()),
            "image/jpeg" => MediaType::Jpeg(value.clone()),
            "image/svg+xml" => MediaType::Svg(value.clone()),
            "application/javascript" => MediaType::Javascript(value.clone()),
            _ => MediaType::Plain(value.clone()),
        };
        content.push(mt);
    }
    jupyter_protocol::Media { content }
}
