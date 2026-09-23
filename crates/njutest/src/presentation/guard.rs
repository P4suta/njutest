// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest guard`: one file drawn as a run measured it, each changed line marked with where it stands.

use std::collections::BTreeMap;

use super::spec::{builds, named, painted};
use super::{MeasuredLine, Missing, Strokes, Style, Telling, Terminal, folded};
use crate::spec::{Section, Specification};

/// The file at `path` drawn from `measured`, its changed lines marked, for `terminal`.
///
/// Only lines a run vouches for can be drawn, so the page is made from [`MeasuredLine`]s and from nothing a reader's editor holds now; the mark on a line is where the weakest change that starts on it stands.
#[must_use]
pub fn page(
    specification: &Specification,
    path: &str,
    measured: &[(u32, MeasuredLine)],
    terminal: Terminal,
) -> String {
    let telling = Telling::of(terminal);
    let strokes = telling.strokes();
    let marks: BTreeMap<u32, Section> = specification
        .lines(path)
        .iter()
        .map(|line| (line.number(), line.section()))
        .collect();
    let widest = measured
        .iter()
        .map(|(number, _)| number.to_string().len())
        .max()
        .unwrap_or(1);
    let mut page = String::new();
    let headline = format!(
        "{path} as run {} measured it, {}",
        specification.run(),
        builds(specification)
    );
    for line in folded(&headline, telling.room(0)) {
        page.push_str(&telling.painted(Style::Frame, &line));
        page.push('\n');
    }
    page.push('\n');
    for (number, text) in measured {
        let mark = marks.get(number).map_or_else(
            || " ".to_owned(),
            |section| telling.painted(painted(*section), mark(*section, strokes)),
        );
        let number = telling.painted(Style::Frame, &format!("{number:>widest$}"));
        page.push_str(&format!("{mark} {number}  {}\n", text.text()));
    }
    page.push('\n');
    let legend: Vec<String> = Section::ALL
        .into_iter()
        .map(|section| {
            format!(
                "{} {}",
                telling.painted(painted(section), mark(section, strokes)),
                named(section)
            )
        })
        .collect();
    page.push_str(&legend.join("   "));
    page.push('\n');
    page
}

/// What is said instead of the file at `path` when the run cannot vouch for its bytes, for `terminal`.
///
/// One note for the file, and none of its code: a mark beside a line the run did not measure would say something about a program nobody asked about.
#[must_use]
pub fn unasked(
    specification: &Specification,
    path: &str,
    missing: Missing,
    terminal: Terminal,
) -> String {
    let telling = Telling::of(terminal);
    let said = format!(
        "{} {path} is not yet asked: {}; {}",
        telling.strokes().unasked,
        missing.why(),
        super::until_measured(specification.run())
    );
    let mut note = String::new();
    for line in folded(&said, telling.room(0)) {
        note.push_str(&telling.painted(Style::Limitation, &line));
        note.push('\n');
    }
    note
}

/// What a line standing in `section` is marked with, in `strokes`.
#[must_use]
pub const fn mark(section: Section, strokes: Strokes) -> &'static str {
    match section {
        Section::Pinned => strokes.pinned,
        Section::Free => strokes.free,
        Section::Same => strokes.same,
        Section::Unsettled => strokes.unsettled,
    }
}
