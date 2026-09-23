// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest spec`: what a run established each item pins and leaves free, drawn for a person.

use super::{Style, Telling, Terminal, folded, wide};
use crate::report::{Answered, Discharged, Established, Outcome};
use crate::spec::{
    Answer, Asked, Change, Edit, Free, Held, Item, Pin, Same, Section, Specification, Unsettled,
};

/// How far a change is set in under its heading.
const CHANGE: usize = 4;

/// How far what the run established about a change is set in under it, past where the change's own line continues.
const HELD: usize = 8;

/// How much further a line that did not fit is set in than the line it continues.
const HANGING: usize = 2;

/// What `spec` says about `specification`, drawn for `terminal`.
///
/// A section with nothing in it is not drawn, and each change is listed once, under where it stands across every build; what each build established is on the lines beneath it.
#[must_use]
pub fn page(specification: &Specification, terminal: Terminal) -> String {
    let telling = Telling::of(terminal);
    let mut page = String::new();
    for line in folded(&headline(specification), telling.room(0)) {
        page.push_str(&telling.painted(Style::Frame, &line));
        page.push('\n');
    }
    for item in specification.items() {
        page.push('\n');
        page.push_str(&drawn(item, telling));
    }
    page
}

/// The line that says what was read, from which run, and how much of the workspace that run asked about.
fn headline(specification: &Specification) -> String {
    let builds = match specification.builds() {
        [one] => format!("1 build, {one}"),
        several => format!("{} builds, {}", several.len(), several.join(", ")),
    };
    let items = match specification.items().len() {
        1 => "1 item".to_owned(),
        count => format!("{count} items"),
    };
    format!(
        "spec of {items} from run {}, {}, {builds}",
        specification.run(),
        crate::spec::kind(specification.kind())
    )
}

/// One item, with each section that has something in it.
fn drawn(item: &Item, telling: Telling) -> String {
    let mut drawn = format!(
        "{}  {}\n",
        telling.painted(Style::Subject, item.path()),
        telling.painted(Style::Subject, item.name())
    );
    for section in Section::ALL {
        let changes: Vec<&Change> = item
            .changes()
            .iter()
            .filter(|change| change.section() == section)
            .collect();
        if changes.is_empty() {
            continue;
        }
        drawn.push_str("  ");
        drawn.push_str(&telling.painted(painted(section), heading(section)));
        drawn.push('\n');
        for change in changes {
            drawn.push_str(&listed(change, telling));
        }
    }
    drawn
}

/// The heading a section is drawn under.
#[must_use]
pub const fn heading(section: Section) -> &'static str {
    match section {
        Section::Pinned => "what is pinned",
        Section::Free => "what is left free",
        Section::Same => "what is the same program",
        Section::Unsettled => "what the run could not tell",
    }
}

/// How a section's heading is painted, which is what it is worth to a reader.
const fn painted(section: Section) -> Style {
    match section {
        Section::Pinned => Style::Well,
        Section::Free => Style::Gap,
        Section::Same => Style::Frame,
        Section::Unsettled => Style::Limitation,
    }
}

/// One change: what it did, the command that asks about it, and what each build established.
///
/// The command is the locator a reader types, and it is never folded: a command broken across two lines is one nobody can select, so where it does not fit beside the change it has a line of its own.
fn listed(change: &Change, telling: Telling) -> String {
    let edit = format!("- {}", edited(change.edit(), telling));
    let command = telling.command(&format!("njutest explain {}", change.locator()));
    let together = format!("{edit}  {command}");
    let mut listed = if wide(&together) <= telling.room(CHANGE) {
        format!("{}{together}\n", " ".repeat(CHANGE))
    } else {
        let mut apart = set_in(&edit, CHANGE, telling);
        apart.push_str(&" ".repeat(CHANGE.saturating_add(HANGING)));
        apart.push_str(&command);
        apart.push('\n');
        apart
    };
    let said: Vec<(&str, String)> = change
        .answers()
        .map(|answer| (answer.build(), established(answer)))
        .collect();
    let agreed = said
        .iter()
        .all(|(_, one)| said.first().is_some_and(|(_, first)| first == one));
    match (agreed, said.first()) {
        (true, Some((_, one))) => listed.push_str(&set_in(one, HELD, telling)),
        (true, None) | (false, _) => {
            for (build, one) in &said {
                listed.push_str(&set_in(&format!("{build}: {one}"), HELD, telling));
            }
        }
    }
    listed
}

/// `text` set in by `indent`, folded to what is left of the terminal, with every line after the first set in a little further.
fn set_in(text: &str, indent: usize, telling: Telling) -> String {
    let hanging = indent.saturating_add(HANGING);
    let mut set = String::new();
    for (at, line) in folded(text, telling.room(hanging)).into_iter().enumerate() {
        set.push_str(&" ".repeat(if at == 0 { indent } else { hanging }));
        set.push_str(&line);
        set.push('\n');
    }
    set
}

