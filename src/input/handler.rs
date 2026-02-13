use crate::app::{App, Mode, SearchDirection};
use crate::input::vim::CellVimAction;
use crate::notebook::model::{Cell, CellType};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use uuid::Uuid;

/// Handle key events in Normal mode (cell-level navigation and operations).
pub async fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // --- Digit accumulation for count prefix ---
    if !ctrl {
        match key.code {
            KeyCode::Char(c @ '1'..='9') => {
                let digit = c as usize - '0' as usize;
                app.normal_count = Some(app.normal_count.unwrap_or(0) * 10 + digit);
                return Ok(());
            }
            KeyCode::Char('0') if app.normal_count.is_some() => {
                app.normal_count = Some(app.normal_count.unwrap() * 10);
                return Ok(());
            }
            // Search
            KeyCode::Char('/') => {
                app.search_direction = SearchDirection::Forward;
                app.search_buffer.clear();
                app.search_from_cell = false;
                app.mode = Mode::Search;
            }
            KeyCode::Char('?') => {
                app.search_direction = SearchDirection::Backward;
                app.search_buffer.clear();
                app.search_from_cell = false;
                app.mode = Mode::Search;
            }
            KeyCode::Char('n') => {
                // Repeat last search forward across cells
                search_next_in_cells(app, false);
            }
            KeyCode::Char('N') => {
                // Repeat last search backward across cells
                search_next_in_cells(app, true);
            }
            KeyCode::Esc => {
                // Clear search highlights
                app.search_matches.clear();
                app.status_message = String::new();
            }

            _ => {}
        }
    }

    let n = app.normal_count.take().unwrap_or(1);
    let last_cell = app.notebook.cells.len().saturating_sub(1);

    match key.code {
        // Navigation (repeated N times)
        KeyCode::Char('j') | KeyCode::Down if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.selected_cell = (app.selected_cell + n).min(last_cell);
        }
        KeyCode::Char('k') | KeyCode::Up if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.selected_cell = app.selected_cell.saturating_sub(n);
        }

        // Move cell down/up (Shift+J/K)
        KeyCode::Char('J') => {
            for _ in 0..n {
                app.selected_cell = app.notebook.move_cell_down(app.selected_cell);
            }
        }
        KeyCode::Char('K') => {
            for _ in 0..n {
                app.selected_cell = app.notebook.move_cell_up(app.selected_cell);
            }
        }

        KeyCode::Char('g') => {
            // gg = go to first cell (simplified: single g goes to top)
            app.selected_cell = 0;
        }
        KeyCode::Char('G') => {
            if n > 1 {
                // NG = go to cell N (1-indexed, like vim)
                app.selected_cell = (n - 1).min(last_cell);
            } else {
                app.selected_cell = last_cell;
            }
        }

        // Enter cell in CellNormal mode
        KeyCode::Char('i') | KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.enter_cell();
        }

        // Execute cell and stay in Normal mode (Shift+Enter)
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.execute_selected_cell().await?;
        }

        // Cell operations
        KeyCode::Char('o') => {
            // Insert new code cell below and enter it
            let new_cell = Cell::new_code("");
            app.notebook.insert_cell_after(app.selected_cell, new_cell);
            app.selected_cell += 1;
            app.enter_cell();
            app.enter_cell_insert(); // Go straight to insert in a new cell
        }
        KeyCode::Char('O') => {
            // Insert new code cell above and enter it
            let new_cell = Cell::new_code("");
            app.notebook.insert_cell_before(app.selected_cell, new_cell);
            app.enter_cell();
            app.enter_cell_insert();
        }
        KeyCode::Char('d') => {
            // Delete selected cell, yank it into buffer
            if let Some(removed) = app.notebook.delete_cell(app.selected_cell) {
                app.yanked_cell = Some(removed);
                if app.selected_cell >= app.notebook.cells.len() {
                    app.selected_cell = app.notebook.cells.len() - 1;
                }
                app.status_message = "Cell deleted (yanked)".to_string();
            }
        }

        // Yank cell
        KeyCode::Char('y') => {
            app.yanked_cell = Some(app.notebook.cells[app.selected_cell].clone());
            app.status_message = "Cell yanked".to_string();
        }

        // Put (paste) yanked cell below
        KeyCode::Char('p') => {
            if let Some(cell) = &app.yanked_cell {
                let mut new_cell = cell.clone();
                new_cell.id = Uuid::new_v4().to_string(); // Fresh ID
                new_cell.clear_outputs();
                app.notebook.insert_cell_after(app.selected_cell, new_cell);
                app.selected_cell += 1;
                app.status_message = "Cell pasted below".to_string();
            } else {
                app.status_message = "Nothing to paste".to_string();
            }
        }

        // Put (paste) yanked cell above
        KeyCode::Char('P') => {
            if let Some(cell) = &app.yanked_cell {
                let mut new_cell = cell.clone();
                new_cell.id = Uuid::new_v4().to_string();
                new_cell.clear_outputs();
                app.notebook.insert_cell_before(app.selected_cell, new_cell);
                app.status_message = "Cell pasted above".to_string();
            } else {
                app.status_message = "Nothing to paste".to_string();
            }
        }

        // Execute cell
        KeyCode::Char('x') => {
            app.execute_selected_cell().await?;
        }
        // Execute and move to next cell
        KeyCode::Char('X') => {
            app.execute_selected_cell().await?;
            if app.selected_cell < last_cell {
                app.selected_cell += 1;
            }
        }

        // Change cell type
        KeyCode::Char('m') => {
            // Toggle between code and markdown
            let cell = &mut app.notebook.cells[app.selected_cell];
            cell.cell_type = match cell.cell_type {
                CellType::Code => CellType::Markdown,
                CellType::Markdown => CellType::Code,
                CellType::Raw => CellType::Code,
            };
            cell.clear_outputs();
            app.notebook.dirty = true;
        }

        // Command mode
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.command_buffer.clear();
        }

        // Quick save
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match app.notebook.save(None) {
                Ok(()) => app.status_message = "Saved".to_string(),
                Err(e) => app.status_message = format!("Save failed: {}", e),
            }
        }

        _ => {}
    }

    Ok(())
}

