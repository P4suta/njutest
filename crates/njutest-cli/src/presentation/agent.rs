// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run has to say to something that will act on it without a terminal.

use std::fmt::Write as _;

use super::{Blindness, Place, Spot, Told};

/// What a run found, as a briefing something can act on.
///
/// Not the record stream, which is a filter's format and says nothing about
/// what to do; not the canonical document, which is an audit of everything and
/// is mostly not about the gaps. What a reader without a screen needs is the
/// code around each gap, what was asked of the tests there, the one command
/// that answers it, and the one command that proves the answer worked — so
/// that acting on this can be checked rather than believed.
///
/// Markdown rather than JSON because the whole of it is instructions, and
/// because a person reviewing what their agent was handed can read it.
#[must_use]
pub fn brief(told: &Told) -> String {
    let mut out = String::new();
    heading(&mut out, told);
    for (at, place) in told.places.iter().enumerate() {
        gap(&mut out, place, at.saturating_add(1));
    }
    if !told.limitations.is_empty() {
        out.push_str("\n## What this run could not establish\n\n");
        for limitation in &told.limitations {
            let _written = writeln!(out, "- `{}` — {}", limitation.name, limitation.detail);
        }
        out.push_str(
            "\nThese are not gaps in the tests. They bound what the verdict above claims.\n",
        );
    }
    out
}

/// What the run concluded, and what is being asked of whoever reads this.
fn heading(out: &mut String, told: &Told) {
    let spots: usize = told.places.iter().map(|place| place.spots.len()).sum();
    let head = &told.headline;
    let _written = writeln!(
        out,
        "# njutest: {} — {spots} place{} the tests do not see\n",
        head.verdict.name(),
        if spots == 1 { "" } else { "s" }
    );
    let _written = writeln!(
        out,
        "A mutation run changed the code in {} places and ran the tests each time. \
         {} of those changes were noticed and {} were not.\n",
        head.cataloged, head.killed, spots
    );
    if spots == 0 {
        out.push_str("There is nothing to do.\n");
        return;
    }
    out.push_str(
        "For each place below, do one of two things:\n\n\
         1. **Write a test that notices the change.** The change is shown; a test that \
            would fail with it in place is a test the suite is missing.\n\
         2. **Record why it is not a gap**, with `njutest accept <name> --reason \"...\"`, \
            if the change cannot affect anything a caller can observe.\n\n\
         Each place says which of the two it wants; they are not interchangeable. Where a \
         `replay` command is given, run it after your change: it puts that one change back \
         to the tests and says whether they notice it now. Where none is given, the run \
         established nothing there and there is nothing for a replay to prove.\n",
    );
}

/// One place the tests do not see, with everything needed to close it.
fn gap(out: &mut String, place: &Place, at: usize) {
    let _written = writeln!(out, "\n## {at}. `{}` in `{}`\n", place.item, place.path);
    if let Some(instead) = &place.instead {
        let _written = writeln!(
            out,
            "The source cannot be shown: {}\n",
            match instead {
                super::Excerpt::Moved =>
                    "the file has changed since the run, so read it yourself before acting",
                _ => "the file could not be read",
            }
        );
    } else {
        out.push_str("```rust\n");
        for (line, text) in &place.excerpt {
            let _written = writeln!(out, "{line:>4} | {text}");
        }
        out.push_str("```\n");
    }
    for spot in &place.spots {
        one(out, spot);
    }
}

/// One change, what came of it, and what to do — which is not the same for all of them.
///
/// A blind spot and a claim nothing could decide want different work, and the
/// loop that proves the work closes only for the first. Telling something to
/// write a test and then run `replay` where `replay` can pass without proving
/// anything is telling it that it succeeded at something it did not do.
fn one(out: &mut String, spot: &Spot) {
    let asked = if spot.now.trim().is_empty() {
        format!("deleting `{}`", spot.was.trim())
    } else {
        format!("`{}` became `{}`", spot.was.trim(), spot.now.trim())
    };
    let named = &spot.locator;
    let _written = writeln!(
        out,
        "\n### `{named}` at line {}\n\nOn line {}, {asked}, and {}.\n",
        spot.line,
        spot.line,
        came(spot.blindness)
    );
    let _written = write!(
        out,
        "{}",
        match spot.blindness {
            Blindness::Ran => format!(
                "**Write a test that fails with this change in place.** The tests already run \
             this line; what is missing is an assertion about what it decides.\n\n\
             ```console\n\
             njutest explain {named}   # which targets reached it, and what each did\n\
             njutest replay  {named}   # after your test: `REPRODUCED` means the gap is still open\n\
             ```\n\n\
             If the change cannot affect anything a caller can observe, record that instead: \
             `njutest accept {named} --reason \"...\"`.\n"
            ),
            Blindness::Never => format!(
                "**Write a test that reaches this line at all.** Asserting harder elsewhere \
             will not close this: nothing executes the line, so nothing can notice \
             anything about it.\n\n\
             ```console\n\
             njutest explain {named}   # why every measured target answered that it does not reach it\n\
             njutest replay  {named}   # after your test: `REPRODUCED` means it is still unreached\n\
             ```\n\n\
             If the line is unreachable by construction, record that instead: \
             `njutest accept {named} --reason \"...\"`.\n"
            ),
            Blindness::Waited => format!(
                "**Do not write a test for this yet.** The run established nothing here, so \
             there is no gap to close and no way to tell whether one exists. `replay` can \
             pass without proving anything, which would say a fix worked when nothing was \
             fixed.\n\n\
             ```console\n\
             njutest explain {named}   # what ran, and how long it ran for\n\
             ```\n\n\
             Find out why nothing finished — a mutation that does not terminate, a bound \
             that is too tight, a machine that was busy — and run the verification again.\n"
            ),
        }
    );
}

/// What came of changing the code there, in the fewest words that are true.
const fn came(blindness: Blindness) -> &'static str {
    match blindness {
        Blindness::Ran => {
            "the tests ran this line and passed anyway, so nothing asserts what it decides"
        }
        Blindness::Never => "no test executes this line at all, so nothing could have noticed",
        Blindness::Waited => "nothing finished, so the run established nothing about it",
    }
}
