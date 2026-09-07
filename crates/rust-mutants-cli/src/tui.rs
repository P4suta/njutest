// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading a stored run at a terminal: the outcomes on the left, and on the right whichever of the mutant, its source, the findings, or the keys the reader asked for.
//!
//! The drawing is a pure function of what is being browsed, so a test draws
//! into a buffer and compares it, and the terminal loop below is only the part
//! that reads keys and swaps screens. Keys arrive as [`Key`] rather than as
//! `char`, because a browser that could not tell `Escape` from a letter could
//! not have a search box.

use std::collections::BTreeMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::report::run::{RunDocument, RunMutantDocument};
use crate::report::sources::Held;

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

/// The outcomes a digit goes straight to, in the order the digits name them.
pub const SHORTCUTS: [&str; 5] = ["all", "survived", "killed", "not_run", "errored"];

/// How many rows a page moves by.
const PAGE: usize = 10;

/// How many lines of source are shown either side of a mutation.
const AROUND: u32 = 12;

/// What a keypress does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep reading.
    Continue,
    /// Put the terminal back and end.
    Quit,
}

/// One key, as the browser reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character key.
    Char(char),
    /// Move down one.
    Down,
    /// Move up one.
    Up,
    /// Move down a page.
    PageDown,
    /// Move up a page.
    PageUp,
    /// Go to the first.
    Home,
    /// Go to the last.
    End,
    /// Take one character back out of the search.
    Backspace,
    /// Leave the search with what it holds.
    Enter,
    /// Leave the search with nothing in it, or put a pane away.
    Escape,
}

/// Which pane the reader is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    /// The mutant the reader is on.
    #[default]
    Mutants,
    /// The file it is in, around the mutation.
    Source,
    /// Everything that stops the run from being clean.
    Findings,
    /// The keys.
    Help,
}

/// What the reader has narrowed the rows to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Which of [`OUTCOMES`] is wanted.
    outcome: usize,
    /// The text a row has to hold somewhere.
    query: String,
    /// Whether the query is being typed rather than only applied.
    typing: bool,
}

impl Filter {
    /// Whether this row is one the reader asked for.
    #[must_use]
    pub fn admits(&self, mutant: &RunMutantDocument) -> bool {
        let wanted = OUTCOMES.get(self.outcome).copied().unwrap_or("all");
        if wanted != "all" && mutant.outcome != wanted {
            return false;
        }
        if self.query.is_empty() {
            return true;
        }
        let wanted = self.query.to_lowercase();
        [
            &mutant.path,
            &mutant.rule,
            &mutant.family,
            &mutant.display_id,
            &mutant.id,
            &mutant.outcome,
        ]
        .into_iter()
        .any(|field| field.to_lowercase().contains(&wanted))
    }
}

/// One run, as somebody is reading it.
#[derive(Debug, Clone)]
pub struct Browser {
    document: RunDocument,
    sources: BTreeMap<String, Held>,
    selected: usize,
    filter: Filter,
    pane: Pane,
    yanked: Option<String>,
}

impl Browser {
    /// A browser of `document`, at its first mutant, with the sources it can show.
    #[must_use]
    pub const fn of(document: RunDocument, sources: BTreeMap<String, Held>) -> Self {
        Self {
            document,
            sources,
            selected: 0,
            filter: Filter {
                outcome: 0,
                query: String::new(),
                typing: false,
            },
            pane: Pane::Mutants,
            yanked: None,
        }
    }

    /// The mutants the current filter admits, in catalog order.
    #[must_use]
    pub fn shown(&self) -> Vec<&RunMutantDocument> {
        self.document
            .mutants
            .iter()
            .filter(|mutant| self.filter.admits(mutant))
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
        OUTCOMES.get(self.filter.outcome).copied().unwrap_or("all")
    }

