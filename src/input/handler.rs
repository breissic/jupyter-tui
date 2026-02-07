use crate::app::{App, Mode};
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

        // Enter insert mode
        KeyCode::Char('i') | KeyCode::Enter => {
            app.enter_insert_mode();
        }

        // Cell operations
        KeyCode::Char('o') => {
            // Insert new code cell below
            let new_cell = Cell::new_code("");
            app.notebook.insert_cell_after(app.selected_cell, new_cell);
            app.selected_cell += 1;
            app.enter_insert_mode();
        }
        KeyCode::Char('O') => {
            // Insert new code cell above
            let new_cell = Cell::new_code("");
            app.notebook.insert_cell_before(app.selected_cell, new_cell);
            app.enter_insert_mode();
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
        // Shift+Enter also executes (if terminal supports it)
        KeyCode::Char('X') => {
            app.execute_selected_cell().await?;
            // Move to next cell after execution
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

/// Handle key events in Insert mode (editing cell content via tui-textarea).
///
/// Esc exits insert mode. All other keys are forwarded to the TextArea.
pub fn handle_insert_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.exit_insert_mode();
        }
        _ => {
            // Forward the key event to the TextArea editor
            if let Some(editor) = &mut app.editor {
                editor.input(key);
            }
        }
    }
}

/// Handle key events in Command mode (:w, :q, :3c, etc.).
pub async fn handle_command_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.command_buffer.clear();
            app.status_message = String::new();
        }
        KeyCode::Enter => {
            let cmd = app.command_buffer.clone();
            app.command_buffer.clear();
            app.mode = Mode::Normal;
            execute_command(app, &cmd).await?;
        }
        KeyCode::Char(c) => {
            app.command_buffer.push(c);
        }
        KeyCode::Backspace => {
            app.command_buffer.pop();
            if app.command_buffer.is_empty() {
                app.mode = Mode::Normal;
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
