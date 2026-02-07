use crate::app::App;
use crate::notebook::model::{CellType, ExecutionState};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Width of the relative line number gutter (digits + padding).
const LINE_NUMBER_WIDTH: u16 = 4;

/// Render the scrollable list of cells.
pub fn render_cell_list(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.notebook.cells.is_empty() {
        return;
    }

    // Ensure selected cell is visible by adjusting scroll offset
    if app.selected_cell < app.scroll_offset {
        app.scroll_offset = app.selected_cell;
    }

    // Render cells from scroll_offset until we run out of vertical space
    let mut y = area.y;
    let mut cells_rendered = 0;

    for idx in app.scroll_offset..app.notebook.cells.len() {
        if y >= area.y + area.height {
            break;
        }

        let is_selected = idx == app.selected_cell;
        let is_editing = app.mode.is_in_cell() && is_selected;
        let cell_number = idx + 1; // 1-indexed for display

        let cell = &app.notebook.cells[idx];

        // Calculate cell height: source lines + output lines + borders
        let source_lines = if is_editing {
            // When editing, use the editor's line count
            app.editor
                .as_ref()
                .map(|e| e.lines().len())
                .unwrap_or(1)
                .max(1)
        } else {
            cell.source.lines().count().max(1)
        };

        let output_lines: usize = cell
            .outputs
            .iter()
            .map(|o| match o {
                crate::notebook::model::CellOutput::Stream { text, .. } => {
                    text.lines().count().max(1)
                }
                crate::notebook::model::CellOutput::ExecuteResult { data, .. } => data
                    .get("text/plain")
                    .map(|t| t.lines().count())
                    .unwrap_or(1),
                crate::notebook::model::CellOutput::Error { traceback, .. } => {
                    traceback.len().max(1)
                }
                crate::notebook::model::CellOutput::DisplayData { data } => data
                    .get("text/plain")
                    .map(|t| t.lines().count())
                    .unwrap_or(1),
            })
            .sum();

        let output_section_height = if output_lines > 0 {
            output_lines + 1
        } else {
            0
        }; // +1 for separator
        let cell_height = (source_lines + output_section_height + 2) as u16; // +2 for borders
        let cell_height = cell_height.min(area.y + area.height - y); // Clamp to available space

        let cell_area = Rect::new(area.x, y, area.width, cell_height);

        render_cell(
            frame,
            app,
            idx,
            cell_number,
            is_selected,
            is_editing,
            cell_area,
        );

        y += cell_height;
        cells_rendered += 1;
    }

    // Adjust scroll if selected cell was pushed below visible area
    if cells_rendered > 0 && app.selected_cell >= app.scroll_offset + cells_rendered {
        app.scroll_offset = app
            .selected_cell
            .saturating_sub(cells_rendered.saturating_sub(1));
    }
}

/// Render a single cell.
fn render_cell(
    frame: &mut Frame,
    app: &mut App,
    cell_idx: usize,
    cell_number: usize,
    is_selected: bool,
    is_editing: bool,
    area: Rect,
) {
    let cell = &app.notebook.cells[cell_idx];

    // Cell type indicator and execution count
    let type_indicator = match cell.cell_type {
        CellType::Code => {
            let exec_count = match (&cell.execution_state, cell.execution_count) {
                (ExecutionState::Running, _) => "*".to_string(),
                (_, Some(n)) => n.to_string(),
                (_, None) => " ".to_string(),
            };
            format!("[{}] In [{}]", cell_number, exec_count)
        }
        CellType::Markdown => format!("[{}] Md", cell_number),
        CellType::Raw => format!("[{}] Raw", cell_number),
    };

    // Border styling based on mode
    let border_style = if is_editing {
        match &app.mode {
            crate::app::Mode::CellInsert => Style::default().fg(Color::Green),
            crate::app::Mode::CellVisual => Style::default().fg(Color::Magenta),
            _ => Style::default().fg(Color::Yellow), // CellNormal
        }
    } else if is_selected {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let exec_state_indicator = match cell.execution_state {
        ExecutionState::Running => " [running]",
        ExecutionState::Error => " [error]",
        _ => "",
    };

    let has_output = !cell.outputs.is_empty();

    let source_lines_count = if is_editing {
        app.editor
            .as_ref()
            .map(|e| e.lines().len())
            .unwrap_or(1)
            .max(1) as u16
    } else {
        cell.source.lines().count().max(1) as u16
    };

    // Collect output data we need before dropping the borrow on cell
    let cell_type = cell.cell_type.clone();
    let cell_source = cell.source.clone();
    let outputs_clone = if has_output {
        Some(cell.outputs.clone())
    } else {
        None
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!("{}{}", type_indicator, exec_state_indicator));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if has_output {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(source_lines_count),
                Constraint::Length(1), // separator
                Constraint::Min(1),    // output
            ])
            .split(inner);

        // Render source (or editor)
        if is_editing {
            render_editor_with_line_numbers(frame, app, chunks[0]);
        } else {
            render_source_direct(frame, &cell_source, &cell_type, chunks[0]);
        }

        // Separator
        let sep = Paragraph::new(Line::from("\u{2500}".repeat(chunks[1].width as usize)))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep, chunks[1]);

        // Render output
        if let Some(outputs) = &outputs_clone {
            render_outputs(frame, outputs, chunks[2]);
        }
    } else if is_editing {
        render_editor_with_line_numbers(frame, app, inner);
    } else {
        render_source_direct(frame, &cell_source, &cell_type, inner);
    }
}

