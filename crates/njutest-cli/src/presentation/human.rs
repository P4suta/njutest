// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a person is told, drawn for a terminal.

use std::fmt::Write as _;

use super::{Diagnostic, Excerpt, Headline, Place, Site, Style, Telling, Terminal, Told};

/// What a run has to say, drawn for `terminal`.
#[must_use]
pub fn draw(told: &Told, terminal: Terminal) -> String {
    let mut out = String::new();
    for place in &told.places {
        blind(&mut out, place, terminal);
        out.push('\n');
    }
    for diagnostic in &told.diagnostics {
        said(&mut out, diagnostic, terminal);
        out.push('\n');
    }
    stated(&mut out, told, terminal);
    headline(&mut out, told, terminal);
    out
}

/// What the run could not establish, kept quiet and kept.
///
/// A limitation is not a gap and does not compete with one for a reader's
/// attention, but a run that passed over it silently would be claiming
/// something it did not establish.
fn stated(out: &mut String, told: &Told, terminal: Terminal) {
    if told.limitations.is_empty() {
        return;
    }
    let telling = Telling::of(terminal);
    for limitation in &told.limitations {
        let said = format!(
            "{} {}",
            telling.painted(Style::Limitation, &limitation.name),
            telling.painted(Style::Frame, &limitation.detail)
        );
        let opening = 4;
        let mut lines = super::folded(&said, telling.room(opening)).into_iter();
        if let Some(first) = lines.next() {
            let _written = writeln!(out, "  {} {first}", telling.frame(telling.strokes().beside));
        }
        for rest in lines {
            let _written = writeln!(out, "{:opening$}{rest}", "");
        }
    }
    out.push('\n');
}

/// One item of the source, with every place the tests were blind to marked on it.
///
/// The source is drawn once and the marks go under the lines they are on, so
/// a reader sees the shape of the blindness rather than reconstructing it from
/// a list of entries that each quote one line.
fn blind(out: &mut String, place: &Place, terminal: Terminal) {
    let telling = Telling::of(terminal);
    let strokes = telling.strokes();
    let gutter = place
        .excerpt
        .iter()
        .map(|(line, _text)| line.to_string().len())
        .chain(place.spots.iter().map(|spot| spot.line.to_string().len()))
        .max()
        .unwrap_or(1);
    let counted = match place.spots.len() {
        1 => "1 blind spot".to_owned(),
        many => format!("{many} blind spots"),
    };
    let at = place.spots.first().map_or_else(
        || place.path.clone(),
        |spot| format!("{}:{}", place.path, spot.line),
    );
    let _written = writeln!(
        out,
        "{:gutter$} {} {}   {}   {}",
        "",
        telling.frame(strokes.opening),
        telling.painted(Style::Subject, &place.item),
        telling.painted(
            Style::Frame,
            &telling.linked(&format!("file://{}", place.path), &at)
        ),
        telling.painted(Style::Gap, &counted)
    );
    if let Some(instead) = &place.instead {
        let why = match instead {
            Excerpt::Moved => "the file has changed since the run, so the lines are not shown",
            _ => "the file could not be read, so the lines are not shown",
        };
        let _written = writeln!(
            out,
            "{:gutter$} {} {}",
            "",
            telling.frame(strokes.rule),
            telling.painted(Style::Limitation, why)
        );
    }
    for (line, text) in &place.excerpt {
        let here: Vec<&super::Spot> = place
            .spots
            .iter()
            .filter(|spot| spot.line == *line)
            .collect();
        let _written = writeln!(
            out,
            "{:>gutter$} {} {}",
            telling.painted(Style::Frame, &line.to_string()),
            telling.frame(strokes.rule),
            telling.lit(text, &here)
        );
        for spot in here {
            for marked in telling.spot(spot, gutter, text) {
                let _written = writeln!(
                    out,
                    "{:gutter$} {} {marked}",
                    "",
                    telling.frame(strokes.beside)
                );
            }
        }
    }
    let named: Vec<&str> = place
        .spots
        .iter()
        .map(|spot| spot.locator.as_str())
        .collect();
    if let Some(first) = named.first() {
        let _written = writeln!(
            out,
            "{:gutter$} {} {}",
            "",
            telling.frame(strokes.closing),
            telling.command(&format!("njutest explain {first}"))
        );
    }
}

/// One diagnostic: what it is, where it is, and what to do about it.
fn said(out: &mut String, diagnostic: &Diagnostic, terminal: Terminal) {
    let telling = Telling::of(terminal);
    let _written = writeln!(
        out,
        "{} {}",
        telling.severity(diagnostic.severity, diagnostic.code),
        telling.painted(Style::Subject, &diagnostic.title)
    );
    let gutter = diagnostic
        .at
        .as_ref()
        .map_or(1, |site| site.line.to_string().len());
    if let Some(site) = &diagnostic.at {
        at(out, site, gutter, telling);
    }
    let last = diagnostic
        .notes
        .len()
        .saturating_add(diagnostic.actions.len());
    let mut written = 0usize;
    for note in &diagnostic.notes {
        written = written.saturating_add(1);
        under(out, (gutter, written == last), note, telling);
    }
    let column = diagnostic
        .actions
        .iter()
        .map(|action| super::wide(&action.said))
        .max()
        .unwrap_or_default();
    for action in &diagnostic.actions {
        written = written.saturating_add(1);
        let said = format!(
            "{:column$}  {}",
            action.said,
            telling.command(&action.command)
        );
        under(out, (gutter, written == last), &said, telling);
    }
}