/// Handle key events in CellNormal mode (vim motions inside a cell).
pub async fn handle_cell_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    // Delegate to the CellVim state machine
    let editor = match &mut app.editor {
        Some(e) => e,
        None => {
            // No editor, shouldn't be in CellNormal -- bail to Normal
            app.mode = Mode::Normal;
            return Ok(());
        }
    };

    // Take cell_vim out temporarily to satisfy borrow checker
    let mut cell_vim = std::mem::replace(&mut app.cell_vim, crate::input::vim::CellVim::new());
    let action = cell_vim.handle_normal(key, editor);
    app.cell_vim = cell_vim;

    match action {
        CellVimAction::Nop => {}
        CellVimAction::EnterInsert => {
            app.enter_cell_insert();
        }
        CellVimAction::EnterVisual => {
            app.enter_cell_visual();
        }
        CellVimAction::EnterVisualLine => {
            // Select the full current line, then enter visual
            if let Some(editor) = &mut app.editor {
                use tui_textarea::CursorMove;
                editor.move_cursor(CursorMove::Head);
                editor.start_selection();
                editor.move_cursor(CursorMove::End);
            }
            app.mode = Mode::CellVisual;
        }
        CellVimAction::ExitCell => {
            app.exit_cell();
        }
        CellVimAction::EnterCommand => {
            app.mode = Mode::Command;
            app.command_buffer.clear();
        }
        CellVimAction::ExecuteCell => {
            // Sync editor to cell, execute, stay in cell
            app.sync_editor_to_cell();
            app.execute_selected_cell().await?;
        }
        CellVimAction::EnterSearch { forward } => {
            app.search_direction = if forward {
                SearchDirection::Forward
            } else {
                SearchDirection::Backward
            };
            app.search_buffer.clear();
            app.search_from_cell = true;
            app.mode = Mode::Search;
        }
        CellVimAction::SearchNext => {
            // Repeat search in current direction within the cell
            if let Some(editor) = &mut app.editor {
                if app.last_search.is_some() {
                    let found = match app.search_direction {
                        SearchDirection::Forward => editor.search_forward(false),
                        SearchDirection::Backward => editor.search_back(false),
                    };
                    if !found {
                        app.status_message = "Pattern not found".to_string();
                    }
                } else {
                    app.status_message = "No previous search".to_string();
                }
            }
        }
        CellVimAction::SearchPrev => {
            // Repeat search in opposite direction within the cell
            if let Some(editor) = &mut app.editor {
                if app.last_search.is_some() {
                    let found = match app.search_direction {
                        SearchDirection::Forward => editor.search_back(false),
                        SearchDirection::Backward => editor.search_forward(false),
                    };
                    if !found {
                        app.status_message = "Pattern not found".to_string();
                    }
                } else {
                    app.status_message = "No previous search".to_string();
                }
            }
        }
        CellVimAction::ExecuteCellAndExit => {
            // Sync editor, execute, then exit to Normal mode
            app.sync_editor_to_cell();
            app.execute_selected_cell().await?;
            app.exit_cell();
        }
    }

    Ok(())
}

