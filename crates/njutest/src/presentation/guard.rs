// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest guard`: one file drawn as a run measured it, each changed line marked with where it stands.

use std::collections::BTreeMap;

use super::spec::{accounts, builds, glance, named, painted};
use super::{MeasuredLine, Missing, Strokes, Style, Telling, Terminal, folded, wide};
use crate::spec::{Item, Line, Section, Specification};

/// How an editor is written for: a font with more than ASCII, no colour of this tool's choosing, and no width to fold to, since the editor wraps.
const EDITOR: Terminal = Terminal {
    width: usize::MAX,
    colour: false,
    unicode: true,
    drawing: true,
};

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
            |section| telling.painted(painted(*section), mark(*section, &strokes)),
        );
        page.push_str(&mark);
        page.push(' ');
        page.push_str(&telling.painted(Style::Frame, &format!("{number:>widest$}")));
        page.push_str("  ");
        page.push_str(text.text());
        page.push('\n');
    }
    page.push('\n');
    let mut legend = String::new();
    for section in Section::ALL {
        let entry = format!(
            "{} {}",
            telling.painted(painted(section), mark(section, &strokes)),
            named(section)
        );
        let joined = wide(&legend)
            .saturating_add(LEGEND_GAP.len())
            .saturating_add(wide(&entry));
        if !legend.is_empty() && joined > telling.room(0) {
            page.push_str(&legend);
            page.push('\n');
            legend.clear();
        }
        if !legend.is_empty() {
            legend.push_str(LEGEND_GAP);
        }
        legend.push_str(&entry);
    }
    page.push_str(&legend);
    page.push('\n');
    page
}

/// What stands between two marks of the legend, which a line breaks at rather than inside one.
const LEGEND_GAP: &str = "   ";

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
pub const fn mark(section: Section, strokes: &Strokes) -> &'static str {
    match section {
        Section::Pinned => strokes.pinned,
        Section::Free => strokes.free,
        Section::Same => strokes.same,
        Section::Unsettled => strokes.unsettled,
    }
}

/// What an editor shows beside a line standing in `section`: its mark and what the mark means.
#[must_use]
pub fn labelled(section: Section) -> String {
    format!(
        "{} {}",
        mark(section, &Telling::of(EDITOR).strokes()),
        named(section)
    )
}

/// What a reader hovering over the mark on `line` is told: each change that starts there, what the builds established about it, and the command that asks about it.
#[must_use]
pub fn told(line: &Line<'_>) -> String {
    line.changes()
        .map(|change| {
            let mut said = vec![glance(change, EDITOR)];
            said.extend(accounts(change));
            said.push(format!("njutest explain {}", change.locator()));
            said.join("\n")
        })
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// How the changes of `item` stand, as a lens above it says: how many in each section that has any.
#[must_use]
pub fn summed(item: &Item) -> String {
    let counted: Vec<String> = Section::ALL
        .into_iter()
        .filter_map(|section| {
            let count = item
                .changes()
                .iter()
                .filter(|change| change.section() == section)
                .count();
            (count > 0).then(|| format!("{count} {}", named(section)))
        })
        .collect();
    format!("{}: {}", item.name(), counted.join(", "))
}
