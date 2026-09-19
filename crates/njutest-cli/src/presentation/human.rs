// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a person is told, drawn for a terminal.

use std::fmt::Write as _;

use super::{Diagnostic, Excerpt, Headline, Missing, Place, Site, Style, Telling, Terminal, Told};

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
    heads(out, place, gutter, telling);
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
    wants(out, place, gutter, telling);
}

/// What the item is, where it is, and how much of it the tests do not see.
///
/// The counts go beside the name while there is room for them and under it
/// when there is not, rather than off the edge, where a narrow terminal breaks
/// them wherever they happen to reach.
fn heads(out: &mut String, place: &Place, gutter: usize, telling: Telling) {
    let strokes = telling.strokes();
    let counted = counting(place);
    let at = place.spots.first().map_or_else(
        || place.path.clone(),
        |spot| format!("{}:{}", place.path, spot.line),
    );
    let named = format!(
        "{:gutter$} {} {}   {}",
        "",
        telling.frame(strokes.opening),
        telling.painted(Style::Subject, &place.item),
        telling.painted(
            Style::Frame,
            &telling.linked(&format!("file://{}", place.path), &at)
        )
    );
    let opening = gutter
        .saturating_add(1)
        .saturating_add(strokes.opening.chars().count())
        .saturating_add(1);
    let heading = telling.painted(Style::Gap, &counted);
    if super::wide(&named)
        .saturating_add(3)
        .saturating_add(counted.len())
        <= telling.room(0)
    {
        let _written = writeln!(out, "{named}   {heading}");
    } else {
        let _written = writeln!(out, "{named}");
        for folded in super::folded(&heading, telling.room(opening)) {
            let _written = writeln!(out, "{:opening$}{folded}", "");
        }
    }
    let Some(instead) = &place.instead else {
        return;
    };
    let why = instead.why();
    let opening = gutter
        .saturating_add(1)
        .saturating_add(strokes.rule.chars().count());
    let mut lines = super::folded(why, telling.room(opening.saturating_add(1))).into_iter();
    if let Some(first) = lines.next() {
        let _written = writeln!(
            out,
            "{:gutter$} {} {}",
            "",
            telling.frame(strokes.rule),
            telling.painted(Style::Limitation, &first)
        );
    }
    for rest in lines {
        let _written = writeln!(
            out,
            "{:opening$} {}",
            "",
            telling.painted(Style::Limitation, &rest)
        );
    }
}

/// What this item asks for, and the command that starts each piece of the work.
///
/// A command goes on its own line rather than being folded when it will not
/// fit beside what it answers: a wrapped command is one nobody can select.
fn wants(out: &mut String, place: &Place, gutter: usize, telling: Telling) {
    let strokes = telling.strokes();
    for (at, (asks, first)) in asked(place).into_iter().enumerate() {
        let corner = if at == 0 {
            strokes.closing
        } else {
            strokes.branch
        };
        let command = format!("njutest explain {first}");
        let opening = gutter
            .saturating_add(1)
            .saturating_add(corner.chars().count())
            .saturating_add(1);
        let asked = telling.painted(Style::Gap, asks);
        let typed = telling.command(&command);
        let head = format!("{:gutter$} {} {asked}", "", telling.frame(corner));
        let together = asks
            .len()
            .saturating_add(2)
            .saturating_add(command.len())
            .saturating_add(opening);
        if together <= telling.room(0) {
            let _written = writeln!(out, "{head}  {typed}");
        } else {
            let _written = writeln!(out, "{head}");
            let _written = writeln!(out, "{:opening$}{typed}", "");
        }
    }
}

/// How many places the tests do not see here, and what kinds they are.
///
/// One number over three kinds would say they are one fact. They are one
/// drawing, because they are in one item and a reader reads it once, and they
/// are not one fact, because each kind asks for different work.
fn counting(place: &Place) -> String {
    let mut kinds: Vec<(super::Standing, usize)> = Vec::new();
    for spot in &place.spots {
        match kinds.iter_mut().find(|(kind, _)| *kind == spot.standing) {
            Some((_, count)) => *count = count.saturating_add(1),
            None => kinds.push((spot.standing, 1)),
        }
    }
    kinds
        .into_iter()
        .map(|(kind, count)| format!("{count} {}", kind.counted()))
        .collect::<Vec<String>>()
        .join(", ")
}