/// Action to take after handling a CellInsert key event.
pub enum CellInsertAction {
    /// No special action needed
    None,
    /// Execute cell and exit to Normal mode (Shift+Enter)
    ExecuteAndExit,
    /// Request tab completion from the kernel
    RequestCompletion,
}

/// Handle key events in CellInsert mode (typing in a cell).
/// Esc returns to CellNormal (not App Normal).
pub fn handle_cell_insert_mode(app: &mut App, key: KeyEvent) -> CellInsertAction {
    // If completions are showing, handle navigation keys
    if !app.completions.is_empty() {
        match key.code {
            KeyCode::Tab => {
                // Cycle forward through completions
                app.completion_selected = (app.completion_selected + 1) % app.completions.len();
                return CellInsertAction::None;
            }
            KeyCode::BackTab => {
                // Cycle backward through completions
                if app.completion_selected == 0 {
                    app.completion_selected = app.completions.len() - 1;
                } else {
                    app.completion_selected -= 1;
                }
                return CellInsertAction::None;
            }
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                // Apply selected completion
                apply_completion(app);
                return CellInsertAction::None;
            }
            KeyCode::Esc => {
                // Dismiss completions, stay in CellInsert
                app.clear_completions();
                return CellInsertAction::None;
            }
            KeyCode::Up => {
                // Navigate up in completion list
                if app.completion_selected == 0 {
                    app.completion_selected = app.completions.len() - 1;
                } else {
                    app.completion_selected -= 1;
                }
                return CellInsertAction::None;
            }
            KeyCode::Down => {
                // Navigate down in completion list
                app.completion_selected = (app.completion_selected + 1) % app.completions.len();
                return CellInsertAction::None;
            }
            _ => {
                // Any other key dismisses completions and is processed normally
                app.clear_completions();
            }
        }
    }

    match key.code {
        KeyCode::Esc => {
            app.clear_completions();
            app.return_to_cell_normal();
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            // Execute cell and exit to Normal mode
            app.clear_completions();
            return CellInsertAction::ExecuteAndExit;
        }
        KeyCode::Tab if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            // Request completion from kernel
            return CellInsertAction::RequestCompletion;
        }
        _ => {
            // Forward the key event to the TextArea editor
            if let Some(editor) = &mut app.editor {
                editor.input(key);
            }
        }
    }
    CellInsertAction::None
}

/// Handle key events in CellVisual mode (visual selection in a cell).
pub fn handle_cell_visual_mode(app: &mut App, key: KeyEvent) -> bool {
    // Shift+Enter: execute cell and exit
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
        return true;
    }

    let editor = match &mut app.editor {
        Some(e) => e,
        None => {
            app.mode = Mode::Normal;
            return false;
        }
    };

    let mut cell_vim = std::mem::replace(&mut app.cell_vim, crate::input::vim::CellVim::new());
    let action = cell_vim.handle_visual(key, editor);
    app.cell_vim = cell_vim;

    match action {
        CellVimAction::Nop => {
            // Visual actions like y/d return Nop but we go back to CellNormal
            // Check if selection was cancelled (y/d/Esc all cancel it)
            if key.code == KeyCode::Char('y')
                || key.code == KeyCode::Char('d')
                || key.code == KeyCode::Esc
                || key.code == KeyCode::Char('v')
            {
                app.return_to_cell_normal();
            }
        }
        CellVimAction::EnterInsert => {
            // c in visual: cut selection then insert
            app.enter_cell_insert();
        }
        _ => {}
    }
    false
}

