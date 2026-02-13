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
    /// Enter search mode (/ = forward, ? = backward)
    EnterSearch { forward: bool },
    /// Jump to next search match (n)
    SearchNext,
    /// Jump to previous search match (N)
    SearchPrev,
    /// Execute cell and exit to Normal mode (Shift+Enter)
    ExecuteCellAndExit,
}

/// Vim state machine for in-cell editing.
///
/// Tracks pending input (for gg, dd, yy, cc sequences), operator-pending
/// state (d{motion}, y{motion}, c{motion}), and count prefixes.
pub struct CellVim {
    pub pending: Pending,
    /// Accumulating count prefix (e.g., the "3" in "3w" or "2d3w")
    pub count: Option<usize>,
    /// Count that was active when an operator was entered (e.g., the "2" in "2dw").
    /// Multiplied with the motion count when the motion arrives.
    op_count: Option<usize>,
}

impl CellVim {
    pub fn new() -> Self {
        Self {
            pending: Pending::None,
            count: None,
            op_count: None,
        }
    }

    /// Consume the accumulated count, returning it (minimum 1).
    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1)
    }

    /// Get the effective count for a motion after an operator,
    /// multiplying op_count * motion_count per vim convention.
    fn effective_count(&mut self) -> usize {
        let motion_count = self.count.take().unwrap_or(1);
        let op_count = self.op_count.take().unwrap_or(1);
        op_count * motion_count
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

        // --- Digit accumulation for count prefix ---
        // 1-9 always start/continue a count. 0 continues if count is already started,
        // otherwise falls through to CursorMove::Head.
        if !ctrl && !shift {
            match key.code {
                KeyCode::Char(c @ '1'..='9') => {
                    let digit = c as usize - '0' as usize;
                    self.count = Some(self.count.unwrap_or(0) * 10 + digit);
                    return CellVimAction::Nop;
                }
                KeyCode::Char('0') if self.count.is_some() => {
                    self.count = Some(self.count.unwrap() * 10);
                    return CellVimAction::Nop;
                }
                _ => {}
            }
        }

        // Handle second key of pending two-key sequences
        match (&self.pending, key.code) {
            // gg -> go to top
            (Pending::Key(KeyCode::Char('g')), KeyCode::Char('g')) => {
                self.pending = Pending::None;
                self.count = None;
                textarea.move_cursor(CursorMove::Top);
                return CellVimAction::Nop;
            }
            // dd -> delete N lines
            (Pending::Operator(PendingOp::Delete), KeyCode::Char('d')) => {
                self.pending = Pending::None;
                let n = self.effective_count();
                delete_lines(textarea, n);
                return CellVimAction::Nop;
            }
            // yy -> yank N lines
            (Pending::Operator(PendingOp::Yank), KeyCode::Char('y')) => {
                self.pending = Pending::None;
                let n = self.effective_count();
                yank_lines(textarea, n);
                return CellVimAction::Nop;
            }
            // cc -> change N lines
            (Pending::Operator(PendingOp::Change), KeyCode::Char('c')) => {
                self.pending = Pending::None;
                let n = self.effective_count();
                change_lines(textarea, n);
                return CellVimAction::EnterInsert;
            }
            // Pending operator + motion: apply the operator over the motion
            (Pending::Operator(op), _) => {
                let op = *op;
                let n = self.effective_count();
                // The selection was started when the operator was entered.
                // Apply the motion N times, then cut/copy.
                let did_move = apply_motion_n(key, textarea, n);
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
                    self.op_count = None;
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
        let n = self.take_count();

        match key.code {
            // -- Motions (repeated N times) --
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                move_n(textarea, CursorMove::Back, n);
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                move_n(textarea, CursorMove::Down, n);
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                move_n(textarea, CursorMove::Up, n);
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                move_n(textarea, CursorMove::Forward, n);
            }
            KeyCode::Char('w') if !ctrl => {
                move_n(textarea, CursorMove::WordForward, n);
            }
            KeyCode::Char('e') if !ctrl => {
                move_n(textarea, CursorMove::WordEnd, n);
            }
            KeyCode::Char('b') if !ctrl => {
                move_n(textarea, CursorMove::WordBack, n);
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
                textarea.scroll((n as i16, 0));
            }
            KeyCode::Char('y') if ctrl => {
                textarea.scroll((-(n as i16), 0));
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
                for _ in 0..n {
                    textarea.delete_next_char();
                }
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
                for _ in 0..n {
                    textarea.paste();
                }
            }
            KeyCode::Char('u') if !ctrl => {
                for _ in 0..n {
                    textarea.undo();
                }
            }
            KeyCode::Char('r') if ctrl => {
                for _ in 0..n {
                    textarea.redo();
                }
            }
            KeyCode::Char('J') if !ctrl => {
                for _ in 0..n {
                    // Join lines: go to end of line, delete the newline
                    textarea.move_cursor(CursorMove::End);
                    textarea.delete_next_char();
                    // Insert a space where the join happened
                    textarea.insert_char(' ');
                }
            }

            // -- Operators --
            KeyCode::Char('d') if !ctrl => {
                textarea.start_selection();
                self.op_count = if n > 1 { Some(n) } else { None };
                self.pending = Pending::Operator(PendingOp::Delete);
            }
            KeyCode::Char('y') if !ctrl && !shift => {
                textarea.start_selection();
                self.op_count = if n > 1 { Some(n) } else { None };
                self.pending = Pending::Operator(PendingOp::Yank);
            }
            KeyCode::Char('c') if !ctrl => {
                textarea.start_selection();
                self.op_count = if n > 1 { Some(n) } else { None };
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

            // -- Search --
            KeyCode::Char('/') if !ctrl => {
                return CellVimAction::EnterSearch { forward: true };
            }
            KeyCode::Char('?') if !ctrl => {
                return CellVimAction::EnterSearch { forward: false };
            }
            KeyCode::Char('n') if !ctrl => {
                return CellVimAction::SearchNext;
            }
            KeyCode::Char('N') if !ctrl => {
                return CellVimAction::SearchPrev;
            }

            // -- Cell execution --
            KeyCode::Char('X') if shift => {
                return CellVimAction::ExecuteCell;
            }

            // -- Execute cell and exit to Normal mode --
            KeyCode::Enter if shift => {
                return CellVimAction::ExecuteCellAndExit;
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

        // --- Digit accumulation for count prefix ---
        if !ctrl {
            match key.code {
                KeyCode::Char(c @ '1'..='9') => {
                    let digit = c as usize - '0' as usize;
                    self.count = Some(self.count.unwrap_or(0) * 10 + digit);
                    return CellVimAction::Nop;
                }
                KeyCode::Char('0') if self.count.is_some() => {
                    self.count = Some(self.count.unwrap() * 10);
                    return CellVimAction::Nop;
                }
                _ => {}
            }
        }

        let n = self.take_count();

        match key.code {
            // Motions (extend selection, repeated N times)
            KeyCode::Char('h') | KeyCode::Left if !ctrl => {
                move_n(textarea, CursorMove::Back, n);
            }
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                move_n(textarea, CursorMove::Down, n);
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                move_n(textarea, CursorMove::Up, n);
            }
            KeyCode::Char('l') | KeyCode::Right if !ctrl => {
                move_n(textarea, CursorMove::Forward, n);
            }
            KeyCode::Char('w') if !ctrl => {
                move_n(textarea, CursorMove::WordForward, n);
            }
            KeyCode::Char('e') if !ctrl => {
                move_n(textarea, CursorMove::WordEnd, n);
            }
            KeyCode::Char('b') if !ctrl => {
                move_n(textarea, CursorMove::WordBack, n);
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
}

// --- Helpers ---

/// Move cursor N times in the given direction.
fn move_n(
    textarea: &mut tui_textarea::TextArea<'_>,
    cursor_move: tui_textarea::CursorMove,
    n: usize,
) {
    for _ in 0..n {
        textarea.move_cursor(cursor_move.clone());
    }
}

/// Apply a single motion to the textarea, returning true if recognized.
/// Used for operator+motion sequences. Does NOT handle count (caller repeats).
fn apply_motion_once(key: KeyEvent, textarea: &mut tui_textarea::TextArea<'_>) -> bool {
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

/// Apply a motion N times. Returns true if the motion was recognized.
fn apply_motion_n(key: KeyEvent, textarea: &mut tui_textarea::TextArea<'_>, n: usize) -> bool {
    // First application tells us if the motion is valid
    if !apply_motion_once(key, textarea) {
        return false;
    }
    // Repeat remaining times
    for _ in 1..n {
        apply_motion_once(key, textarea);
    }
    true
}

/// Delete N lines (dd with count). Selects from Head of current line
/// through N-1 lines down, then cuts.
fn delete_lines(textarea: &mut tui_textarea::TextArea<'_>, n: usize) {
    use tui_textarea::CursorMove;
    textarea.move_cursor(CursorMove::Head);
    textarea.start_selection();
    let cursor_before = textarea.cursor();
    for _ in 0..n {
        textarea.move_cursor(CursorMove::Down);
    }
    // If cursor didn't move (last line), select to end instead
    if textarea.cursor() == cursor_before {
        textarea.move_cursor(CursorMove::End);
    }
    textarea.cut();
}

/// Yank N lines (yy with count). Same selection as delete but copies.
fn yank_lines(textarea: &mut tui_textarea::TextArea<'_>, n: usize) {
    use tui_textarea::CursorMove;
    textarea.move_cursor(CursorMove::Head);
    textarea.start_selection();
    let cursor_before = textarea.cursor();
    for _ in 0..n {
        textarea.move_cursor(CursorMove::Down);
    }
    if textarea.cursor() == cursor_before {
        textarea.move_cursor(CursorMove::End);
    }
    textarea.copy();
}

/// Change N lines (cc with count). Selects from Head to End of the Nth line,
/// cuts, and leaves cursor ready for insert.
fn change_lines(textarea: &mut tui_textarea::TextArea<'_>, n: usize) {
    use tui_textarea::CursorMove;
    textarea.move_cursor(CursorMove::Head);
    textarea.start_selection();
    // For cc, we select to the end of the last line (not the start of next)
    if n > 1 {
        for _ in 0..n - 1 {
            textarea.move_cursor(CursorMove::Down);
        }
    }
    textarea.move_cursor(CursorMove::End);
    textarea.cut();
}
