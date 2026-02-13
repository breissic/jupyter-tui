use crate::app::App;
use crate::notebook::model::{CellType, ExecutionState};
use crate::ui::highlight::Highlighter;
use ansi_to_tui::IntoText;
use image::DynamicImage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui_image::StatefulImage;

/// Width of the relative line number gutter (digits + padding).
const LINE_NUMBER_WIDTH: u16 = 4;

/// Maximum height (in terminal rows) for rendered images.
const MAX_IMAGE_HEIGHT: u16 = 20;

/// Decode a base64-encoded image string into a DynamicImage.
fn decode_base64_image(b64: &str) -> Option<DynamicImage> {
    use image::ImageReader;
    use std::io::Cursor;

    // Strip whitespace/newlines that Jupyter sometimes includes
    let cleaned: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64_decode(&cleaned)?;
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}

/// Simple base64 decoder (standard alphabet, with padding).
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: [u8; 128] = {
        let mut t = [255u8; 128];
        let mut i = 0u8;
        while i < 26 {
            t[(b'A' + i) as usize] = i;
            t[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            t[(b'0' + d) as usize] = d + 52;
            d += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t
    };

    let input = input.as_bytes();
    let len = input.len();
    if len % 4 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(len / 4 * 3);
    let mut i = 0;
    while i < len {
        let a = *TABLE.get(*input.get(i)? as usize)?;
        let b = *TABLE.get(*input.get(i + 1)? as usize)?;
        if a == 255 || b == 255 {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        if input[i + 2] != b'=' {
            let c = *TABLE.get(input[i + 2] as usize)?;
            if c == 255 {
                return None;
            }
            out.push((b << 4) | (c >> 2));
            if input[i + 3] != b'=' {
                let d = *TABLE.get(input[i + 3] as usize)?;
                if d == 255 {
                    return None;
                }
                out.push((c << 6) | d);
            }
        }
        i += 4;
    }
    Some(out)
}

/// Calculate the height in terminal rows that an image would occupy,
/// given the picker's font size and available width.
fn image_height_rows(img: &DynamicImage, font_size: (u16, u16), available_width: u16) -> u16 {
    if font_size.0 == 0 || font_size.1 == 0 || available_width == 0 {
        return 1;
    }
    let avail_px_w = (available_width as u32) * (font_size.0 as u32);
    let scale = (avail_px_w as f64) / (img.width() as f64).max(1.0);
    let scale = scale.min(1.0); // don't upscale
    let h_px = (img.height() as f64 * scale).ceil() as u32;
    let rows = (h_px as f64 / font_size.1 as f64).ceil() as u16;
    rows.min(MAX_IMAGE_HEIGHT).max(1)
}

/// Check if an output's data map contains an image we can render.
fn has_renderable_image(data: &std::collections::HashMap<String, String>) -> bool {
    data.contains_key("image/png") || data.contains_key("image/jpeg")
}

/// Get the base64 image data from an output's data map.
fn get_image_data(data: &std::collections::HashMap<String, String>) -> Option<&str> {
    data.get("image/png")
        .or_else(|| data.get("image/jpeg"))
        .map(|s| s.as_str())
}

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
        let is_editing = (app.mode.is_in_cell()
            || (app.mode == crate::app::Mode::Search && app.search_from_cell))
            && is_selected
            && app.editor.is_some();
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
                crate::notebook::model::CellOutput::ExecuteResult { data, .. } => {
                    let text_lines = data
                        .get("text/plain")
                        .map(|t| t.lines().count())
                        .unwrap_or(0);
                    let img_rows = if has_renderable_image(data) {
                        get_image_data(data)
                            .and_then(decode_base64_image)
                            .map(|img| {
                                image_height_rows(
                                    &img,
                                    app.picker.font_size(),
                                    area.width.saturating_sub(2),
                                ) as usize
                            })
                            .unwrap_or(1)
                    } else {
                        0
                    };
                    text_lines + img_rows
                }
                crate::notebook::model::CellOutput::Error { traceback, .. } => {
                    traceback.len().max(1)
                }
                crate::notebook::model::CellOutput::DisplayData { data } => {
                    let text_lines = data
                        .get("text/plain")
                        .map(|t| t.lines().count())
                        .unwrap_or(0);
                    let img_rows = if has_renderable_image(data) {
                        get_image_data(data)
                            .and_then(decode_base64_image)
                            .map(|img| {
                                image_height_rows(
                                    &img,
                                    app.picker.font_size(),
                                    area.width.saturating_sub(2),
                                ) as usize
                            })
                            .unwrap_or(1)
                    } else {
                        0
                    };
                    (text_lines + img_rows).max(if text_lines == 0 && img_rows == 0 {
                        1
                    } else {
                        0
                    })
                }
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

    let language = app
        .notebook
        .metadata
        .language
        .as_deref()
        .unwrap_or("python");

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
            render_source_direct(
                frame,
                &cell_source,
                &cell_type,
                &app.highlighter,
                language,
                chunks[0],
            );
            // Overlay search highlights on non-editing cells
            apply_search_highlights(frame, app, cell_idx, chunks[0]);
        }

        // Separator
        let sep = Paragraph::new(Line::from("\u{2500}".repeat(chunks[1].width as usize)))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(sep, chunks[1]);

        // Render output
        if let Some(outputs) = &outputs_clone {
            render_outputs(frame, app, cell_idx, outputs, chunks[2]);
        }
    } else if is_editing {
        render_editor_with_line_numbers(frame, app, inner);
    } else {
        render_source_direct(
            frame,
            &cell_source,
            &cell_type,
            &app.highlighter,
            language,
            inner,
        );
        // Overlay search highlights on non-editing cells
        apply_search_highlights(frame, app, cell_idx, inner);
    }
}