/// Render the TextArea editor with a relative line number gutter.
fn render_editor_with_line_numbers(frame: &mut Frame, app: &App, area: Rect) {
    if area.width <= LINE_NUMBER_WIDTH + 1 {
        // Not enough space for gutter, just render editor
        if let Some(editor) = &app.editor {
            frame.render_widget(editor, area);
        }
        return;
    }

    // Split area into [line_numbers | editor]
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(LINE_NUMBER_WIDTH), Constraint::Min(1)])
        .split(area);

    // Render the relative line number gutter
    if let Some(editor) = &app.editor {
        let (cursor_row, _) = editor.cursor();
        let total_lines = editor.lines().len();

        let mut gutter_lines: Vec<Line> = Vec::new();

        for row in 0..area.height as usize {
            let line_idx = row; // Offset by textarea scroll would be ideal, but textarea doesn't expose viewport offset directly. Lines are 0-indexed.

            if line_idx >= total_lines {
                // Past end of file: render tilde
                gutter_lines.push(Line::from(Span::styled(
                    format!("{:>width$}", "~", width = LINE_NUMBER_WIDTH as usize - 1),
                    Style::default().fg(Color::DarkGray),
                )));
            } else if line_idx == cursor_row {
                // Current line: show absolute line number (1-indexed)
                gutter_lines.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        cursor_row + 1,
                        width = LINE_NUMBER_WIDTH as usize - 1
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
            } else {
                // Other lines: show relative distance
                let distance = if line_idx > cursor_row {
                    line_idx - cursor_row
                } else {
                    cursor_row - line_idx
                };
                gutter_lines.push(Line::from(Span::styled(
                    format!(
                        "{:>width$}",
                        distance,
                        width = LINE_NUMBER_WIDTH as usize - 1
                    ),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        let gutter = Paragraph::new(Text::from(gutter_lines));
        frame.render_widget(gutter, chunks[0]);

        // Render the textarea in the remaining space
        frame.render_widget(editor, chunks[1]);
    }
}

/// Render the source code of a cell (non-editing mode) from pre-extracted data.
fn render_source_direct(frame: &mut Frame, source: &str, cell_type: &CellType, area: Rect) {
    let source = if source.is_empty() {
        " ".to_string()
    } else {
        source.to_string()
    };

    let style = match cell_type {
        CellType::Code => Style::default().fg(Color::White),
        CellType::Markdown => Style::default().fg(Color::Yellow),
        CellType::Raw => Style::default().fg(Color::Gray),
    };

    let paragraph = Paragraph::new(Text::styled(source, style)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Render the output(s) of a cell from pre-extracted output data.
fn render_outputs(frame: &mut Frame, outputs: &[crate::notebook::model::CellOutput], area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for output in outputs {
        match output {
            crate::notebook::model::CellOutput::Stream { name, text } => {
                let style = if name == "stderr" {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::White)
                };
                for line in text.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), style)));
                }
            }
            crate::notebook::model::CellOutput::ExecuteResult { data, .. } => {
                if let Some(text) = data.get("text/plain") {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Green),
                        )));
                    }
                }
            }
            crate::notebook::model::CellOutput::Error { traceback, .. } => {
                for line in traceback {
                    // Tracebacks contain ANSI escape codes -- for now just display raw
                    lines.push(Line::from(Span::styled(
                        line.clone(),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
            crate::notebook::model::CellOutput::DisplayData { data } => {
                if let Some(text) = data.get("text/plain") {
                    for line in text.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Magenta),
                        )));
                    }
                }
                if data.contains_key("image/png") {
                    lines.push(Line::from(Span::styled(
                        "[Image: PNG - Kitty graphics rendering TODO]",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }
        }
    }

    if !lines.is_empty() {
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}
