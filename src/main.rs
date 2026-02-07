mod app;
mod event;
mod input;
mod kernel;
mod notebook;
mod ui;

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line args (just a file path for now)
    let file_path = std::env::args().nth(1);

    // Initialize terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;

    let mut terminal = ratatui::init();

    // Run the application
    let result = run(&mut terminal, file_path.as_deref()).await;

    // Restore terminal
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(io::stdout(), LeaveAlternateScreen).context("Failed to leave alternate screen")?;
    ratatui::restore();

    if let Err(ref e) = result {
        eprintln!("Error: {:?}", e);
    }

    result
}

async fn run(terminal: &mut ratatui::DefaultTerminal, file_path: Option<&str>) -> Result<()> {
    // Initialize app (starts kernel, connects, loads notebook)
    let (mut app, kernel_rx) = app::App::new(file_path).await?;

    // Set up event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Spawn event collection loop
    tokio::spawn(event::run_event_loop(event_tx, kernel_rx));

    // Initial draw
    app.draw(terminal)?;

    // Main event loop
    while !app.should_quit {
        if let Some(event) = event_rx.recv().await {
            app.handle_event(event).await?;
            app.draw(terminal)?;
        } else {
            break;
        }
    }

    // Graceful shutdown
    app.shutdown().await?;

    Ok(())
}
