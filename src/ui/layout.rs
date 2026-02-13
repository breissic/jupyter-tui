use crate::app::{App, Mode};
use crate::ui::{cell, statusbar};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Render the full application layout.
pub fn render(frame: &mut Frame, app: &mut App) {
    let has_completions = !app.completions.is_empty();
    let completion_height = if has_completions {
        // Show up to 8 completions + 1 for border
        (app.completions.len().min(8) + 2) as u16
    } else {
        0
    };

    let chunks = if has_completions {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),                    // Cell area
                Constraint::Length(completion_height), // Completion panel
                Constraint::Length(1),                 // Status bar
                Constraint::Length(1),                 // Command line
            ])
            .split(frame.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // Cell area
                Constraint::Length(1), // Status bar
                Constraint::Length(1), // Command line
            ])
            .split(frame.area())
    };

    // Render cells in the main area
    cell::render_cell_list(frame, app, chunks[0]);

    if has_completions {
        render_completion_panel(frame, app, chunks[1]);
        statusbar::render(frame, app, chunks[2]);
        render_command_line(frame, app, chunks[3]);
    } else {
        statusbar::render(frame, app, chunks[1]);
        render_command_line(frame, app, chunks[2]);
    }
}

/// Render the completion panel.
fn render_completion_panel(frame: &mut Frame, app: &App, area: Rect) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};

    let max_visible = area.height.saturating_sub(2) as usize; // -2 for borders
    let total = app.completions.len();
    let selected = app.completion_selected;

    // Scrolling: ensure selected item is visible
    let scroll_offset = if selected >= max_visible {
        selected - max_visible + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for i in scroll_offset..total.min(scroll_offset + max_visible) {
        let item = &app.completions[i];
        let style = if i == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(item.clone(), style)));
    }

    let title = format!(" Completions ({}/{}) ", selected + 1, total);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the bottom command/message line.
fn render_command_line(frame: &mut Frame, app: &App, area: Rect) {
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;
    use ratatui::widgets::Paragraph;

    let content = if app.mode == Mode::Command {
        format!(":{}", app.command_buffer)
    } else if app.mode == Mode::Search {
        let prefix = match app.search_direction {
            crate::app::SearchDirection::Forward => "/",
            crate::app::SearchDirection::Backward => "?",
        };
        format!("{}{}", prefix, app.search_buffer)
    } else {
        app.status_message.clone()
    };

    let style = if app.mode == Mode::Command || app.mode == Mode::Search {
        Style::default().fg(Color::White)
    } else if app.status_message.starts_with("Error")
        || app.status_message.starts_with("Save failed")
    {
        Style::default().fg(Color::Red)
    } else if app.status_message.starts_with("Unsaved changes") {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let paragraph = Paragraph::new(Span::styled(content, style));
    frame.render_widget(paragraph, area);
}
