// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one round changed about the last one, which is what a watcher is reading for.

use std::collections::BTreeMap;

use super::{Spot, Style, Telling, Terminal, Told};

/// Every gap of a run, under the name that survives the edit the reader just made.
///
/// Keyed on the locator without its line and on what the run replaced, so two gaps of one rule on one item that differ only by line are one entry.
/// That is the trade: a watcher is reading for change, and the second of two identical gaps was never the news.
fn gaps(told: &Told) -> BTreeMap<(&str, &str), &Spot> {
    let mut found = BTreeMap::new();
    for place in &told.places {
        for spot in &place.spots {
            if spot.standing.is_a_gap() {
                found.insert((spot.unmoved(), spot.was.as_str()), spot);
            }
        }
    }
    found
}

/// One gap, named the way the reader will type it back and shown as what the run did.
fn said(spot: &Spot) -> String {
    let was = spot.was.trim();
    let now = spot.now.trim();
    if now.is_empty() {
        return format!("{}   {was} deleted", spot.locator);
    }
    format!("{}   {was} => {now}", spot.locator)
}

/// What changed between `before` and `now`, in the fewest lines that carry it.
///
/// Only gaps in the tests are compared.
/// A measurement the run could not settle is not a gap that the reader closed by editing — it is a run that established nothing, twice, and counting its disappearance as progress is this surface concluding from how it measured (ADR 0023).
#[must_use]
pub fn moved(before: &Told, now: &Told, terminal: Terminal) -> String {
    let telling = Telling::of(terminal);
    let (was, is) = (gaps(before), gaps(now));
    let mut lines: Vec<String> = Vec::new();
    if before.headline.verdict != now.headline.verdict {
        lines.push(format!("  {}\n", telling.verdict(now.headline.verdict)));
    }
    lines.extend(
        was.iter()
            .filter(|(named, _)| !is.contains_key(*named))
            .map(|(_named, spot)| {
                format!(
                    "  {} {}\n",
                    telling.painted(Style::Well, "closed"),
                    telling.painted(Style::Frame, &said(spot))
                )
            }),
    );
    lines.extend(
        is.iter()
            .filter(|(named, _)| !was.contains_key(*named))
            .map(|(_named, spot)| {
                format!(
                    "  {} {}\n",
                    telling.painted(Style::Gap, "new"),
                    telling.painted(Style::Frame, &said(spot))
                )
            }),
    );
    if lines.is_empty() {
        let held = is.len();
        let plural = if held == 1 { "" } else { "s" };
        lines.push(format!(
            "  {}\n",
            telling.painted(
                Style::Frame,
                &format!("no change; {held} gap{plural} still open")
            )
        ));
    }
    lines.concat()
}
