use crate::app::App;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Render the status bar at the bottom of the screen.
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mode_style = match &app.mode {
        crate::app::Mode::Normal => Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        crate::app::Mode::Insert => Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
        crate::app::Mode::Visual => Style::default()
            .fg(Color::Black)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        crate::app::Mode::Command => Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    };

    let mode_text = format!(" {} ", app.mode);

    let file_name = app
        .notebook
        .file_path
        .as_ref()
        .map(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if app.notebook.dirty {
                format!("{} [+]", name)
            } else {
                name
            }
        })
        .unwrap_or_else(|| {
            if app.notebook.dirty {
                "[No Name] [+]".to_string()
            } else {
                "[No Name]".to_string()
            }
        });

    let cell_info = format!(
        "Cell {}/{} ",
        app.selected_cell + 1,
        app.notebook.cells.len()
    );

    let kernel_style = match app.kernel_status.as_str() {
        "busy" => Style::default().fg(Color::Black).bg(Color::Yellow),
        "idle" => Style::default().fg(Color::Black).bg(Color::Green),
        _ => Style::default().fg(Color::Black).bg(Color::Red),
    };
    let kernel_text = format!(" {} ", app.kernel_status);

    // Calculate padding
    let left_len = mode_text.len() + file_name.len() + 2;
    let right_len = cell_info.len() + kernel_text.len();
    let padding = if area.width as usize > left_len + right_len {
        " ".repeat(area.width as usize - left_len - right_len)
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(mode_text, mode_style),
        Span::raw(" "),
        Span::styled(file_name, Style::default().fg(Color::White)),
        Span::raw(padding),
        Span::styled(cell_info, Style::default().fg(Color::DarkGray)),
        Span::styled(kernel_text, kernel_style),
    ]);

    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}