/// Handle key events in Command mode (:w, :q, :3c, :3, etc.).
pub async fn handle_command_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Return to whatever mode we came from
            if app.editor.is_some() {
                app.mode = Mode::CellNormal;
            } else {
                app.mode = Mode::Normal;
            }
            app.command_buffer.clear();
            app.status_message = String::new();
        }
        KeyCode::Enter => {
            let cmd = app.command_buffer.clone();
            app.command_buffer.clear();
            // Return to the appropriate mode first
            if app.editor.is_some() {
                app.mode = Mode::CellNormal;
            } else {
                app.mode = Mode::Normal;
            }
            execute_command(app, &cmd).await?;
        }
        KeyCode::Char(c) => {
            app.command_buffer.push(c);
        }
        KeyCode::Backspace => {
            app.command_buffer.pop();
            if app.command_buffer.is_empty() {
                if app.editor.is_some() {
                    app.mode = Mode::CellNormal;
                } else {
                    app.mode = Mode::Normal;
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// Handle key events in Search mode (/ or ? prompt).
pub fn handle_search_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            // Cancel search, return to previous mode
            app.search_buffer.clear();
            if app.search_from_cell && app.editor.is_some() {
                app.mode = Mode::CellNormal;
            } else {
                app.mode = Mode::Normal;
            }
            app.status_message = String::new();
        }
        KeyCode::Enter => {
            let pattern = app.search_buffer.clone();
            app.search_buffer.clear();

            if pattern.is_empty() {
                // Empty pattern: re-use last search if available
                if app.last_search.is_none() {
                    if app.search_from_cell && app.editor.is_some() {
                        app.mode = Mode::CellNormal;
                    } else {
                        app.mode = Mode::Normal;
                    }
                    app.status_message = "No previous search".to_string();
                    return;
                }
                // Fall through with existing last_search + pattern on editor
            } else {
                app.last_search = Some(pattern.clone());

                // Set the search pattern on the editor if we're in a cell
                if let Some(editor) = &mut app.editor {
                    // Use regex::escape for literal search (vim default)
                    let escaped = escape_regex(&pattern);
                    editor.set_search_pattern(escaped).ok();
                    editor.set_search_style(
                        ratatui::style::Style::default()
                            .bg(ratatui::style::Color::Yellow)
                            .fg(ratatui::style::Color::Black),
                    );
                }
            }

            if app.search_from_cell && app.editor.is_some() {
                // In-cell search: jump to first match
                if let Some(editor) = &mut app.editor {
                    let found = match app.search_direction {
                        SearchDirection::Forward => editor.search_forward(false),
                        SearchDirection::Backward => editor.search_back(false),
                    };
                    if !found {
                        app.status_message = "Pattern not found".to_string();
                    }
                }
                app.mode = Mode::CellNormal;
            } else {
                // Cross-cell search from Normal mode
                app.mode = Mode::Normal;
                search_next_in_cells(app, false);
            }
        }
        KeyCode::Char(c) => {
            app.search_buffer.push(c);
        }
        KeyCode::Backspace => {
            app.search_buffer.pop();
            if app.search_buffer.is_empty() {
                // Don't cancel on empty backspace, just leave buffer empty
                // User can press Esc to cancel
            }
        }
        _ => {}
    }
}

/// Search for the last_search pattern across cells starting from the current position.
/// `reverse` flips the direction relative to `app.search_direction`.
/// Stays in Normal mode and highlights matches across all cells.
fn search_next_in_cells(app: &mut App, reverse: bool) {
    let pattern = match &app.last_search {
        Some(p) => p.clone(),
        None => {
            app.status_message = "No previous search".to_string();
            return;
        }
    };

    let forward = match app.search_direction {
        SearchDirection::Forward => !reverse,
        SearchDirection::Backward => reverse,
    };

    let num_cells = app.notebook.cells.len();
    if num_cells == 0 {
        return;
    }

    // Build all matches across all cells for highlighting
    find_all_matches_in_cells(app, &pattern);

    // Search through cells starting from the one after (or before) the current
    let start = app.selected_cell;
    for offset in 1..=num_cells {
        let idx = if forward {
            (start + offset) % num_cells
        } else {
            (start + num_cells - offset) % num_cells
        };

        let source = &app.notebook.cells[idx].source;

        // Check if this cell has any match
        if find_pattern_in_text(source, &pattern, false, forward).is_some() {
            // Jump to the cell but stay in Normal mode
            if app.editor.is_some() {
                app.exit_cell();
            }
            app.selected_cell = idx;
            app.status_message = format!("/{}", pattern);
            return;
        }
    }

    // Also check the current cell itself if we looped all the way around
    let source = &app.notebook.cells[start].source;
    if find_pattern_in_text(source, &pattern, false, forward).is_some() {
        app.status_message = format!("/{} (same cell)", pattern);
        return;
    }

    app.status_message = format!("Pattern not found: {}", pattern);
}

