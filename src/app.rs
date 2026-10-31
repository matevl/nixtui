//! Application state and all the logic that mutates it. No drawing in here.

use std::collections::VecDeque;

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use tokio::task::JoinHandle;

use crate::config::{Channel, Config};
use crate::event::{Event, NixEvent, Tx};
use crate::nix::{self, is_unfree_error, profile::Package, search::SearchHit};

const LOG_CAPACITY: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Keystrokes go to the search bar.
    Search,
    ConfirmRemove(String),
    /// A command was refused because of an unfree licence; waiting for y/n to retry.
    ConfirmUnfree(Action),
}

/// A mutating `nix profile` command, kept as data so it can be run again
/// (for instance with unfree packages allowed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Install {
        flake: String,
        attr: String,
        channel: Channel,
    },
    Upgrade(String),
    UpgradeAll,
    Remove(String),
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::Install { attr, channel, .. } => {
                format!("install {attr} ({})", channel.label())
            }
            Action::Upgrade(name) => format!("upgrade {name}"),
            Action::UpgradeAll => "upgrade all".into(),
            Action::Remove(name) => format!("remove {name}"),
        }
    }

    async fn run(self, unfree: bool) -> Result<()> {
        match self {
            Action::Install { flake, attr, .. } => {
                nix::profile::install(&flake, &attr, unfree).await
            }
            Action::Upgrade(name) => nix::profile::upgrade(&name, unfree).await,
            Action::UpgradeAll => nix::profile::upgrade_all(unfree).await,
            Action::Remove(name) => nix::profile::remove(&name).await,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Installed,
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchStatus {
    Idle,
    Searching,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: Level,
    pub text: String,
}

pub struct App {
    pub config: Config,
    /// Channel used for searching and for a plain Enter install.
    pub channel: Channel,
    tx: Tx,
    pub should_quit: bool,
    pub mode: Mode,
    pub pane: Pane,

    pub installed: Vec<Package>,
    pub installed_state: ListState,
    pub installed_loading: bool,

    pub query: String,
    pub results: Vec<SearchHit>,
    pub results_state: ListState,
    pub search_status: SearchStatus,
    /// Incremented on every query change; lets us ignore stale results.
    search_seq: u64,
    search_task: Option<JoinHandle<()>>,

    /// Label of the running mutating command (`nix profile` holds a lock, so one at a time).
    pub busy: Option<String>,
    /// The running command and whether it already runs with unfree allowed.
    current: Option<(Action, bool)>,
    pub log: VecDeque<LogEntry>,
    pub spinner: usize,
}

impl App {
    pub fn new(config: Config, tx: Tx) -> Self {
        Self {
            channel: config.default_channel,
            config,
            tx,
            should_quit: false,
            mode: Mode::Normal,
            pane: Pane::Installed,
            installed: Vec::new(),
            installed_state: ListState::default(),
            installed_loading: false,
            query: String::new(),
            results: Vec::new(),
            results_state: ListState::default(),
            search_status: SearchStatus::Idle,
            search_seq: 0,
            search_task: None,
            busy: None,
            current: None,
            log: VecDeque::new(),
            spinner: 0,
        }
    }

    /// True while something is in flight (the UI animates a spinner).
    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
            || self.installed_loading
            || self.search_status == SearchStatus::Searching
    }

    pub fn selected_installed(&self) -> Option<&Package> {
        self.installed_state
            .selected()
            .and_then(|i| self.installed.get(i))
    }

    pub fn selected_hit(&self) -> Option<&SearchHit> {
        self.results_state
            .selected()
            .and_then(|i| self.results.get(i))
    }

    /// Returns whether the screen needs to be redrawn.
    pub fn handle(&mut self, ev: Event) -> bool {
        match ev {
            Event::Key(key) => {
                self.on_key(key);
                true
            }
            Event::Resize => true,
            Event::Tick => {
                if self.is_busy() {
                    self.spinner = self.spinner.wrapping_add(1);
                    true
                } else {
                    false
                }
            }
            Event::Nix(ev) => {
                self.on_nix(ev);
                true
            }
        }
    }

    // ---------------------------------------------------------------- keys

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match self.mode.clone() {
            Mode::Normal => self.on_key_normal(key),
            Mode::Search => self.on_key_search(key),
            Mode::ConfirmRemove(name) => self.on_key_confirm_remove(key, name),
            Mode::ConfirmUnfree(action) => self.on_key_confirm_unfree(key, action),
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.pane = Pane::Results;
            }
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l') => self.toggle_pane(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Home | KeyCode::Char('g') => self.move_selection(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_selection(isize::MAX / 2),
            KeyCode::Char('r') => self.refresh_profile(),
            KeyCode::Char('c') => self.toggle_channel(),
            KeyCode::Char('U') => self.upgrade_all(),
            KeyCode::Char('u') if self.pane == Pane::Installed => self.upgrade_selected(),
            KeyCode::Char('x') | KeyCode::Char('d') if self.pane == Pane::Installed => {
                if let Some(pkg) = self.selected_installed() {
                    self.mode = Mode::ConfirmRemove(pkg.name.clone());
                }
            }
            KeyCode::Enter if self.pane == Pane::Results => {
                self.install_selected(wants_other_channel(&key));
            }
            _ => {}
        }
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // Leaving the bar keeps the query and results so you can still pick one.
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => self.install_selected(wants_other_channel(&key)),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('n') if ctrl => self.move_selection(1),
            KeyCode::Char('p') if ctrl => self.move_selection(-1),
            KeyCode::Char('t') if ctrl => self.toggle_channel(),
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.on_query_changed();
            }
            KeyCode::Char('w') if ctrl => {
                delete_last_word(&mut self.query);
                self.on_query_changed();
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.on_query_changed();
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.query.push(c);
                self.on_query_changed();
            }
            _ => {}
        }
    }

    fn on_key_confirm_remove(&mut self, key: KeyEvent, name: String) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.start(Action::Remove(name), false);
            }
            KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    fn on_key_confirm_unfree(&mut self, key: KeyEvent, action: Action) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.start(action, true);
            }
            KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    /// Switch between stable and unstable; the current query is searched again
    /// on the new channel.
    fn toggle_channel(&mut self) {
        self.channel = self.channel.other();
        if !self.query.trim().is_empty() {
            self.on_query_changed();
        }
    }

    fn toggle_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Installed => Pane::Results,
            Pane::Results => Pane::Installed,
        };
    }

    /// Move the selection of the focused pane; huge deltas jump to the ends.
    fn move_selection(&mut self, delta: isize) {
        let (state, len) = match self.pane {
            Pane::Installed => (&mut self.installed_state, self.installed.len()),
            Pane::Results => (&mut self.results_state, self.results.len()),
        };
        if len == 0 {
            state.select(None);
            return;
        }
        let current = state.selected().unwrap_or(0) as isize;
        let next = current.saturating_add(delta).clamp(0, len as isize - 1);
        state.select(Some(next as usize));
    }

    // ---------------------------------------------------------- nix actions

    pub fn refresh_profile(&mut self) {
        self.installed_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = nix::profile::list().await;
            let _ = tx.send(Event::Nix(NixEvent::ProfileLoaded(result)));
        });
    }

    fn upgrade_selected(&mut self) {
        let Some(name) = self.selected_installed().map(|p| p.name.clone()) else {
            return;
        };
        self.start(Action::Upgrade(name), false);
    }

    fn upgrade_all(&mut self) {
        self.start(Action::UpgradeAll, false);
    }

    /// Install the selected result from the active channel, or from the other
    /// one when `other_channel` is set (Shift/Alt+Enter).
    fn install_selected(&mut self, other_channel: bool) {
        let Some(attr) = self.selected_hit().map(|h| h.attr.clone()) else {
            return;
        };
        let channel = if other_channel {
            self.channel.other()
        } else {
            self.channel
        };
        let flake = self.config.flake_ref(channel).to_string();
        self.start(
            Action::Install {
                flake,
                attr,
                channel,
            },
            false,
        );
    }

    /// Run a mutating `nix profile` command in the background.
    fn start(&mut self, action: Action, unfree: bool) {
        if let Some(running) = self.busy.clone() {
            self.push_log(Level::Error, format!("busy: `{running}` is still running"));
            return;
        }
        let label = if unfree {
            format!("{} (unfree)", action.label())
        } else {
            action.label()
        };
        self.busy = Some(label.clone());
        self.current = Some((action.clone(), unfree));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = action.run(unfree).await;
            let _ = tx.send(Event::Nix(NixEvent::CommandDone { label, result }));
        });
    }

    // --------------------------------------------------------------- search

    /// Debounced, cancellable search: every keystroke aborts the previous task
    /// (which kills its `nix search` child) and schedules a fresh one.
    fn on_query_changed(&mut self) {
        self.search_seq += 1;
        if let Some(task) = self.search_task.take() {
            task.abort();
        }

        let query = self.query.trim().to_string();
        if query.chars().count() < self.config.search_min_chars {
            self.results.clear();
            self.results_state.select(None);
            self.search_status = SearchStatus::Idle;
            return;
        }

        self.search_status = SearchStatus::Searching;
        let seq = self.search_seq;
        let tx = self.tx.clone();
        let flake = self.config.flake_ref(self.channel).to_string();
        let debounce = self.config.search_debounce;
        let limit = self.config.search_max_results;
        self.search_task = Some(tokio::spawn(async move {
            tokio::time::sleep(debounce).await;
            let result = nix::search::search(&flake, &query, limit).await;
            let _ = tx.send(Event::Nix(NixEvent::SearchDone { seq, result }));
        }));
    }

    // ------------------------------------------------------- background results

    fn on_nix(&mut self, ev: NixEvent) {
        match ev {
            NixEvent::ProfileLoaded(result) => {
                self.installed_loading = false;
                match result {
                    Ok(pkgs) => self.set_installed(pkgs),
                    Err(e) => self.push_log(Level::Error, format!("profile: {e:#}")),
                }
            }
            NixEvent::SearchDone { seq, result } => {
                if seq != self.search_seq {
                    return; // a newer query is already on its way
                }
                self.search_task = None;
                match result {
                    Ok(hits) => {
                        self.results_state
                            .select(if hits.is_empty() { None } else { Some(0) });
                        self.results = hits;
                        self.search_status = SearchStatus::Idle;
                    }
                    Err(e) => self.search_status = SearchStatus::Failed(format!("{e:#}")),
                }
            }
            NixEvent::CommandDone { label, result } => {
                self.busy = None;
                let current = self.current.take();
                match result {
                    Ok(()) => {
                        self.push_log(Level::Success, format!("✓ {label}"));
                        self.refresh_profile();
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        match current {
                            // First attempt refused over the licence: ask, then retry.
                            Some((action, false)) if is_unfree_error(&msg) => {
                                self.mode = Mode::ConfirmUnfree(action);
                            }
                            _ => self.push_log(Level::Error, format!("✗ {label}: {msg}")),
                        }
                    }
                }
            }
        }
    }

    /// Replace the package list while keeping the cursor on the same package.
    fn set_installed(&mut self, pkgs: Vec<Package>) {
        let previous = self.selected_installed().map(|p| p.name.clone());
        let old_index = self.installed_state.selected().unwrap_or(0);
        self.installed = pkgs;
        let index = previous
            .and_then(|name| self.installed.iter().position(|p| p.name == name))
            .or_else(|| {
                (!self.installed.is_empty()).then(|| old_index.min(self.installed.len() - 1))
            });
        self.installed_state.select(index);
    }

    fn push_log(&mut self, level: Level, text: String) {
        for line in text.lines() {
            if self.log.len() >= LOG_CAPACITY {
                self.log.pop_front();
            }
            self.log.push_back(LogEntry {
                level,
                text: line.to_string(),
            });
        }
    }
}

/// Shift+Enter needs a terminal speaking the kitty keyboard protocol (see
/// main.rs); Alt+Enter works almost everywhere and is the fallback.
fn wants_other_channel(key: &KeyEvent) -> bool {
    key.modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
}

fn delete_last_word(s: &mut String) {
    while s.ends_with(' ') {
        s.pop();
    }
    while s.chars().last().is_some_and(|c| c != ' ') {
        s.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_last_word() {
        let mut s = "text editor".to_string();
        delete_last_word(&mut s);
        assert_eq!(s, "text ");
        delete_last_word(&mut s);
        assert_eq!(s, "");
    }
}