    /// What the reader is searching for.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.filter.query
    }

    /// Which pane is on the right.
    #[must_use]
    pub const fn pane(&self) -> Pane {
        self.pane
    }

    /// The identity the reader asked to take away, when they asked for one.
    #[must_use]
    pub fn yanked(&self) -> Option<&str> {
        self.yanked.as_deref()
    }

    /// Moves to the next mutant the filter admits.
    pub fn down(&mut self) {
        self.step(1);
    }

    /// Moves to the previous one.
    pub const fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Moves `by` rows down, stopping at the last.
    pub fn step(&mut self, by: usize) {
        let last = self.shown().len().saturating_sub(1);
        self.selected = self.selected.saturating_add(by).min(last);
    }

    /// Goes to the first row the filter admits.
    pub const fn first(&mut self) {
        self.selected = 0;
    }

    /// Goes to the last row the filter admits.
    pub fn last(&mut self) {
        self.selected = self.shown().len().saturating_sub(1);
    }

    /// Narrows to the next outcome, and starts again at the first mutant it admits.
    pub const fn narrow(&mut self) {
        let next = self.filter.outcome.saturating_add(1);
        self.filter.outcome = if next < OUTCOMES.len() { next } else { 0 };
        self.selected = 0;
    }

    /// Narrows straight to `outcome`, when it is one this browser knows.
    pub fn narrow_to(&mut self, outcome: &str) {
        if let Some(at) = OUTCOMES.iter().position(|one| *one == outcome) {
            self.filter.outcome = at;
            self.selected = 0;
        }
    }

    /// Shows `pane`, or the mutant again when it is already showing.
    pub fn show(&mut self, pane: Pane) {
        self.pane = if self.pane == pane {
            Pane::Mutants
        } else {
            pane
        };
    }

    /// Takes the identity of the mutant the reader is on, to hand back when they leave.
    pub fn yank(&mut self) {
        self.yanked = self.current().map(|mutant| mutant.id.clone());
    }
}

/// What one keypress does.
#[must_use]
pub fn pressed(browser: &mut Browser, key: Key) -> Flow {
    if browser.filter.typing {
        return typing(browser, key);
    }
    match key {
        Key::Char('q') => return Flow::Quit,
        Key::Escape => {
            if browser.pane == Pane::Mutants {
                return Flow::Quit;
            }
            browser.pane = Pane::Mutants;
        }
        Key::Char('j') | Key::Down => browser.down(),
        Key::Char('k') | Key::Up => browser.up(),
        Key::PageDown => browser.step(PAGE),
        Key::PageUp => browser.selected = browser.selected.saturating_sub(PAGE),
        Key::Home => browser.first(),
        Key::End => browser.last(),
        Key::Char('f') => browser.narrow(),
        Key::Char('r') => browser.show(Pane::Mutants),
        Key::Char('p') => browser.show(Pane::Source),
        Key::Char('F') => browser.show(Pane::Findings),
        Key::Char('?') => browser.show(Pane::Help),
        Key::Char('y') => browser.yank(),
        Key::Char('/') => {
            browser.filter.typing = true;
            browser.filter.query.clear();
        }
        Key::Char(digit) if digit.is_ascii_digit() => {
            if let Some(wanted) = digit
                .to_digit(10)
                .and_then(|one| usize::try_from(one).ok())
                .and_then(|one| one.checked_sub(1))
                .and_then(|one| SHORTCUTS.get(one))
            {
                browser.narrow_to(wanted);
            }
        }
        Key::Char(_) | Key::Backspace | Key::Enter => {}
    }
    Flow::Continue
}

/// What one keypress does while the search is being typed, where every character is a character.
fn typing(browser: &mut Browser, key: Key) -> Flow {
    match key {
        Key::Char(character) => browser.filter.query.push(character),
        Key::Backspace => {
            let _dropped = browser.filter.query.pop();
        }
        Key::Enter => browser.filter.typing = false,
        Key::Escape => {
            browser.filter.typing = false;
            browser.filter.query.clear();
        }
        Key::Down | Key::Up | Key::PageDown | Key::PageUp | Key::Home | Key::End => {}
    }
    browser.selected = 0;
    Flow::Continue
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
    match browser.pane {
        Pane::Mutants => detail(frame, browser, right),
        Pane::Source => source(frame, browser, right),
        Pane::Findings => findings(frame, browser, right),
        Pane::Help => help(frame, right),
    }
    frame.render_widget(Paragraph::new(Line::from(status(browser))), bottom);
}