/// Render the TextArea editor with a relative line number gutter and syntax highlighting.
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

    if let Some(editor) = &app.editor {
        let (cursor_row, _) = editor.cursor();
        let total_lines = editor.lines().len();
        let visible_height = chunks[0].height as usize;

        // Render the textarea first so we can detect scroll offset from the buffer
        let editor_area = chunks[1];
        frame.render_widget(editor, editor_area);

        // Detect actual scroll offset by reading the buffer
        let scroll_top = {
            let buf = frame.buffer_mut();
            detect_scroll_offset(buf, editor_area, editor.lines())
        };

        // --- Render the relative line number gutter ---
        let mut gutter_lines: Vec<Line> = Vec::new();

        for row in 0..visible_height {
            let line_idx = scroll_top + row;

            if line_idx >= total_lines {
                gutter_lines.push(Line::from(Span::styled(
                    format!("{:>width$}", "~", width = LINE_NUMBER_WIDTH as usize - 1),
                    Style::default().fg(Color::DarkGray),
                )));
            } else if line_idx == cursor_row {
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

        // --- Post-process buffer for syntax highlighting ---
        let language = app
            .notebook
            .metadata
            .language
            .as_deref()
            .unwrap_or("python");

        let cell = &app.notebook.cells[app.selected_cell];
        if cell.cell_type == CellType::Code {
            let source = editor.lines().join("\n");
            let highlighted = app.highlighter.highlight_lines(&source, language);

            let buf = frame.buffer_mut();

            for row in 0..editor_area.height as usize {
                let src_line = scroll_top + row;
                if src_line >= highlighted.len() {
                    break;
                }

                let spans = &highlighted[src_line];
                let buf_y = editor_area.y + row as u16;

                // Walk through the highlighted spans and apply colors to buffer cells
                let mut col_offset: u16 = 0;
                for (style, text) in spans {
                    for _ch in text.chars() {
                        let buf_x = editor_area.x + col_offset;
                        if buf_x >= editor_area.x + editor_area.width {
                            break;
                        }

                        let buf_cell = &mut buf[(buf_x, buf_y)];

                        // Only apply syntax highlighting if the cell has default fg and bg
                        // (preserves cursor, selection, search highlights, and other tui-textarea styling)
                        let cell_fg = buf_cell.fg;
                        let cell_bg = buf_cell.bg;
                        if (cell_fg == Color::Reset || cell_fg == Color::White)
                            && cell_bg == Color::Reset
                        {
                            buf_cell.fg = style.fg.unwrap_or(Color::White);
                            if style.add_modifier.contains(Modifier::BOLD) {
                                buf_cell.modifier.insert(Modifier::BOLD);
                            }
                            if style.add_modifier.contains(Modifier::ITALIC) {
                                buf_cell.modifier.insert(Modifier::ITALIC);
                            }
                        }

                        col_offset += 1;
                    }
                }
            }
        }
    }
}

/// Detect the scroll offset of the textarea by reading buffer content and matching
/// against source lines. Returns the index of the first visible source line.
fn detect_scroll_offset(
    buf: &ratatui::buffer::Buffer,
    editor_area: Rect,
    source_lines: &[String],
) -> usize {
    if source_lines.is_empty() || editor_area.height == 0 || editor_area.width == 0 {
        return 0;
    }

    // Read the text of the first buffer row in the editor area
    let mut first_row_text = String::new();
    for x in editor_area.x..editor_area.x + editor_area.width {
        let cell = &buf[(x, editor_area.y)];
        first_row_text.push_str(cell.symbol());
    }
    let first_row_text = first_row_text.trim_end();

    // Find which source line matches (check from beginning, first match wins)
    for (i, line) in source_lines.iter().enumerate() {
        let trimmed_line = if line.len() > editor_area.width as usize {
            &line[..editor_area.width as usize]
        } else {
            line.as_str()
        };
        if first_row_text == trimmed_line {
            return i;
        }
    }

    // Fallback: use the same logic as tui-textarea's next_scroll_top
    // This works because tui-textarea computes scroll_top from the previous
    // scroll_top, cursor position, and viewport height. On the first render,
    // prev_top is 0, so scroll_top = max(0, cursor - height + 1) if cursor >= height.
    0
}

/// Render the source code of a cell (non-editing mode) from pre-extracted data.
fn render_source_direct(
    frame: &mut Frame,
    source: &str,
    cell_type: &CellType,
    highlighter: &Highlighter,
    language: &str,
    area: Rect,
) {
    let source = if source.is_empty() {
        " ".to_string()
    } else {
        source.to_string()
    };

    match cell_type {
        CellType::Code => {
            // Use syntax highlighting for code cells
            let highlighted = highlighter.highlight_lines(&source, language);
            let lines: Vec<Line> = highlighted
                .into_iter()
                .map(|spans| {
                    Line::from(
                        spans
                            .into_iter()
                            .map(|(style, text)| Span::styled(text, style))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let paragraph = Paragraph::new(Text::from(lines));
            frame.render_widget(paragraph, area);
        }
        CellType::Markdown => {
            let style = Style::default().fg(Color::Yellow);
            let paragraph = Paragraph::new(Text::styled(source, style)).wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
        CellType::Raw => {
            let style = Style::default().fg(Color::Gray);
            let paragraph = Paragraph::new(Text::styled(source, style)).wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }
    }
}

/// Post-process the frame buffer to overlay search highlights on a cell's source area.
/// Matches from `app.search_matches` that belong to `cell_idx` get a yellow background.
fn apply_search_highlights(frame: &mut Frame, app: &App, cell_idx: usize, area: Rect) {
    if app.search_matches.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }

    let buf = frame.buffer_mut();

    for &(match_cell, row, col, len) in &app.search_matches {
        if match_cell != cell_idx {
            continue;
        }

        let buf_y = area.y + row as u16;
        if buf_y >= area.y + area.height {
            continue; // Row is outside the visible area
        }

        for offset in 0..len {
            let buf_x = area.x + (col + offset) as u16;
            if buf_x >= area.x + area.width {
                break; // Past the right edge
            }

            let buf_cell = &mut buf[(buf_x, buf_y)];
            buf_cell.bg = Color::Yellow;
            buf_cell.fg = Color::Black;
        }
    }
}

/// Render the output(s) of a cell, including inline images via Kitty graphics protocol.
fn render_outputs(
    frame: &mut Frame,
    app: &mut App,
    cell_idx: usize,
    outputs: &[crate::notebook::model::CellOutput],
    area: Rect,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    // We need to lay out text and images sequentially in the output area.
    // Build a list of output segments: either text lines or an image.
    enum OutputSegment {
        Text(Vec<Line<'static>>),
        Image { output_idx: usize, height: u16 },
    }

    let mut segments: Vec<OutputSegment> = Vec::new();

    for (output_idx, output) in outputs.iter().enumerate() {
        match output {
            crate::notebook::model::CellOutput::Stream { name, text } => {
                let mut lines = Vec::new();
                if name == "stderr" {
                    match text.into_text() {
                        Ok(parsed) => {
                            for mut line in parsed.lines {
                                for span in &mut line.spans {
                                    if span.style.fg.is_none() {
                                        span.style.fg = Some(Color::Red);
                                    }
                                }
                                lines.push(line);
                            }
                        }
                        Err(_) => {
                            for line in text.lines() {
                                lines.push(Line::from(Span::styled(
                                    line.to_string(),
                                    Style::default().fg(Color::Red),
                                )));
                            }
                        }
                    }
                } else {
                    match text.into_text() {
                        Ok(parsed) => lines.extend(parsed.lines),
                        Err(_) => {
                            for line in text.lines() {
                                lines.push(Line::from(Span::raw(line.to_string())));
                            }
                        }
                    }
                }
                if !lines.is_empty() {
                    segments.push(OutputSegment::Text(lines));
                }
            }
            crate::notebook::model::CellOutput::ExecuteResult { data, .. } => {
                // Render text/plain first
                if let Some(text) = data.get("text/plain") {
                    let mut lines = Vec::new();
                    match text.into_text() {
                        Ok(parsed) => {
                            for mut line in parsed.lines {
                                for span in &mut line.spans {
                                    if span.style.fg.is_none() {
                                        span.style.fg = Some(Color::Green);
                                    }
                                }
                                lines.push(line);
                            }
                        }
                        Err(_) => {
                            for line in text.lines() {
                                lines.push(Line::from(Span::styled(
                                    line.to_string(),
                                    Style::default().fg(Color::Green),
                                )));
                            }
                        }
                    }
                    if !lines.is_empty() {
                        segments.push(OutputSegment::Text(lines));
                    }
                }
                // Then render image if present
                if has_renderable_image(data) {
                    let img_height = get_image_data(data)
                        .and_then(decode_base64_image)
                        .map(|img| {
                            image_height_rows(&img, app.picker.font_size(), area.width) as u16
                        })
                        .unwrap_or(1);
                    segments.push(OutputSegment::Image {
                        output_idx,
                        height: img_height,
                    });
                }
            }
            crate::notebook::model::CellOutput::Error { traceback, .. } => {
                let mut lines = Vec::new();
                for tb_line in traceback {
                    match tb_line.into_text() {
                        Ok(parsed) => lines.extend(parsed.lines),
                        Err(_) => {
                            lines.push(Line::from(Span::styled(
                                tb_line.clone(),
                                Style::default().fg(Color::Red),
                            )));
                        }
                    }
                }
                if !lines.is_empty() {
                    segments.push(OutputSegment::Text(lines));
                }
            }
            crate::notebook::model::CellOutput::DisplayData { data } => {
                // Render text/plain if no image, or render both if both exist
                let has_img = has_renderable_image(data);

                if let Some(text) = data.get("text/plain") {
                    // Only show text/plain if there's no image (image is the preferred repr)
                    if !has_img {
                        let mut lines = Vec::new();
                        match text.into_text() {
                            Ok(parsed) => {
                                for mut line in parsed.lines {
                                    for span in &mut line.spans {
                                        if span.style.fg.is_none() {
                                            span.style.fg = Some(Color::Magenta);
                                        }
                                    }
                                    lines.push(line);
                                }
                            }
                            Err(_) => {
                                for line in text.lines() {
                                    lines.push(Line::from(Span::styled(
                                        line.to_string(),
                                        Style::default().fg(Color::Magenta),
                                    )));
                                }
                            }
                        }
                        if !lines.is_empty() {
                            segments.push(OutputSegment::Text(lines));
                        }
                    }
                }

                if has_img {
                    let img_height = get_image_data(data)
                        .and_then(decode_base64_image)
                        .map(|img| {
                            image_height_rows(&img, app.picker.font_size(), area.width) as u16
                        })
                        .unwrap_or(1);
                    segments.push(OutputSegment::Image {
                        output_idx,
                        height: img_height,
                    });
                }
            }
        }
    }

    // Now render each segment sequentially into the area
    let mut y = area.y;
    for segment in &segments {
        let remaining = area.y + area.height - y;
        if remaining == 0 {
            break;
        }

        match segment {
            OutputSegment::Text(lines) => {
                let h = (lines.len() as u16).min(remaining);
                let seg_area = Rect::new(area.x, y, area.width, h);
                let paragraph =
                    Paragraph::new(Text::from(lines.clone())).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, seg_area);
                y += h;
            }
            OutputSegment::Image { output_idx, height } => {
                let h = (*height).min(remaining);
                let seg_area = Rect::new(area.x, y, area.width, h);

                // Get or create the StatefulProtocol for this image
                let key = (cell_idx, *output_idx);
                if !app.image_states.contains_key(&key) {
                    // Decode the image and create a new protocol state
                    let img_data = outputs[*output_idx].image_data();
                    if let Some(dyn_img) = img_data.and_then(|d| decode_base64_image(d)) {
                        let protocol = app.picker.new_resize_protocol(dyn_img);
                        app.image_states.insert(key, protocol);
                    }
                }

                if let Some(protocol) = app.image_states.get_mut(&key) {
                    let image_widget = StatefulImage::default();
                    frame.render_stateful_widget(image_widget, seg_area, protocol);
                }
                y += h;
            }
        }
    }
}
