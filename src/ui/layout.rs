use crate::app::{App, Mode};
use crate::ui::{cell, statusbar};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

/// Render the full application layout.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Cell area (takes remaining space)
            Constraint::Length(1), // Status bar
            Constraint::Length(1), // Command line / message
        ])
        .split(frame.area());

    // Render cells in the main area
    cell::render_cell_list(frame, app, chunks[0]);

    // Render status bar
    statusbar::render(frame, app, chunks[1]);

    // Render command line or status message
    render_command_line(frame, app, chunks[2]);
}

/// Render the bottom command/message line.
fn render_command_line(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;
    use ratatui::widgets::Paragraph;

    let content = if app.mode == Mode::Command {
        format!(":{}", app.command_buffer)
    } else {
        app.status_message.clone()
    };

    let style = if app.mode == Mode::Command {
        Style::default().fg(Color::White)
    } else if app.status_message.starts_with("Error")
        || app.status_message.starts_with("Save failed")
    {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let paragraph = Paragraph::new(Span::styled(content, style));
    frame.render_widget(paragraph, area);
}