/// One line under a diagnostic, folded to the terminal and hung under itself.
///
/// The last one closes the drawing, which is how a reader's eye finds where
/// one diagnostic ends and the next begins without counting blank lines.
fn under(out: &mut String, (gutter, last): (usize, bool), text: &str, telling: Telling) {
    let strokes = telling.strokes();
    let corner = if last {
        strokes.closing
    } else {
        strokes.branch
    };
    let opening = gutter
        .saturating_add(1)
        .saturating_add(corner.chars().count());
    let mut lines = if text.contains("  ") {
        vec![text.to_owned()]
    } else {
        super::folded(text, telling.room(opening.saturating_add(1)))
    }
    .into_iter();
    if let Some(first) = lines.next() {
        let _written = writeln!(out, "{:gutter$} {} {first}", "", telling.frame(corner));
    }
    for rest in lines {
        let _written = writeln!(out, "{:opening$} {rest}", "");
    }
}

/// Where a diagnostic is, and the line it is on, under a mark.
fn at(out: &mut String, site: &Site, gutter: usize, telling: Telling) {
    let strokes = telling.strokes();
    let _written = writeln!(
        out,
        "{:gutter$} {}{}",
        "",
        telling.frame(strokes.opening),
        telling.where_at(site)
    );
    match &site.excerpt {
        Excerpt::Read(line) => {
            let rule = telling.frame(strokes.rule);
            let beside = telling.frame(strokes.beside);
            let _written = writeln!(out, "{:gutter$} {rule}", "");
            let _written = writeln!(
                out,
                "{} {rule} {line}",
                telling.painted(Style::Frame, &site.line.to_string())
            );
            for caret in telling.caret(site, gutter, line) {
                let _written = writeln!(out, "{:gutter$} {beside} {caret}", "");
            }
        }
        Excerpt::Moved => aside(
            out,
            gutter,
            "the file has changed since the run, so the line is not shown",
            telling,
        ),
        Excerpt::Unreadable => aside(
            out,
            gutter,
            "the file could not be read, so the line is not shown",
            telling,
        ),
    }
}

/// Why a line is not being drawn, where a line would have been.
fn aside(out: &mut String, gutter: usize, why: &str, telling: Telling) {
    let _written = writeln!(
        out,
        "{:gutter$} {} {}",
        "",
        telling.frame(telling.strokes().rule),
        telling.painted(Style::Limitation, why)
    );
}

/// The answer, and what it is made of, in as few lines as it takes.
fn headline(out: &mut String, told: &Told, terminal: Terminal) {
    let telling = Telling::of(terminal);
    let Headline {
        verdict,
        killed,
        survived,
        unreached,
        duration_ms,
        kept,
        ..
    } = &told.headline;
    let seconds = as_secs(*duration_ms);
    let counted = telling.painted(
        Style::Frame,
        &format!("{killed} killed, {survived} survived, {unreached} unreached   {seconds}"),
    );
    let _written = writeln!(out, "  {}   {counted}", telling.verdict(*verdict));
    let kept = telling.painted(Style::Frame, kept);
    if told.places.is_empty()
        && !told
            .diagnostics
            .iter()
            .any(|one| one.severity == super::Severity::Gap)
    {
        let _written = writeln!(out, "  {kept}");
        return;
    }
    let _written = writeln!(
        out,
        "  {}   {kept}",
        telling.painted(Style::Gap, &gaps(told))
    );
}

/// How many gaps there are and how many files they are in, which is what a reader wants before the list.
fn gaps(told: &Told) -> String {
    let count = told
        .places
        .iter()
        .map(|place| place.spots.len())
        .sum::<usize>()
        .saturating_add(
            told.diagnostics
                .iter()
                .filter(|one| one.severity == super::Severity::Gap)
                .count(),
        );
    let files: std::collections::BTreeSet<&str> = told
        .places
        .iter()
        .map(|place| place.path.as_str())
        .chain(
            told.diagnostics
                .iter()
                .filter(|one| one.severity == super::Severity::Gap)
                .filter_map(|one| one.at.as_ref())
                .map(|site| site.path.as_str()),
        )
        .collect();
    let files = files.len().max(1);
    format!(
        "{count} gap{} in {files} file{}",
        if count == 1 { "" } else { "s" },
        if files == 1 { "" } else { "s" }
    )
}

/// A duration as a person reads one, to a tenth of a second.
///
/// Counted rather than divided as a float: this is read, not computed with,
/// and integer arithmetic is exact at every length a run can be.
fn as_secs(duration_ms: u64) -> String {
    let tenths = duration_ms.saturating_add(50) / 100;
    format!("{}.{}s", tenths / 10, tenths % 10)
}