/// What a change did, as a reader reads it at a glance.
fn edited(edit: Edit<'_>, telling: Telling) -> String {
    match edit {
        Edit::Replaced { was, now } => format!(
            "{} {} {}",
            glanced(was, telling),
            telling.strokes().becomes,
            telling.painted(Style::Changed, &glanced(now, telling))
        ),
        Edit::Deleted { was } => format!("deleting {}", glanced(was, telling)),
    }
}

/// Code quoted as code when it fits on one line, or its first line quoted and how many more there are.
fn glanced(code: &str, telling: Telling) -> String {
    let mut lines = code.lines();
    let first = lines.next().unwrap_or_default().trim_end();
    let more = lines.count();
    if more == 0 {
        return format!("`{first}`");
    }
    let ellipsis = if telling.terminal().unicode {
        "\u{2026}"
    } else {
        "..."
    };
    let lines = if more == 1 { "line" } else { "lines" };
    format!("`{first}`{ellipsis} ({more} more {lines})")
}

/// What one build established, and where it was established when that was another run.
fn established(answer: Answer<'_>) -> String {
    let said = worded(&answer.held());
    match answer.established() {
        Established::Here => said,
        Established::ReadBackFrom(run) => format!("{said}; established by run {run}"),
    }
}

/// What one build established about a change, in the words that are true of it.
#[must_use]
pub fn worded(held: &Held) -> String {
    match held {
        Held::Pinned(pin) => pinned(pin),
        Held::Free { free, accepted } => {
            let said = left(free);
            if *accepted {
                format!("{said}; a reviewer accepted it")
            } else {
                said
            }
        }
        Held::Same(Same::Compiled) => "the compiler renders it identically".to_owned(),
        Held::Same(Same::Model) => {
            "the model checker proves the two equal throughout its domain".to_owned()
        }
        Held::Unsettled(unsettled) => unknown(unsettled),
    }
}

/// What noticed a change, and who else was or was not asked.
fn pinned(pin: &Pin) -> String {
    match pin {
        Pin::Tests { by, asked } => {
            let mut parts = vec![format!("{by} notices it")];
            match asked {
                Asked::Here { before, unasked } => {
                    parts.extend(before.iter().map(first));
                    parts.extend(never_asked(unasked));
                }
                Asked::Unrecorded => {}
            }
            parts.join("; ")
        }
        Pin::Types => "the compiler refuses it".to_owned(),
        Pin::Model => "the model checker finds an input that tells the two apart".to_owned(),
    }
}

/// What a target asked before the one that noticed answered.
fn first(answered: &Answered) -> String {
    if answered.outcome == Outcome::Survived {
        return format!("{} ran it first and did not notice", answered.target);
    }
    format!(
        "{} was asked first and answered {}",
        answered.target,
        answered.outcome.name()
    )
}

/// The targets that reach a change and were never asked, because one asked before them noticed.
fn never_asked(unasked: &[String]) -> Option<String> {
    match unasked {
        [] => None,
        [one] => Some(format!(
            "1 more target reaches it and was never asked: {one}"
        )),
        several => Some(format!(
            "{} more targets reach it and were never asked: {}",
            several.len(),
            several.join(", ")
        )),
    }
}

/// Why nothing noticed a change that makes a different program.
fn left(free: &Free) -> String {
    match free {
        Free::Ran { answered, removed } => {
            let mut parts = vec![match answered.as_slice() {
                [one] => format!("{one} ran it and did not notice"),
                several => format!("{} ran it and none noticed", several.join(", ")),
            }];
            parts.extend(
                removed
                    .iter()
                    .map(|one| format!("{} removed {}", one.proof, one.target)),
            );
            parts.join("; ")
        }
        Free::Removed(removed) => format!(
            "{}, so nothing ran it; check the proof, not the tests",
            every_removed(removed)
        ),
        Free::Never => "nothing the suite runs executes it".to_owned(),
        Free::Unrecorded => {
            "nothing noticed it, and the record does not say what ran it".to_owned()
        }
    }
}

/// What removed every target that reaches a change.
fn every_removed(removed: &[Discharged]) -> String {
    let targets: Vec<&str> = removed.iter().map(|one| one.target.as_str()).collect();
    match removed {
        [one, rest @ ..] if rest.iter().all(|other| other.proof == one.proof) => format!(
            "{} removed every target that reaches it ({})",
            one.proof,
            targets.join(", ")
        ),
        several => format!(
            "{}, which is every target that reaches it",
            several
                .iter()
                .map(|one| format!("{} removed {}", one.proof, one.target))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Why a build established nothing about a change.
fn unknown(unsettled: &Unsettled) -> String {
    match unsettled {
        Unsettled::StepLimit { on, observed } => {
            format!("{on} crossed its step allowance at {observed} without a verdict")
        }
        Unsettled::Waited { on } => format!("this machine stopped waiting for {on}"),
        Unsettled::Unconfirmed { on } => format!("{on} did not answer the same way twice"),
        Unsettled::Errored { on } => format!("{on} could not be measured"),
    }
}
