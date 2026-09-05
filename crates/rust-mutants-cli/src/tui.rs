// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a stored run at a terminal: the outcomes on the left, what one mutant is on the right.
//!
//! The drawing is a pure function of what is being browsed, so a test draws
//! into a buffer and compares it, and the terminal loop below is only the
//! part that reads keys and swaps screens.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::report::run::{RunDocument, RunMutantDocument};

/// Every outcome a browser can narrow to, in the order the key cycles them.
pub const OUTCOMES: [&str; 7] = [
    "all",
    "killed",
    "survived",
    "timed_out",
    "inconclusive",
    "errored",
    "not_run",
];

/// What a keypress does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep reading.
    Continue,
    /// Put the terminal back and end.
    Quit,
}

/// One run, as somebody is reading it.
#[derive(Debug, Clone)]
pub struct Browser {
    document: RunDocument,
    selected: usize,
    narrowed: usize,
}

impl Browser {
    /// A browser of `document`, at its first mutant.
    #[must_use]
    pub const fn of(document: RunDocument) -> Self {
        Self {
            document,
            selected: 0,
            narrowed: 0,
        }
    }

    /// The mutants the current filter admits, in catalog order.
    #[must_use]
    pub fn shown(&self) -> Vec<&RunMutantDocument> {
        let wanted = OUTCOMES.get(self.narrowed).copied().unwrap_or("all");
        self.document
            .mutants
            .iter()
            .filter(|mutant| wanted == "all" || mutant.outcome == wanted)
            .collect()
    }

    /// The mutant the reader is on, when the filter admits any.
    #[must_use]
    pub fn current(&self) -> Option<&RunMutantDocument> {
        self.shown().get(self.selected).copied()
    }

    /// What the filter is narrowed to.
    #[must_use]
    pub fn narrowing(&self) -> &'static str {
        OUTCOMES.get(self.narrowed).copied().unwrap_or("all")
    }

    /// Moves to the next mutant the filter admits.
    pub fn down(&mut self) {
        let last = self.shown().len().saturating_sub(1);
        self.selected = self.selected.saturating_add(1).min(last);
    }

    /// Moves to the previous one.
    pub const fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Narrows to the next outcome, and starts again at the first mutant it admits.
    pub const fn narrow(&mut self) {
        let next = self.narrowed.saturating_add(1);
        self.narrowed = if next < OUTCOMES.len() { next } else { 0 };
        self.selected = 0;
    }
}

/// Draws the whole browser.
pub fn draw(frame: &mut Frame<'_>, browser: &Browser) {
    let area = frame.area();
    let [top, middle, bottom] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(area);
    header(frame, browser, top);
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .areas(middle);
    list(frame, browser, left);
    detail(frame, browser, right);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " j/k move · f filter · q quit ",
            Style::default().fg(Color::DarkGray),
        ))),
        bottom,
    );
}

fn header(frame: &mut Frame<'_>, browser: &Browser, area: Rect) {
    let accounting = &browser.document.accounting;
    let score = browser.document.score.as_ref().map_or_else(
        || "no score: the run decided nothing".to_owned(),
        |score| {
            format!(
                "{:.1}%  ({} detected of {} decided)",
                score.value * 100.0,
                score.detected,
                score.decided
            )
        },
    );
    let text = vec![
        Line::from(format!(
            "{}  ·  {}",
            browser.document.run.id, browser.document.workspace.root_name
        )),
        Line::from(format!(
            "{score}  ·  killed {} survived {} not run {}",
            accounting.killed, accounting.survived, accounting.not_run
        )),
    ];
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" run ")),
        area,
    );
}

fn list(frame: &mut Frame<'_>, browser: &Browser, area: Rect) {
    let items: Vec<ListItem<'_>> = browser
        .shown()
        .iter()
        .map(|mutant| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<10} ", short(&mutant.outcome)),
                    Style::default().fg(colour(&mutant.outcome)),
                ),
                Span::raw(format!("{} {}", mutant.display_id, mutant.rule)),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select((!items.is_empty()).then_some(browser.selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" mutants: {} ", browser.narrowing())),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut state,
    );
}

fn detail(frame: &mut Frame<'_>, browser: &Browser, area: Rect) {
    let text = browser.current().map_or_else(
        || vec![Line::from("nothing here")],
        |mutant| {
            vec![
                Line::from(format!("{}:{}:{}", mutant.path, mutant.line, mutant.column)),
                Line::from(format!("{} ({})", mutant.rule, mutant.family)),
                Line::from(""),
                Line::from(format!("{}  →  {}", mutant.original, mutant.replacement)),
                Line::from(""),
                Line::from(format!("outcome   {}", mutant.outcome)),
                Line::from(format!("target    {}", mutant.target)),
                Line::from(format!("duration  {} ms", mutant.duration_ms)),
                Line::from(format!("id        {}", mutant.id)),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" mutant ")),
        area,
    );
}

/// What one keypress does.
#[must_use]
pub fn pressed(browser: &mut Browser, key: char) -> Flow {
    match key {
        'q' => Flow::Quit,
        'j' => {
            browser.down();
            Flow::Continue
        }
        'k' => {
            browser.up();
            Flow::Continue
        }
        'f' => {
            browser.narrow();
            Flow::Continue
        }
        _ => Flow::Continue,
    }
}

/// Reads one run at the terminal until the reader quits.
///
/// # Errors
/// Whatever the terminal refused: an alternate screen it would not swap to,
/// a raw mode it would not enter, a draw it would not accept.
pub fn browse(document: RunDocument) -> std::io::Result<()> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

    let mut browser = Browser::of(document);
    let mut terminal = ratatui::init();
    let outcome = loop {
        if let Err(error) = terminal.draw(|frame| draw(frame, &browser)) {
            break Err(error);
        }
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let pressed_key = match key.code {
                    KeyCode::Char(character) => character,
                    KeyCode::Down => 'j',
                    KeyCode::Up => 'k',
                    KeyCode::Esc => 'q',
                    _ => ' ',
                };
                if pressed(&mut browser, pressed_key) == Flow::Quit {
                    break Ok(());
                }
            }
            Ok(_other) => {}
            Err(error) => break Err(error),
        }
    };
    ratatui::restore();
    outcome
}

/// The outcome, short enough for a column.
fn short(outcome: &str) -> &str {
    match outcome {
        "timed_out" => "timeout",
        "inconclusive" => "inconcl.",
        "not_run" => "not run",
        other => other,
    }
}

/// What an outcome is coloured.
const fn colour(outcome: &str) -> Color {
    match outcome.as_bytes() {
        b"killed" | b"timed_out" => Color::Green,
        b"survived" => Color::Red,
        b"not_run" => Color::Yellow,
        _ => Color::Magenta,
    }
}
