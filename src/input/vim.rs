use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Operator-pending state for vim d/y/c commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOp {
    Delete,
    Yank,
    Change,
}

/// Pending input state for multi-key sequences (gg, dd, yy, cc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    None,
    /// Waiting for second key in a sequence (e.g., first 'g' of 'gg')
    Key(KeyCode),
    /// Operator pending: waiting for a motion to apply the operator
    Operator(PendingOp),
}

/// What the vim state machine tells the caller to do.
pub enum CellVimAction {
    /// No action needed (motion was applied directly to textarea)
    Nop,
    /// Transition to CellInsert mode
    EnterInsert,
    /// Transition to CellVisual mode
    EnterVisual,
    /// Transition to CellVisual line mode (select full line)
    EnterVisualLine,
    /// Exit cell back to App Normal mode
    ExitCell,
    /// Enter command mode (: was pressed)
    EnterCommand,
    /// Execute the current cell
    ExecuteCell,
}

/// Vim state machine for in-cell editing.
///
/// Tracks pending input (for gg, dd, yy, cc sequences) and operator-pending
/// state (d{motion}, y{motion}, c{motion}).
pub struct CellVim {
    pub pending: Pending,
}

impl CellVim {
    pub fn new() -> Self {
        Self {
            pending: Pending::None,
        }
    }

