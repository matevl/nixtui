mod app;
mod config;
mod event;
mod nix;
mod ui;

use anyhow::Result;
use ratatui::{
    crossterm::{
        event::{
            KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        },
        execute,
        terminal::supports_keyboard_enhancement,
    },
    DefaultTerminal,
};
use tokio::sync::mpsc;

use crate::{app::App, config::Config};

#[tokio::main]
async fn main() -> Result<()> {
    // Installs a panic hook that restores the terminal before printing the panic.
    let mut terminal = ratatui::init();

    // Kitty keyboard protocol: lets us tell Shift+Enter apart from Enter.
    // Terminals without it (plain xterm, tmux by default...) simply don't report
    // the modifier; Alt+Enter is the fallback binding.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    let result = run(&mut terminal).await;

    if enhanced {
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    result
}

async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    event::spawn_input(tx.clone());
    event::spawn_ticker(tx.clone());

    let mut app = App::new(Config::default(), tx);
    app.refresh_profile();

    let mut dirty = true;
    while !app.should_quit {
        if dirty {
            terminal.draw(|f| ui::draw(f, &mut app))?;
        }
        let Some(ev) = rx.recv().await else { break };
        dirty = app.handle(ev);
        // Drain whatever else is already queued so we render once per burst.
        while let Ok(ev) = rx.try_recv() {
            dirty |= app.handle(ev);
        }
    }
    Ok(())
}
