//! Rendering only. Reads `App`, never changes its logic state.

use std::collections::HashSet;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Level, Mode, Pane, SearchStatus};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const LOG_HEIGHT: u16 = 5;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(LOG_HEIGHT),
        Constraint::Length(1),
    ])
    .split(area);
    let cols =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(rows[1]);

    draw_search(f, app, rows[0]);
    draw_installed(f, app, cols[0]);
    draw_results(f, app, cols[1]);
    draw_log(f, app, rows[2]);
    draw_footer(f, app, rows[3]);

    if let Mode::ConfirmRemove(name) = &app.mode {
        draw_confirm(f, name, area);
    }
}

fn spinner(app: &App) -> &'static str {
    SPINNER[app.spinner % SPINNER.len()]
}

fn border(focused: bool) -> Style {
    if focused {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::DarkGray)
    }
}

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    let active = app.mode == Mode::Search;
    let channel = app.channel.label();
    let title = match &app.search_status {
        SearchStatus::Searching => format!(" Search [{channel}] {} ", spinner(app)),
        SearchStatus::Failed(_) => format!(" Search [{channel}] (failed) "),
        SearchStatus::Idle => format!(" Search [{channel}] "),
    };
    let block = Block::bordered().title(title).border_style(border(active));

    let text = if app.query.is_empty() && !active {
        Line::from(Span::styled("press / to search nixpkgs", dim()))
    } else {
        Line::from(app.query.as_str())
    };
    f.render_widget(Paragraph::new(text).block(block), area);

    if active {
        let x = area.x + 1 + app.query.chars().count() as u16;
        f.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y + 1));
    }
}

fn draw_installed(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.pane == Pane::Installed && app.mode != Mode::Search;
    let title = if app.installed_loading {
        format!(" Installed {} ", spinner(app))
    } else {
        format!(" Installed ({}) ", app.installed.len())
    };
    let mut block = Block::bordered().title(title).border_style(border(focused));
    // Show where the selected package came from, lazygit-detail style.
    if let Some(origin) = app.selected_installed().and_then(|p| p.origin.clone()) {
        block = block.title_bottom(Line::from(Span::styled(format!(" {origin} "), dim())));
    }

    if app.installed.is_empty() {
        let msg = if app.installed_loading {
            "loading…"
        } else {
            "nothing installed in this profile"
        };
        f.render_widget(Paragraph::new(Span::styled(msg, dim())).block(block), area);
        return;
    }

    let items: Vec<ListItem> = app
        .installed
        .iter()
        .map(|p| {
            let mut spans = vec![Span::styled(p.name.clone(), Style::new().bold())];
            if let Some(v) = &p.version {
                spans.push(Span::styled(format!(" {v}"), dim()));
            }
            if !p.active {
                spans.push(Span::styled(" (disabled)", Style::new().fg(Color::Yellow)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.installed_state);
}

fn draw_results(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.pane == Pane::Results;
    let block = Block::bordered()
        .title(format!(" Results ({}) ", app.results.len()))
        .border_style(border(focused));

    if app.results.is_empty() {
        let line = match &app.search_status {
            SearchStatus::Failed(e) => {
                Line::from(Span::styled(e.clone(), Style::new().fg(Color::Red)))
            }
            SearchStatus::Searching => Line::from(Span::styled(
                "searching… (first run can take a while)",
                dim(),
            )),
            SearchStatus::Idle
                if app.query.trim().chars().count() < app.config.search_min_chars =>
            {
                Line::from(Span::styled(
                    format!("type at least {} characters", app.config.search_min_chars),
                    dim(),
                ))
            }
            SearchStatus::Idle => Line::from(Span::styled("no results", dim())),
        };
        f.render_widget(
            Paragraph::new(line).block(block).wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = {
        let installed: HashSet<&str> = app.installed.iter().map(|p| p.name.as_str()).collect();
        app.results
            .iter()
            .map(|h| {
                let leaf = h.attr.rsplit('.').next().unwrap_or(&h.attr);
                let mark = if installed.contains(leaf) {
                    Span::styled("✓ ", Style::new().fg(Color::Green))
                } else {
                    Span::raw("  ")
                };
                ListItem::new(Line::from(vec![
                    mark,
                    Span::styled(h.attr.clone(), Style::new().bold()),
                    Span::styled(format!(" {}", h.version), dim()),
                    Span::styled(format!("  {}", h.description), dim()),
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.results_state);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let capacity = area.height.saturating_sub(2) as usize;
    let reserved = usize::from(app.busy.is_some());
    let skip = app
        .log
        .len()
        .saturating_sub(capacity.saturating_sub(reserved));

    let mut lines: Vec<Line> = app
        .log
        .iter()
        .skip(skip)
        .map(|e| {
            let color = match e.level {
                Level::Success => Color::Green,
                Level::Error => Color::Red,
            };
            Line::from(Span::styled(e.text.clone(), Style::new().fg(color)))
        })
        .collect();
    if let Some(label) = &app.busy {
        lines.push(Line::from(Span::styled(
            format!("{} {label}…", spinner(app)),
            Style::new().fg(Color::Yellow),
        )));
    }

    let block = Block::bordered().title(" Log ").border_style(border(false));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints: &[(&str, &str)] = match (&app.mode, app.pane) {
        (Mode::Search, _) => &[
            ("↑/↓", "select"),
            ("Enter", "install"),
            ("S-Enter/Alt-Enter", "install from other channel"),
            ("Ctrl-t", "channel"),
            ("Esc", "done"),
        ],
        (Mode::ConfirmRemove(_), _) => &[("y", "confirm"), ("n", "cancel")],
        (Mode::Normal, Pane::Installed) => &[
            ("/", "search"),
            ("u", "upgrade"),
            ("U", "upgrade all"),
            ("x", "remove"),
            ("r", "refresh"),
            ("c", "channel"),
            ("Tab", "pane"),
            ("q", "quit"),
        ],
        (Mode::Normal, Pane::Results) => &[
            ("/", "search"),
            ("Enter", "install"),
            ("Alt-Enter", "other channel"),
            ("c", "channel"),
            ("U", "upgrade all"),
            ("Tab", "pane"),
            ("q", "quit"),
        ],
    };

    let mut spans = Vec::new();
    for (i, (key, desc)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", dim()));
        }
        spans.push(Span::styled(*key, Style::new().fg(Color::Cyan).bold()));
        spans.push(Span::styled(format!(" {desc}"), dim()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_confirm(f: &mut Frame, name: &str, area: Rect) {
    let rect = centered(area, 52, 5);
    let block = Block::bordered()
        .title(" Remove ")
        .border_style(Style::new().fg(Color::Red));
    let text = vec![
        Line::from(format!("Remove \"{name}\" from your profile?")),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::new().fg(Color::Cyan).bold()),
            Span::raw(" yes  "),
            Span::styled("n", Style::new().fg(Color::Cyan).bold()),
            Span::raw(" no"),
        ]),
    ];
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(text).block(block), rect);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}