/// Find a pattern in text, returning (row, col) of the first match.
/// If `is_current_cell` is true and we're searching forward, we skip to find
/// matches after the current cursor conceptually (we just find the first match
/// since we don't track cursor position across cells in Normal mode).
fn find_pattern_in_text(
    source: &str,
    pattern: &str,
    is_current_cell: bool,
    forward: bool,
) -> Option<(usize, usize)> {
    let pattern_lower = pattern.to_lowercase();
    let lines: Vec<&str> = source.lines().collect();

    if lines.is_empty() {
        return None;
    }

    if forward {
        for (row, line) in lines.iter().enumerate() {
            if let Some(col) = line.to_lowercase().find(&pattern_lower) {
                // Skip the very first match in the current cell to avoid finding
                // the same match we're already on (for offset == 0)
                if is_current_cell && row == 0 && col == 0 {
                    // Try to find a later match on the same line
                    if let Some(col2) = line[col + 1..].to_lowercase().find(&pattern_lower) {
                        return Some((row, col + 1 + col2));
                    }
                    // Otherwise continue to next lines
                    continue;
                }
                return Some((row, col));
            }
        }
    } else {
        for row in (0..lines.len()).rev() {
            if let Some(col) = lines[row].to_lowercase().rfind(&pattern_lower) {
                return Some((row, col));
            }
        }
    }

    None
}

/// Find all matches of a pattern across all cells and store them in app.search_matches.
fn find_all_matches_in_cells(app: &mut App, pattern: &str) {
    app.search_matches.clear();
    let pattern_lower = pattern.to_lowercase();
    let pattern_len = pattern.len();

    for (cell_idx, cell) in app.notebook.cells.iter().enumerate() {
        for (row, line) in cell.source.lines().enumerate() {
            let line_lower = line.to_lowercase();
            let mut start = 0;
            while let Some(col) = line_lower[start..].find(&pattern_lower) {
                app.search_matches
                    .push((cell_idx, row, start + col, pattern_len));
                start += col + 1;
            }
        }
    }
}

/// Apply the currently selected completion to the editor.
/// Replaces the text between cursor_start and cursor_end with the selected match.
fn apply_completion(app: &mut App) {
    let selected = app.completion_selected;
    if selected >= app.completions.len() {
        app.clear_completions();
        return;
    }

    let completion = app.completions[selected].clone();
    let cursor_start = app.completion_cursor_start;
    let cursor_end = app.completion_cursor_end;
    app.clear_completions();

    if let Some(editor) = &mut app.editor {
        // Build the full source, replace the range, and reload the editor
        let lines = editor.lines();
        let source = lines.join("\n");

        // cursor_start and cursor_end are byte offsets into the full source
        if cursor_start <= source.len() && cursor_end <= source.len() && cursor_start <= cursor_end
        {
            let new_source = format!(
                "{}{}{}",
                &source[..cursor_start],
                completion,
                &source[cursor_end..]
            );

            // Calculate the cursor position after insertion
            let new_cursor_byte = cursor_start + completion.len();
            let (new_row, new_col) = byte_offset_to_row_col(&new_source, new_cursor_byte);

            // Reload editor with new content
            let new_lines: Vec<String> = new_source.lines().map(|l| l.to_string()).collect();
            let new_lines = if new_lines.is_empty() {
                vec![String::new()]
            } else {
                new_lines
            };

            // Preserve editor settings
            let mut new_editor = tui_textarea::TextArea::new(new_lines);
            new_editor.set_cursor_style(
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Reset)
                    .bg(ratatui::style::Color::White),
            );
            new_editor.set_cursor_line_style(ratatui::style::Style::default());
            new_editor.move_cursor(tui_textarea::CursorMove::Jump(
                new_row as u16,
                new_col as u16,
            ));

            // Copy search pattern if any
            if let Some(pattern) = &app.last_search {
                let escaped = crate::input::handler::escape_regex(pattern);
                new_editor.set_search_pattern(escaped).ok();
                new_editor.set_search_style(
                    ratatui::style::Style::default()
                        .bg(ratatui::style::Color::Yellow)
                        .fg(ratatui::style::Color::Black),
                );
            }

            *editor = new_editor;
        }
    }
}

