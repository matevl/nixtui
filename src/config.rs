use std::time::Duration;

/// Which nixpkgs channel searches and installs go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Unstable,
}

impl Channel {
    pub fn other(self) -> Self {
        match self {
            Channel::Stable => Channel::Unstable,
            Channel::Unstable => Channel::Stable,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Unstable => "unstable",
        }
    }
}

/// Runtime settings. Hard-coded defaults for now; the natural next step is to
/// load these from `~/.config/nixtui/config.toml`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Flake reference for the stable channel.
    pub stable_ref: String,
    /// Flake reference for the unstable channel.
    pub unstable_ref: String,
    /// Channel selected at startup.
    pub default_channel: Channel,
    /// How long the query must stay unchanged before `nix search` is spawned.
    pub search_debounce: Duration,
    /// Don't search for queries shorter than this (nix search is expensive).
    pub search_min_chars: usize,
    /// Keep at most this many ranked results.
    pub search_max_results: usize,
}

impl Config {
    pub fn flake_ref(&self, channel: Channel) -> &str {
        match channel {
            Channel::Stable => &self.stable_ref,
            Channel::Unstable => &self.unstable_ref,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Resolved through your flake registry, like `nix profile install nixpkgs#foo`.
            stable_ref: "nixpkgs".into(),
            unstable_ref: "github:NixOS/nixpkgs/nixpkgs-unstable".into(),
            default_channel: Channel::Stable,
            search_debounce: Duration::from_millis(300),
            search_min_chars: 2,
            search_max_results: 200,
        }
    }
}