    /// Process a key event in CellNormal mode.
    /// Applies cursor movements and edits directly to the textarea.
    /// Returns an action for mode transitions the caller must handle.
    pub fn handle_normal(
        &mut self,
        key: KeyEvent,
        textarea: &mut tui_textarea::TextArea<'_>,
    ) -> CellVimAction {
        use tui_textarea::CursorMove;

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // Handle second key of pending two-key sequences
        match (&self.pending, key.code) {
            // gg -> go to top
            (Pending::Key(KeyCode::Char('g')), KeyCode::Char('g')) => {
                self.pending = Pending::None;
                textarea.move_cursor(CursorMove::Top);
                return CellVimAction::Nop;
            }
            // dd -> delete line
            (Pending::Operator(PendingOp::Delete), KeyCode::Char('d')) => {
                self.pending = Pending::None;
                textarea.move_cursor(CursorMove::Head);
                textarea.start_selection();
                let cursor = textarea.cursor();
                textarea.move_cursor(CursorMove::Down);
                if cursor == textarea.cursor() {
                    textarea.move_cursor(CursorMove::End);
                }
                textarea.cut();
                return CellVimAction::Nop;
            }
            // yy -> yank line
            (Pending::Operator(PendingOp::Yank), KeyCode::Char('y')) => {
                self.pending = Pending::None;
                textarea.move_cursor(CursorMove::Head);
                textarea.start_selection();
                let cursor = textarea.cursor();
                textarea.move_cursor(CursorMove::Down);
                if cursor == textarea.cursor() {
                    textarea.move_cursor(CursorMove::End);
                }
                textarea.copy();
                return CellVimAction::Nop;
            }
            // cc -> change line
            (Pending::Operator(PendingOp::Change), KeyCode::Char('c')) => {
                self.pending = Pending::None;
                textarea.move_cursor(CursorMove::Head);
                textarea.start_selection();
                textarea.move_cursor(CursorMove::End);
                textarea.cut();
                return CellVimAction::EnterInsert;
            }
            // Pending operator + motion: apply the operator over the motion
            (Pending::Operator(op), _) => {
                let op = *op;
                // The selection was started when the operator was entered.
                // Apply the motion, then cut/copy.
                let did_move = self.apply_motion(key, textarea);
                if did_move {
                    match op {
                        PendingOp::Delete => {
                            textarea.cut();
                            self.pending = Pending::None;
                            return CellVimAction::Nop;
                        }
                        PendingOp::Yank => {
                            textarea.copy();
                            self.pending = Pending::None;
                            return CellVimAction::Nop;
                        }
                        PendingOp::Change => {
                            textarea.cut();
                            self.pending = Pending::None;
                            return CellVimAction::EnterInsert;
                        }
                    }
                } else {
                    // Motion not recognized, cancel pending
                    self.pending = Pending::None;
                    textarea.cancel_selection();
                    return CellVimAction::Nop;
                }
            }
            // First 'g' was pressed but second key isn't 'g' -> cancel
            (Pending::Key(KeyCode::Char('g')), _) => {
                self.pending = Pending::None;
                // Fall through to normal key handling
            }
            (Pending::Key(_), _) => {
                self.pending = Pending::None;
            }
            _ => {}
        }

        // Normal mode key handling
        match key.code {
            // -- Motions --
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                textarea.move_cursor(CursorMove::Back);
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                textarea.move_cursor(CursorMove::Down);
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                textarea.move_cursor(CursorMove::Up);
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                textarea.move_cursor(CursorMove::Forward);
            }
            KeyCode::Char('w') if !ctrl => {
                textarea.move_cursor(CursorMove::WordForward);
            }
            KeyCode::Char('e') if !ctrl => {
                textarea.move_cursor(CursorMove::WordEnd);
            }
            KeyCode::Char('b') if !ctrl => {
                textarea.move_cursor(CursorMove::WordBack);
            }
            KeyCode::Char('0') => {
                textarea.move_cursor(CursorMove::Head);
            }
            KeyCode::Char('^') => {
                textarea.move_cursor(CursorMove::Head);
            }
            KeyCode::Char('$') => {
                textarea.move_cursor(CursorMove::End);
            }
            KeyCode::Char('G') if !ctrl => {
                textarea.move_cursor(CursorMove::Bottom);
            }
            KeyCode::Char('g') if !ctrl => {
                self.pending = Pending::Key(KeyCode::Char('g'));
            }

            // -- Scrolling --
            KeyCode::Char('e') if ctrl => {
                textarea.scroll((1, 0));
            }
            KeyCode::Char('y') if ctrl => {
                textarea.scroll((-1, 0));
            }
            KeyCode::Char('d') if ctrl => {
                textarea.scroll(tui_textarea::Scrolling::HalfPageDown);
            }
            KeyCode::Char('u') if ctrl => {
                textarea.scroll(tui_textarea::Scrolling::HalfPageUp);
            }
            KeyCode::Char('f') if ctrl => {
                textarea.scroll(tui_textarea::Scrolling::PageDown);
            }
            KeyCode::Char('b') if ctrl => {
                textarea.scroll(tui_textarea::Scrolling::PageUp);
            }

            // -- Enter insert mode --
            KeyCode::Char('i') if !ctrl => {
                textarea.cancel_selection();
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('a') if !ctrl && !shift => {
                textarea.cancel_selection();
                textarea.move_cursor(CursorMove::Forward);
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('A') if !ctrl => {
                textarea.cancel_selection();
                textarea.move_cursor(CursorMove::End);
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('I') if !ctrl => {
                textarea.cancel_selection();
                textarea.move_cursor(CursorMove::Head);
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('o') if !ctrl => {
                textarea.move_cursor(CursorMove::End);
                textarea.insert_newline();
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('O') if !ctrl => {
                textarea.move_cursor(CursorMove::Head);
                textarea.insert_newline();
                textarea.move_cursor(CursorMove::Up);
                return CellVimAction::EnterInsert;
            }

            // -- Editing in normal mode --
            KeyCode::Char('x') if !ctrl => {
                textarea.delete_next_char();
            }
            KeyCode::Char('D') if !ctrl => {
                textarea.delete_line_by_end();
            }
            KeyCode::Char('C') if !ctrl => {
                textarea.delete_line_by_end();
                textarea.cancel_selection();
                return CellVimAction::EnterInsert;
            }
            KeyCode::Char('p') if !ctrl => {
                textarea.paste();
            }
            KeyCode::Char('u') if !ctrl => {
                textarea.undo();
            }
            KeyCode::Char('r') if ctrl => {
                textarea.redo();
            }
            KeyCode::Char('J') if !ctrl => {
                // Join lines: go to end of line, delete the newline
                textarea.move_cursor(CursorMove::End);
                textarea.delete_next_char();
                // Insert a space where the join happened
                textarea.insert_char(' ');
            }

            // -- Operators --
            KeyCode::Char('d') if !ctrl => {
                textarea.start_selection();
                self.pending = Pending::Operator(PendingOp::Delete);
            }
            KeyCode::Char('y') if !ctrl && !shift => {
                textarea.start_selection();
                self.pending = Pending::Operator(PendingOp::Yank);
            }
            KeyCode::Char('c') if !ctrl => {
                textarea.start_selection();
                self.pending = Pending::Operator(PendingOp::Change);
            }

            // -- Visual mode --
            KeyCode::Char('v') if !ctrl => {
                return CellVimAction::EnterVisual;
            }
            KeyCode::Char('V') if !ctrl => {
                return CellVimAction::EnterVisualLine;
            }

            // -- Command mode --
            KeyCode::Char(':') => {
                return CellVimAction::EnterCommand;
            }

            // -- Cell execution --
            KeyCode::Char('X') if shift => {
                return CellVimAction::ExecuteCell;
            }

            // -- Exit cell --
            KeyCode::Esc => {
                return CellVimAction::ExitCell;
            }

            _ => {}
        }

        CellVimAction::Nop
    }

    /// Process a key event in CellVisual mode.
    /// The selection is already active. Motions extend it.
    pub fn handle_visual(
        &mut self,
        key: KeyEvent,
        textarea: &mut tui_textarea::TextArea<'_>,
    ) -> CellVimAction {
        use tui_textarea::CursorMove;

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            // Motions (extend selection)
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                textarea.move_cursor(CursorMove::Back);
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                textarea.move_cursor(CursorMove::Down);
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                textarea.move_cursor(CursorMove::Up);
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                textarea.move_cursor(CursorMove::Forward);
            }
            KeyCode::Char('w') if !ctrl => {
                textarea.move_cursor(CursorMove::WordForward);
            }
            KeyCode::Char('e') if !ctrl => {
                textarea.move_cursor(CursorMove::WordEnd);
            }
            KeyCode::Char('b') if !ctrl => {
                textarea.move_cursor(CursorMove::WordBack);
            }
            KeyCode::Char('0') => {
                textarea.move_cursor(CursorMove::Head);
            }
            KeyCode::Char('^') => {
                textarea.move_cursor(CursorMove::Head);
            }
            KeyCode::Char('$') => {
                textarea.move_cursor(CursorMove::End);
            }
            KeyCode::Char('G') if !ctrl => {
                textarea.move_cursor(CursorMove::Bottom);
            }
            KeyCode::Char('g') if !ctrl => {
                // Handle gg in visual
                match &self.pending {
                    Pending::Key(KeyCode::Char('g')) => {
                        self.pending = Pending::None;
                        textarea.move_cursor(CursorMove::Top);
                    }
                    _ => {
                        self.pending = Pending::Key(KeyCode::Char('g'));
                    }
                }
            }

            // Actions on selection
            KeyCode::Char('y') if !ctrl => {
                textarea.move_cursor(CursorMove::Forward); // Vim inclusive selection
                textarea.copy();
                return CellVimAction::Nop; // Caller will return to CellNormal
            }
            KeyCode::Char('d') if !ctrl => {
                textarea.move_cursor(CursorMove::Forward);
                textarea.cut();
                return CellVimAction::Nop;
            }
            KeyCode::Char('c') if !ctrl => {
                textarea.move_cursor(CursorMove::Forward);
                textarea.cut();
                return CellVimAction::EnterInsert;
            }

            // Cancel visual
            KeyCode::Esc | KeyCode::Char('v') => {
                textarea.cancel_selection();
                return CellVimAction::Nop;
            }

            _ => {}
        }

        CellVimAction::Nop
    }

    /// Apply a motion to the textarea, returning true if a known motion was applied.
    fn apply_motion(&self, key: KeyEvent, textarea: &mut tui_textarea::TextArea<'_>) -> bool {
        use tui_textarea::CursorMove;

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                textarea.move_cursor(CursorMove::Back);
                true
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                textarea.move_cursor(CursorMove::Down);
                true
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                textarea.move_cursor(CursorMove::Up);
                true
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                textarea.move_cursor(CursorMove::Forward);
                true
            }
            KeyCode::Char('w') if !ctrl => {
                textarea.move_cursor(CursorMove::WordForward);
                true
            }
            KeyCode::Char('e') if !ctrl => {
                textarea.move_cursor(CursorMove::WordEnd);
                textarea.move_cursor(CursorMove::Forward); // inclusive
                true
            }
            KeyCode::Char('b') if !ctrl => {
                textarea.move_cursor(CursorMove::WordBack);
                true
            }
            KeyCode::Char('0') | KeyCode::Char('^') => {
                textarea.move_cursor(CursorMove::Head);
                true
            }
            KeyCode::Char('$') => {
                textarea.move_cursor(CursorMove::End);
                true
            }
            KeyCode::Char('G') if !ctrl => {
                textarea.move_cursor(CursorMove::Bottom);
                true
            }
            _ => false,
        }
    }
}
