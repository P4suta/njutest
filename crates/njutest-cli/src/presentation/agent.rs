// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run has to say to something that will act on it without a terminal.

use std::fmt::Write as _;

use super::{Blindness, Place, Spot, Standing, Told, Unsettled};

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
        let _written = writeln!(out, "The source cannot be shown: {}\n", instead.told());
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
        came(spot.standing)
    );
    let _written = write!(
        out,
        "{}",
        match spot.standing {
            Standing::Blind(blindness) => close(blindness, named),
            Standing::Unsettled(unsettled) => settle(unsettled, named),
        }
    );
}

/// What closes a gap in the tests: the test to write, and the command that proves it closed.
///
/// Every branch here offers `replay`, and it is the only function that does.
/// `replay` puts one change back and says whether the suite notices it now,
/// which is a proof only where the run established something to reproduce.
/// Taking `Blindness` rather than `Standing` is what makes that a fact about
/// the program rather than a note for whoever adds the next case (ADR 0023).
fn close(blindness: Blindness, named: &str) -> String {
    let (work, why, explains) = match blindness {
        Blindness::Ran => (
            "**Write a test that fails with this change in place.**",
            "The tests already run this line; what is missing is an assertion about what it \
             decides.",
            "which targets reached it, and what each did",
        ),
        Blindness::Never => (
            "**Write a test that reaches this line at all.**",
            "Asserting harder elsewhere will not close this: nothing executes the line, so \
             nothing can notice anything about it.",
            "why every measured target answered that it does not reach it",
        ),
    };
    let instead = match blindness {
        Blindness::Ran => {
            "If the change cannot affect anything a caller can observe, record that instead"
        }
        Blindness::Never => "If the line is unreachable by construction, record that instead",
    };
    format!(
        "{work} {why}\n\n\
         ```console\n\
         njutest explain {named}   # {explains}\n\
         njutest replay  {named}   # after your test: {}\n\
         ```\n\n\
         {instead}: `njutest accept {named} --reason \"...\"`.\n",
        blindness.replay_proves()
    )
}

/// What a claim nothing decided asks for, which is an investigation and never a test.
///
/// No `replay` anywhere in it, and none available: `Unsettled` has no
/// `replay_proves`, so a branch added here cannot offer one by forgetting not
/// to. Telling something to write a test and then run `replay`, where
/// `replay` can pass without proving anything, is telling it that it
/// succeeded at something it did not do.
fn settle(unsettled: Unsettled, named: &str) -> String {
    let (explains, look) = match unsettled {
        Unsettled::Waited => (
            "what ran, and how long it ran for",
            "Find out why nothing finished — a mutation that does not terminate, a bound that \
             is too tight, a machine that was busy — and run the verification again.",
        ),
        Unsettled::Errored => (
            "what was attempted, and what the harness said",
            "Find out why nothing could be measured — a target that does not build, a harness \
             that did not start — and run the verification again.",
        ),
    };
    format!(
        "**Do not write a test for this yet.** The run established nothing here, so there is \
         no gap to close and no way to tell whether one exists.\n\n\
         ```console\n\
         njutest explain {named}   # {explains}\n\
         ```\n\n\
         {look}\n"
    )
}

/// What came of changing the code there, in the fewest words that are true.
const fn came(standing: Standing) -> &'static str {
    match standing {
        Standing::Blind(Blindness::Ran) => {
            "the tests ran this line and passed anyway, so nothing asserts what it decides"
        }
        Standing::Blind(Blindness::Never) => {
            "no test executes this line at all, so nothing could have noticed"
        }
        Standing::Unsettled(Unsettled::Waited) => {
            "nothing finished, so the run established nothing about it"
        }
        Standing::Unsettled(Unsettled::Errored) => {
            "nothing could be measured, so the run established nothing about it"
        }
    }
}