/// What this item asks for, once per distinct kind of work, with one place to start on each.
fn asked(place: &Place) -> Vec<(&'static str, &str)> {
    let mut found: Vec<(&'static str, &str)> = Vec::new();
    for spot in &place.spots {
        let asks = spot.standing.asks();
        if !found.iter().any(|(said, _)| *said == asks) {
            found.push((asks, spot.locator.as_str()));
        }
    }
    found
}

/// One diagnostic: what it is, where it is, and what to do about it.
fn said(out: &mut String, diagnostic: &Diagnostic, terminal: Terminal) {
    let telling = Telling::of(terminal);
    let severity = telling.severity(diagnostic.severity, diagnostic.code);
    let opening = super::wide(&severity).saturating_add(1);
    let mut titled = super::folded(&diagnostic.title, telling.room(opening)).into_iter();
    if let Some(first) = titled.next() {
        let _written = writeln!(
            out,
            "{severity} {}",
            telling.painted(Style::Subject, &first)
        );
    }
    for rest in titled {
        let _written = writeln!(
            out,
            "{:opening$}{}",
            "",
            telling.painted(Style::Subject, &rest)
        );
    }
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
    let hung = gutter
        .saturating_add(2)
        .saturating_add(telling.strokes().closing.chars().count());
    for action in &diagnostic.actions {
        written = written.saturating_add(1);
        let together = column
            .saturating_add(2)
            .saturating_add(action.command.len())
            .saturating_add(hung);
        if together <= telling.room(0) {
            let said = format!(
                "{:column$}  {}",
                action.said,
                telling.command(&action.command)
            );
            under(out, (gutter, written == last), &said, telling);
        } else {
            under(out, (gutter, written == last), &action.said, telling);
            let _written = writeln!(out, "{:hung$}{}", "", telling.command(&action.command));
        }
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
        Excerpt::Instead(Missing::Moved) => aside(
            out,
            gutter,
            "the file has changed since the run, so the line is not shown",
            telling,
        ),
        Excerpt::Instead(Missing::Unreadable) => aside(
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
        runaway,
        waited,
        duration_ms,
        kept,
        ..
    } = &told.headline;
    let seconds = as_secs(*duration_ms);
    let ran_away = if *runaway == 0 {
        String::new()
    } else {
        format!("{runaway} never finished  ")
    };
    let stopped_waiting = if *waited == 0 {
        String::new()
    } else {
        format!("{waited} not waited out  ")
    };
    let counted = telling.painted(
        Style::Frame,
        &format!(
            "{killed} killed  {survived} survived  {unreached} unreached  \
             {ran_away}{stopped_waiting}{seconds}"
        ),
    );
    headed(
        out,
        &format!("{}   {counted}", telling.verdict(*verdict)),
        telling,
    );
    let kept = telling.painted(Style::Frame, kept);
    if told.places.is_empty()
        && !told
            .diagnostics
            .iter()
            .any(|one| one.severity == super::Severity::Gap)
    {
        headed(out, &kept, telling);
        return;
    }
    headed(
        out,
        &format!("{}   {kept}", telling.painted(Style::Gap, &gaps(told))),
        telling,
    );
}

/// One line of the headline, indented and folded like everything else.
///
/// A headline that ran past the edge was the one line nothing folded, so a
/// narrow terminal wrapped it wherever it happened to reach and the two halves
/// of a verdict ended up in different places.
fn headed(out: &mut String, text: &str, telling: Telling) {
    let mut lines = super::folded(text, telling.room(2)).into_iter();
    if let Some(first) = lines.next() {
        let _written = writeln!(out, "  {first}");
    }
    for rest in lines {
        let _written = writeln!(out, "    {rest}");
    }
}

/// How many gaps there are, how many files they are in, and how much the run did not settle.
///
/// A place the run could not measure is not a gap in somebody's tests, and a
/// headline that counted it as one would report a busy machine as a hole in
/// their suite. `Standing` keeps the two apart, so this counts rather than
/// remembers (ADR 0023).
fn gaps(told: &Told) -> String {
    let count = told
        .places
        .iter()
        .flat_map(|place| &place.spots)
        .filter(|spot| spot.standing.is_a_gap())
        .count()
        .saturating_add(
            told.diagnostics
                .iter()
                .filter(|one| one.severity == super::Severity::Gap)
                .count(),
        );
    let unsettled = told
        .places
        .iter()
        .flat_map(|place| &place.spots)
        .filter(|spot| !spot.standing.is_a_gap())
        .count();
    let files: std::collections::BTreeSet<&str> = told
        .places
        .iter()
        .filter(|place| place.spots.iter().any(|spot| spot.standing.is_a_gap()))
        .map(|place| place.path.as_str())
        .chain(
            told.diagnostics
                .iter()
                .filter(|one| one.severity == super::Severity::Gap)
                .filter_map(|one| one.at.as_ref())
                .map(|site| site.path.as_str()),
        )
        .collect();
    let unestablished = if unsettled == 0 {
        String::new()
    } else {
        format!("{unsettled} not established")
    };
    if count == 0 {
        return unestablished;
    }
    let found = format!(
        "{count} gap{} in {} file{}",
        if count == 1 { "" } else { "s" },
        files.len().max(1),
        if files.len() == 1 { "" } else { "s" }
    );
    if unsettled == 0 {
        found
    } else {
        format!("{found}, {unestablished}")
    }
}

/// A duration as a person reads one, to a tenth of a second.
///
/// Counted rather than divided as a float: this is read, not computed with,
/// and integer arithmetic is exact at every length a run can be.
fn as_secs(duration_ms: u64) -> String {
    let tenths = duration_ms.saturating_add(50) / 100;
    format!("{}.{}s", tenths / 10, tenths % 10)
}
