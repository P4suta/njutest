// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every sentence a watched seam can put in front of a person, in one place somebody reads.

use std::fmt::Write as _;

use njutest_cli::config::Contract;
use njutest_cli::report::{Report, RunKind, SeamDecision, SeamRecord};
use njutest_cli::wire::derive::{Fault, derive};
use njutest_cli::wire::rule::Rule;
use njutest_cli::wire::{Exchange, Spoken};

/// What one HTTP round trip said, as a case writes it down.
struct Said {
    /// Where it fell in the order on its seam.
    seq: u64,
    /// What was asked for.
    method: &'static str,
    /// Where it was asked of.
    path: &'static str,
    /// What the upstream answered.
    status: u16,
    /// How much of the answer was body.
    body_bytes: u64,
}

/// One HTTP round trip on the `payments` seam.
fn exchange(said: &Said) -> Exchange {
    let Said {
        seq,
        method,
        path,
        status,
        body_bytes,
    } = *said;
    Exchange {
        capability: "payments".to_owned(),
        seq,
        during: None,
        duration_ms: 12,
        spoken: Spoken::Http {
            method: method.to_owned(),
            path: path.to_owned(),
            status,
            request_bytes: 96,
            response_bytes: 40_u64.saturating_add(body_bytes),
            body_bytes,
            status_line: format!("HTTP/1.1 {status} OK"),
        },
    }
}

/// The exchanges a run of a small service would record.
fn observed() -> Vec<Exchange> {
    vec![
        exchange(&Said {
            seq: 0,
            method: "POST",
            path: "/orders",
            status: 201,
            body_bytes: 64,
        }),
        exchange(&Said {
            seq: 1,
            method: "GET",
            path: "/orders/1",
            status: 200,
            body_bytes: 0,
        }),
        Exchange {
            capability: "cache".to_owned(),
            seq: 0,
            during: None,
            duration_ms: 1,
            spoken: Spoken::Raw {
                request_bytes: 12,
                response_bytes: 4,
            },
        },
    ]
}

/// A report whose seams were decided the way `decide` says.
fn reported(decide: impl Fn(&Fault) -> SeamDecision) -> Report {
    let mut report = Report::new(
        "20260918T090000Z-aaaaaa",
        RunKind::Full,
        Contract::StandardV1,
    );
    let seen = observed();
    for fault in derive(&seen) {
        let named = seen
            .iter()
            .find(|one| one.capability == fault.capability && one.seq == fault.seq);
        let (asked, answered) =
            named.map_or_else(|| (String::new(), None), |one| one.spoken.asked());
        report.seams.push(SeamRecord {
            id: fault.id.clone(),
            capability: fault.capability.clone(),
            seq: fault.seq,
            asked,
            answered,
            rule: fault.rule,
            decision: decide(&fault),
        });
    }
    report
}

/// One way a suite can answer, with the name a reader sees above it.
type Case = (&'static str, fn(&Fault) -> SeamDecision);

/// What a suite that holds nothing up leaves behind.
const fn nothing_noticed(_fault: &Fault) -> SeamDecision {
    SeamDecision::Unnoticed
}

/// What a suite that holds every seam up leaves behind.
fn all_noticed(fault: &Fault) -> SeamDecision {
    if fault.rule == Rule::TruncateResponse {
        SeamDecision::Proved {
            proof: "no-body-to-cut".to_owned(),
        }
    } else {
        SeamDecision::Tests {
            noticed_by: "payments/test/orders".to_owned(),
        }
    }
}

/// What a run that could not put its questions leaves behind.
const fn nothing_asked(_fault: &Fault) -> SeamDecision {
    SeamDecision::Unreached
}

/// What a suite that holds one seam up and not the other leaves behind.
fn some_noticed(fault: &Fault) -> SeamDecision {
    match fault.capability.as_str() {
        "payments" => SeamDecision::Tests {
            noticed_by: "payments/test/orders".to_owned(),
        },
        _ => SeamDecision::Unnoticed,
    }
}

#[test]
fn every_sentence_a_watched_seam_can_say_is_one_somebody_has_looked_at() {
    let cases: [Case; 4] = [
        ("a suite that holds nothing up", nothing_noticed),
        ("a suite that holds every seam up", all_noticed),
        ("a run that could not put its questions", nothing_asked),
        ("one seam held up and one not", some_noticed),
    ];
    let mut out = String::from(
        "Every sentence a watched seam puts in front of a person.\n\
         Written by crates/njutest-cli/tests/wire_gallery.rs; UPDATE_GOLDEN=1 rewrites it.\n",
    );
    for (name, decide) in cases {
        let report = reported(decide);
        let _written = writeln!(out, "\n=== {name} — the specification\n");
        out.push_str(&njutest_cli::report::spec::page(&report));
        let _written = writeln!(out, "\n=== {name} — what each question came to\n");
        for one in &report.seams {
            let _written = writeln!(
                out,
                "  {} seq {} {} {} -> {}{}",
                one.capability,
                one.seq,
                one.rule.name(),
                if one.asked.is_empty() {
                    "(unread protocol)"
                } else {
                    one.asked.as_str()
                },
                one.decision.name(),
                one.decision
                    .by()
                    .map_or_else(String::new, |who| format!(" ({who})"))
            );
        }
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/wire-gallery.golden");
    if let Err(error) = njutest_devkit::golden::golden(&path, out.as_bytes()) {
        panic!("the wire gallery: {error}");
    }
}

#[test]
fn every_question_a_seam_can_license_appears_in_the_gallery() {
    let licensed: std::collections::BTreeSet<&'static str> = derive(&observed())
        .iter()
        .map(|one| one.rule.name())
        .collect();
    let named: Vec<&'static str> = Rule::ALL
        .iter()
        .map(|one| one.name())
        .filter(|name| !licensed.contains(name))
        .collect();
    assert!(
        named.is_empty(),
        "the gallery is the only place a person reads these sentences, so a question \
         it never draws is one nobody has looked at: {named:?}"
    );
}
