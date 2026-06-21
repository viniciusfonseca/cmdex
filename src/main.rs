mod app;
mod codex;
mod config;
mod workspace;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::DefaultTerminal;
use std::io::stdout;

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let result = run(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    app::run(terminal).await
}

fn restore_terminal(terminal: &mut DefaultTerminal) -> Result<()> {
    execute!(stdout(), DisableMouseCapture)?;
    disable_raw_mode()?;
    ratatui::restore();
    terminal.show_cursor()?;
    Ok(())
}