/// Convert a byte offset in a string to (row, col) coordinates.
fn byte_offset_to_row_col(s: &str, offset: usize) -> (usize, usize) {
    let mut row = 0;
    let mut col = 0;
    for (i, c) in s.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            row += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (row, col)
}

/// Escape a string for use as a literal regex pattern.
pub fn escape_regex(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Parse and execute a command string.
async fn execute_command(app: &mut App, cmd: &str) -> Result<()> {
    let cmd = cmd.trim();

    match cmd {
        "q" | "quit" => {
            if app.notebook.dirty {
                app.status_message =
                    "Unsaved changes (use :q! to force quit, or :w to save first)".to_string();
            } else {
                app.should_quit = true;
            }
        }
        "q!" => {
            app.notebook.dirty = false;
            app.should_quit = true;
        }
        "w" | "write" => match app.notebook.save(None) {
            Ok(()) => app.status_message = "Saved".to_string(),
            Err(e) => app.status_message = format!("Save failed: {}", e),
        },
        "wq" | "x" => match app.notebook.save(None) {
            Ok(()) => {
                app.status_message = "Saved".to_string();
                app.should_quit = true;
            }
            Err(e) => app.status_message = format!("Save failed: {}", e),
        },
        "run-all" | "ra" => {
            app.execute_all_cells().await?;
        }
        "restart" => {
            app.restart_kernel().await?;
        }
        "restart!" => {
            // Restart kernel and run all cells
            app.restart_kernel().await?;
            app.execute_all_cells().await?;
        }
        "interrupt" => {
            let _ = app.kernel_client.interrupt().await;
            app.status_message = "Interrupt sent to kernel".to_string();
        }
        _ => {
            // Check for :Nc pattern (go to cell N)
            // e.g., :3c goes to cell 3
            if let Some(rest) = cmd.strip_suffix('c') {
                if let Ok(n) = rest.parse::<usize>() {
                    if n > 0 && n <= app.notebook.cells.len() {
                        // If we're in a cell, exit it first
                        if app.editor.is_some() {
                            app.exit_cell();
                        }
                        app.selected_cell = n - 1; // 1-indexed to 0-indexed
                        app.status_message = format!("Jumped to cell {}", n);
                    } else {
                        app.status_message =
                            format!("Cell {} out of range (1-{})", n, app.notebook.cells.len());
                    }
                } else {
                    app.status_message = format!("Unknown command: {}", cmd);
                }
            }
            // :N (bare number) -- jump to line N when inside a cell
            else if let Ok(n) = cmd.parse::<usize>() {
                if app.editor.is_some() {
                    if let Some(editor) = &mut app.editor {
                        let line_count = editor.lines().len();
                        if n > 0 && n <= line_count {
                            editor.move_cursor(tui_textarea::CursorMove::Jump(
                                n.saturating_sub(1) as u16,
                                0,
                            ));
                            app.status_message = format!("Line {}", n);
                        } else {
                            app.status_message =
                                format!("Line {} out of range (1-{})", n, line_count);
                        }
                    }
                } else {
                    app.status_message =
                        format!("Unknown command: {} (use :{}c for cell)", cmd, cmd);
                }
            }
            // :w <filename> - save to specific file
            else if let Some(filename) = cmd.strip_prefix("w ") {
                let path = std::path::Path::new(filename.trim());
                match app.notebook.save(Some(path)) {
                    Ok(()) => app.status_message = format!("Saved to {}", filename.trim()),
                    Err(e) => app.status_message = format!("Save failed: {}", e),
                }
            } else {
                app.status_message = format!("Unknown command: {}", cmd);
            }
        }
    }

    Ok(())
}
