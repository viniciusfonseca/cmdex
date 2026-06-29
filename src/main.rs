mod app;
mod codex;
mod config;
mod syntax;
mod theme;
mod workspace;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::DefaultTerminal;
use std::{
    io::stdout,
    process::{Command, Stdio},
};
use tokio::runtime::Builder;

fn main() -> Result<()> {
    let runtime = Builder::new_multi_thread().enable_all().build()?;
    enable_raw_mode()?;
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let result = runtime.block_on(run(&mut terminal));
    restore_terminal(&mut terminal)?;
    drop(terminal);

    let exit = result?;
    drop(runtime);

    match exit {
        app::AppExit::Quit => Ok(()),
        app::AppExit::Restart => restart_cmdex(),
    }
}

async fn run(terminal: &mut DefaultTerminal) -> Result<app::AppExit> {
    app::run(terminal).await
}

fn restore_terminal(terminal: &mut DefaultTerminal) -> Result<()> {
    execute!(stdout(), DisableMouseCapture)?;
    disable_raw_mode()?;
    ratatui::restore();
    terminal.show_cursor()?;
    Ok(())
}

fn restart_cmdex() -> Result<()> {
    let executable = std::env::current_exe()?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let cwd = std::env::current_dir()?;

    Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    Ok(())
}
