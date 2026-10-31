//! One channel carries everything the app reacts to: keys, resizes, ticks and
//! results coming back from background `nix` commands.

use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event as CtEvent, KeyEvent, KeyEventKind};
use tokio::sync::mpsc::UnboundedSender;

use crate::nix::{profile::Package, search::SearchHit};

pub type Tx = UnboundedSender<Event>;

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Resize,
    Tick,
    Nix(NixEvent),
}

/// Results of background work.
#[derive(Debug)]
pub enum NixEvent {
    ProfileLoaded(Result<Vec<Package>>),
    /// `seq` identifies which query produced this; stale results are dropped.
    SearchDone {
        seq: u64,
        result: Result<Vec<SearchHit>>,
    },
    CommandDone {
        label: String,
        result: Result<()>,
    },
}

/// Blocking terminal reads live on their own thread and feed the channel.
pub fn spawn_input(tx: Tx) {
    std::thread::spawn(move || loop {
        // Poll with a timeout so the thread notices when the app has exited.
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => {}
            Ok(false) => {
                if tx.is_closed() {
                    break;
                }
                continue;
            }
            Err(_) => break,
        }
        let Ok(ev) = event::read() else { break };
        let sent = match ev {
            CtEvent::Key(k) if k.kind != KeyEventKind::Release => tx.send(Event::Key(k)),
            CtEvent::Resize(..) => tx.send(Event::Resize),
            _ => continue,
        };
        if sent.is_err() {
            break;
        }
    });
}

/// Drives the spinner. The app only redraws on a tick while something is running.
pub fn spawn_ticker(tx: Tx) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if tx.send(Event::Tick).is_err() {
                break;
            }
        }
    });
}
