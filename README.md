# nixtui

A simple terminal UI to manage the packages in your `nix profile`, in the
spirit of lazygit.

I made it for myself, to stop retyping `nix profile list`, `nix search` and
`nix profile install` all day: see what is installed, upgrade one package or
everything, search nixpkgs as you type, and install with a single key.

> **Heads-up:** this project is vibecoded. It was written with an AI assistant
> (Claude) and has only been tested by using it. Expect rough edges, and read
> the code before trusting it with a profile you care about.

## Install

With flakes enabled:

```sh
# try it
nix run github:matevl/nixtui

# install it
nix profile install github:matevl/nixtui
```

From a checkout:

```sh
nix run .            # or: nix profile install .
nix develop          # dev shell with cargo, clippy, rustfmt, rust-analyzer
cargo run --release
cargo test
```

Requirements: Nix >= 2.20 (`nix-command` and `flakes` are passed to every call,
so you do not need them in your config). `nix` must be in your `PATH`.

## Usage

Start `nixtui`. The left pane lists what is in your profile; the search bar and
results are on the right.

| Key                        | Action                                         |
| -------------------------- | ---------------------------------------------- |
| `/`                        | focus the search bar                           |
| `Enter`                    | install the selected result                    |
| `Shift-Enter`, `Alt-Enter` | install it from the **other** channel          |
| `c` / `Ctrl-t` (in search) | switch channel (stable / unstable)             |
| `u`                        | upgrade the selected package (Installed pane)  |
| `U`                        | upgrade all packages                           |
| `x` / `d`                  | remove the selected package (asks to confirm)  |
| `r`                        | refresh the package list                       |
| `Tab`, `h`, `l`            | switch pane                                    |
| `j` `k` / arrows, `g` `G`  | move, jump to first / last                     |
| `Esc` (in search)          | leave the bar, keep the results                |
| `Ctrl-n` / `Ctrl-p`        | next / previous result while typing            |
| `Ctrl-u` / `Ctrl-w`        | clear / delete last word in the search bar     |
| `q`, `Ctrl-c`              | quit                                           |

`Shift-Enter` only works in terminals that support the kitty keyboard protocol
(kitty, foot, WezTerm, Ghostty, recent Alacritty, ...). Elsewhere it looks like
a plain `Enter`; use `Alt-Enter` instead.

### Channels

The active channel is shown in the search bar title (`Search [stable]`). It
decides which flake is searched and which one a plain `Enter` installs from.
`Shift-Enter` / `Alt-Enter` always installs from the other one. Defaults, in
`src/config.rs`:

| Channel    | Flake reference                           |
| ---------- | ----------------------------------------- |
| `stable`   | `nixpkgs` (resolved by your flake registry) |
| `unstable` | `github:NixOS/nixpkgs/nixpkgs-unstable`   |

If your registry already points `nixpkgs` at unstable, set `stable_ref` to a
release branch such as `github:NixOS/nixpkgs/nixos-25.05`.

### Notes

- Search is debounced (300 ms) and each keystroke cancels the previous
  `nix search`. Results are re-ranked locally (exact name, then prefix, then
  substring, then description) and nested sets such as `python3Packages.*` are
  pushed down.
- The first search after a nixpkgs update evaluates the whole package set and
  can take a while; later ones are cached.
- `nix profile upgrade` only upgrades packages installed from an unlocked flake
  reference (like `nixpkgs#hello`), which is what nixtui uses. Packages you
  installed from a pinned revision will not move.

## Project layout

```
src/
  main.rs        terminal setup/teardown, main loop
  app.rs         state and every state transition (no drawing)
  ui.rs          rendering only
  event.rs       events, input thread, ticker
  config.rs      channels, flake refs, debounce, limits
  nix/
    mod.rs       `nix` subprocess helper
    profile.rs   list / install / upgrade / remove
    search.rs    nix search + parsing
    rank.rs      relevance scoring (pure, unit-tested)
```

## Ideas

- read the configuration from `~/.config/nixtui/config.toml`
- mark outdated packages in the list
- details pane (homepage, license) for the selected result