/// The line at the foot: what is being typed, or what the keys are.
fn status(browser: &Browser) -> Span<'static> {
    if browser.filter.typing {
        return Span::styled(
            format!(" search: {}_ ", browser.filter.query),
            Style::default().fg(Color::Yellow),
        );
    }
    let searching = if browser.filter.query.is_empty() {
        String::new()
    } else {
        format!("search {:?} · ", browser.filter.query)
    };
    let taken = if browser.yanked.is_some() {
        "yanked · "
    } else {
        ""
    };
    Span::styled(
        format!(" {searching}{taken}j/k move · / search · f filter · p source · ? keys · q quit "),
        Style::default().fg(Color::DarkGray),
    )
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
    let title = format!(
        " mutants: {} ({}) ",
        browser.narrowing(),
        browser.shown().len()
    );
    let mut state = ListState::default();
    state.select((!items.is_empty()).then_some(browser.selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
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
                Line::from(format!("noticed   {}", mutant.killed_by.join(", "))),
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

fn source(frame: &mut Frame<'_>, browser: &Browser, area: Rect) {
    let title = browser.current().map_or_else(
        || " source ".to_owned(),
        |mutant| format!(" {} ", mutant.path),
    );
    let text = browser.current().map_or_else(
        || vec![Line::from("nothing here")],
        |mutant| match browser.sources.get(&mutant.path) {
            Some(Held::Measured(held)) => listing(held, mutant),
            Some(Held::Changed) => vec![Line::from(
                "this file changed since the run, so what it holds now is not what was measured",
            )],
            None => vec![Line::from(
                "this tree does not hold the file the run measured",
            )],
        },
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

/// The file around one mutation, with the line it is on marked.
fn listing(held: &str, mutant: &RunMutantDocument) -> Vec<Line<'static>> {
    let first = mutant.line.saturating_sub(AROUND).max(1);
    let last = mutant.line.saturating_add(AROUND);
    held.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let number = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
            (number >= first && number <= last).then(|| {
                let here = number == mutant.line;
                Line::from(vec![
                    Span::styled(
                        format!("{}{number:>4} ", if here { "→" } else { " " }),
                        Style::default().fg(if here {
                            colour(&mutant.outcome)
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::raw(line.to_owned()),
                ])
            })
        })
        .collect()
}

fn findings(frame: &mut Frame<'_>, browser: &Browser, area: Rect) {
    let text: Vec<Line<'static>> = if browser.document.findings.is_empty() {
        vec![Line::from("nothing was found")]
    } else {
        browser
            .document
            .findings
            .iter()
            .flat_map(|finding| {
                [
                    Line::from(Span::styled(
                        finding.kind.clone(),
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(finding.detail.clone()),
                    Line::from(""),
                ]
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" findings ")),
        area,
    );
}

fn help(frame: &mut Frame<'_>, area: Rect) {
    let text: Vec<Line<'static>> = [
        "j / ↓     next mutant",
        "k / ↑     previous mutant",
        "PgDn/PgUp a page at a time",
        "Home/End  the first, the last",
        "/         search path, rule, id",
        "f         cycle the outcome filter",
        "1 - 5     all, survived, killed, not run, errored",
        "r         the mutant",
        "p         the source it is in",
        "F         the findings",
        "?         these keys",
        "y         take the identity away with you",
        "q / Esc   quit",
    ]
    .into_iter()
    .map(|line| Line::from(line.to_owned()))
    .collect();
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" keys ")),
        area,
    );
}

/// Reads one run at the terminal until the reader quits, and hands back what they took.
///
/// # Errors
/// Whatever the terminal refused: an alternate screen it would not swap to,
/// a raw mode it would not enter, a draw it would not accept.
pub fn browse(
    document: RunDocument,
    sources: BTreeMap<String, Held>,
) -> std::io::Result<Option<String>> {
    use ratatui::crossterm::event::{self, Event, KeyEventKind};

    let mut browser = Browser::of(document, sources);
    let mut terminal = ratatui::init();
    let outcome = loop {
        if let Err(error) = terminal.draw(|frame| draw(frame, &browser)) {
            break Err(error);
        }
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                let Some(read) = key_of(key.code) else {
                    continue;
                };
                if pressed(&mut browser, read) == Flow::Quit {
                    break Ok(browser.yanked().map(ToOwned::to_owned));
                }
            }
            Ok(_other) => {}
            Err(error) => break Err(error),
        }
    };
    ratatui::restore();
    outcome
}

/// The key the browser reads, when the terminal sent one it answers to.
const fn key_of(code: ratatui::crossterm::event::KeyCode) -> Option<Key> {
    use ratatui::crossterm::event::KeyCode;
    match code {
        KeyCode::Char(character) => Some(Key::Char(character)),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Up => Some(Key::Up),
        KeyCode::PageDown => Some(Key::PageDown),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Esc => Some(Key::Escape),
        _ => None,
    }
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
