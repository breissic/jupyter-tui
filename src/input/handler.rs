use crate::app::{App, Mode};
use crate::input::vim::CellVimAction;
use crate::notebook::model::{Cell, CellType};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key events in Normal mode (cell-level navigation and operations).
pub async fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected_cell < app.notebook.cells.len() - 1 {
                app.selected_cell += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_cell > 0 {
                app.selected_cell -= 1;
            }
        }
        KeyCode::Char('g') => {
            // gg = go to first cell (simplified: single g goes to top)
            app.selected_cell = 0;
        }
        KeyCode::Char('G') => {
            app.selected_cell = app.notebook.cells.len() - 1;
        }

        // Enter cell in CellNormal mode
        KeyCode::Char('i') | KeyCode::Enter => {
            app.enter_cell();
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
            // dd = delete cell (simplified: single d deletes)
            if let Some(_removed) = app.notebook.delete_cell(app.selected_cell) {
                if app.selected_cell >= app.notebook.cells.len() {
                    app.selected_cell = app.notebook.cells.len() - 1;
                }
                app.status_message = "Cell deleted".to_string();
            }
        }

        // Execute cell
        KeyCode::Char('x') => {
            app.execute_selected_cell().await?;
        }
        // Execute and move to next cell
        KeyCode::Char('X') => {
            app.execute_selected_cell().await?;
            if app.selected_cell < app.notebook.cells.len() - 1 {
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
    }

    Ok(())
}

/// Handle key events in CellInsert mode (typing in a cell).
/// Esc returns to CellNormal (not App Normal).
pub fn handle_cell_insert_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.return_to_cell_normal();
        }
        _ => {
            // Forward the key event to the TextArea editor
            if let Some(editor) = &mut app.editor {
                editor.input(key);
            }
        }
    }
}

/// Handle key events in CellVisual mode (visual selection in a cell).
pub fn handle_cell_visual_mode(app: &mut App, key: KeyEvent) {
    let editor = match &mut app.editor {
        Some(e) => e,
        None => {
            app.mode = Mode::Normal;
            return;
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

/// Parse and execute a command string.
async fn execute_command(app: &mut App, cmd: &str) -> Result<()> {
    let cmd = cmd.trim();

    match cmd {
        "q" | "quit" => {
            app.should_quit = true;
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
