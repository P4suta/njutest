// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every shape a person is ever shown, drawn in one file.
//!
//! A rendering is reviewed by looking at it, and nothing else is a review of
//! one. This draws the whole surface — every severity, every verdict, every
//! way a line can fail to be shown — at every terminal the renderer has to
//! answer for, so a change to the output is a diff somebody reads rather than
//! a number that went up. `UPDATE_GOLDEN=1` rewrites it.

#![expect(
    clippy::too_many_lines,
    reason = "the list of cases is a list: one entry per shape a person is shown, and \
              cutting it into functions would hide the thing it exists to show, which is \
              all of them together"
)]

use std::fmt::Write as _;
use std::path::Path;

use njutest_cli::presentation::{
    Action, Blindness, Diagnostic, Excerpt, Headline, Place, Severity, Site, Spot, Stated,
    Terminal, Told, human,
};
use njutest_cli::report::Verdict;

/// The terminals every rendering has to answer for.
const SHAPES: [(&str, Terminal); 4] = [
    (
        "eighty columns, no colour",
        Terminal {
            width: 80,
            colour: false,
            unicode: false,
        },
    ),
    (
        "forty columns, no colour",
        Terminal {
            width: 40,
            colour: false,
            unicode: false,
        },
    ),
    (
        "eighty columns, colour",
        Terminal {
            width: 80,
            colour: true,
            unicode: false,
        },
    ),
    (
        "eighty columns, unicode",
        Terminal {
            width: 80,
            colour: false,
            unicode: true,
        },
    ),
];

/// Where a run of a project that has said nothing about where it writes was kept.
fn kept(run: &str) -> String {
    format!(
        "{}/runs/{run}",
        njutest_cli::config::Config::default()
            .reports
            .directory
            .display()
    )
}

fn headline(verdict: Verdict, killed: u32, survived: u32, unreached: u32) -> Headline {
    Headline {
        verdict,
        project: "fixture-baseline".to_owned(),
        cataloged: killed.saturating_add(survived).saturating_add(unreached),
        killed,
        refused_by_types: 0,
        survived,
        unreached,
        duration_ms: 1911,
        kept: kept("20260101T000000Z-aaaaaa"),
    }
}

const fn refused(mut headline: Headline, types: u32) -> Headline {
    headline.refused_by_types = types;
    headline.cataloged = headline.cataloged.saturating_add(types);
    headline
}

fn site(at: (u32, u32), excerpt: Excerpt, label: &str, width: usize) -> Site {
    Site {
        path: "src/lib.rs".to_owned(),
        line: at.0,
        column: at.1,
        excerpt,
        label: label.to_owned(),
        width,
    }
}

/// One item of the source, with the lines a run would have read.
fn place(item: &str, from: u32, lines: &[&str], spots: Vec<Spot>) -> Place {
    Place {
        item: item.to_owned(),
        path: "src/lib.rs".to_owned(),
        excerpt: lines
            .iter()
            .enumerate()
            .map(|(at, text)| {
                (
                    from.saturating_add(u32::try_from(at).unwrap_or(0)),
                    (*text).to_owned(),
                )
            })
            .collect(),
        instead: None,
        spots,
    }
}

/// One place the tests did not see.
fn spot(at: (u32, u32), change: (&str, &str), blindness: Blindness, locator: &str) -> Spot {
    Spot {
        line: at.0,
        column: at.1,
        was: change.0.to_owned(),
        now: change.1.to_owned(),
        said: blindness.word().to_owned(),
        blindness,
        locator: locator.to_owned(),
    }
}

/// The function every case is about, as a file would hold it.
const SIGN: [&str; 8] = [
    "pub fn sign(n: i32) -> &'static str {",
    "    if n > 0 {",
    "        \"positive\"",
    "    } else if n < 0 {",
    "        \"negative\"",
    "    } else {",
    "        \"zero\"",
    "    }",
];

/// Every case the renderer has to answer for, named as a reader would ask for it.
fn cases() -> Vec<(&'static str, Told)> {
    let told = |places: Vec<Place>, headline: Headline, limitations: Vec<Stated>| Told {
        headline,
        places,
        diagnostics: Vec::new(),
        limitations,
    };
    vec![
        (
            "a run that found nothing",
            told(Vec::new(), headline(Verdict::Assured, 4, 0, 0), Vec::new()),
        ),
        (
            "one item, one blind spot",
            told(
                vec![place(
                    "sign",
                    7,
                    &SIGN[..3],
                    vec![spot(
                        (8, 10),
                        (">", ">="),
                        Blindness::Ran,
                        "src/lib.rs:sign:gt-to-ge@8",
                    )],
                )],
                headline(Verdict::Insufficient, 7, 1, 0),
                Vec::new(),
            ),
        ),
        (
            "one item, every kind of blindness on it at once",
            told(
                vec![place(
                    "sign",
                    7,
                    &SIGN,
                    vec![
                        spot(
                            (8, 10),
                            (">", ">="),
                            Blindness::Ran,
                            "src/lib.rs:sign:gt-to-ge@8",
                        ),
                        spot(
                            (10, 17),
                            ("<", "<="),
                            Blindness::Never,
                            "src/lib.rs:sign:lt-to-le@10",
                        ),
                        spot(
                            (13, 9),
                            ("\"zero\"", "Default::default()"),
                            Blindness::Waited,
                            "src/lib.rs:sign:return-default@13",
                        ),
                    ],
                )],
                headline(Verdict::Insufficient, 7, 2, 1),
                Vec::new(),
            ),
        ),
        (
            "two items of one file",
            told(
                vec![
                    place(
                        "sign",
                        7,
                        &SIGN[..3],
                        vec![spot(
                            (8, 10),
                            (">", ">="),
                            Blindness::Ran,
                            "src/lib.rs:sign:gt-to-ge@8",
                        )],
                    ),
                    place(
                        "double",
                        18,
                        &["pub fn double(n: i32) -> i32 {", "    n * 2", "}"],
                        vec![spot(
                            (19, 7),
                            ("*", "/"),
                            Blindness::Ran,
                            "src/lib.rs:double:mul-to-div@19",
                        )],
                    ),
                ],
                headline(Verdict::Insufficient, 7, 2, 0),
                Vec::new(),
            ),
        ),
        (
            "a file that moved under the run",
            told(
                vec![Place {
                    instead: Some(Excerpt::Moved),
                    excerpt: Vec::new(),
                    ..place(
                        "sign",
                        7,
                        &SIGN[..3],
                        vec![spot(
                            (8, 10),
                            (">", ">="),
                            Blindness::Ran,
                            "src/lib.rs:sign:gt-to-ge@8",
                        )],
                    )
                }],
                headline(Verdict::Insufficient, 7, 1, 0),
                Vec::new(),
            ),
        ),
        (
            "a line whose characters are wider than one byte",
            told(
                vec![place(
                    "heading",
                    2,
                    &[
                        "pub fn heading(count: usize) -> bool {",
                        "    let \u{898b}\u{51fa}\u{3057} = count > 0;",
                        "    \u{898b}\u{51fa}\u{3057}",
                    ],
                    vec![spot(
                        (3, 24),
                        (">", ">="),
                        Blindness::Ran,
                        "src/lib.rs:heading:gt-to-ge@3",
                    )],
                )],
                headline(Verdict::Insufficient, 0, 1, 0),
                Vec::new(),
            ),
        ),
        (
            "what a run could not establish, which is not what it found",
            told(
                Vec::new(),
                headline(Verdict::Assured, 4, 0, 0),
                vec![
                    Stated {
                        name: "git-metadata-unavailable".to_owned(),
                        detail: "git could not be asked, so the run cannot name the commit it \
                                 verified"
                            .to_owned(),
                    },
                    Stated {
                        name: "skipped-test-code".to_owned(),
                        detail: "1 place was not mutated: test-code".to_owned(),
                    },
                ],
            ),
        ),
        (
            "a run that could not proceed",
            Told {
                headline: headline(Verdict::Error, 0, 0, 0),
                places: Vec::new(),
                diagnostics: vec![Diagnostic {
                    severity: Severity::Refusal,
                    code: "NJ5001",
                    title: "the workspace does not compile before anything is instrumented"
                        .to_owned(),
                    at: Some(site(
                        (4, 5),
                        Excerpt::Read("    undefined_function();".to_owned()),
                        "cannot find function `undefined_function` in this scope",
                        18,
                    )),
                    notes: vec![
                        "a run measures a suite that passes, so there is nothing here to measure"
                            .to_owned(),
                    ],
                    actions: vec![Action {
                        said: "see it yourself".to_owned(),
                        command: "cargo test --workspace --no-run".to_owned(),
                    }],
                }],
                limitations: Vec::new(),
            },
        ),
        (
            "the rules a run actually applies, on one function",
            told(
                vec![place(
                    "settle",
                    20,
                    &[
                        "pub fn settle(&mut self, kind: Kind, n: u32) -> Result<u32, Error> {",
                        "    let held = self.store.read()?;",
                        "    self.count += 1;",
                        "    log(\"settling\");",
                        "    let total = held.saturating_add(n);",
                        "    match kind {",
                        "        Kind::Whole if total > 0 => Ok(total),",
                        "        Kind::Part => Ok(total / 2),",
                        "        Kind::None => Err(Error::Empty),",
                        "    }",
                        "}",
                    ],
                    vec![
                        spot(
                            (21, 33),
                            ("?", ".unwrap()"),
                            Blindness::Ran,
                            "src/lib.rs:settle:question-to-unwrap@21",
                        ),
                        spot(
                            (22, 5),
                            ("self.count += 1;", ""),
                            Blindness::Ran,
                            "src/lib.rs:settle:delete-compound-assignment@22",
                        ),
                        spot(
                            (23, 5),
                            ("log(\"settling\");", ""),
                            Blindness::Never,
                            "src/lib.rs:settle:delete-call-statement@23",
                        ),
                        spot(
                            (24, 22),
                            ("saturating_add", "wrapping_add"),
                            Blindness::Ran,
                            "src/lib.rs:settle:saturating-add-to-wrapping-add@24",
                        ),
                        spot(
                            (26, 21),
                            ("if total > 0 ", ""),
                            Blindness::Ran,
                            "src/lib.rs:settle:remove-match-guard@26",
                        ),
                        spot(
                            (27, 9),
                            ("Kind::Part => Ok(total / 2),", ""),
                            Blindness::Never,
                            "src/lib.rs:settle:delete-match-arm@27",
                        ),
                        spot(
                            (28, 23),
                            ("Err(Error::Empty)", "Ok(Default::default())"),
                            Blindness::Ran,
                            "src/lib.rs:settle:return-ok-default@28",
                        ),
                    ],
                )],
                headline(Verdict::Insufficient, 24, 6, 1),
                Vec::new(),
            ),
        ),
        (
            "a part of a catalog",
            told(Vec::new(), headline(Verdict::Partial, 3, 0, 0), Vec::new()),
        ),
        (
            "a target that noticed none of what it was put to",
            Told {
                headline: headline(Verdict::Insufficient, 31, 0, 0),
                places: Vec::new(),
                diagnostics: vec![Diagnostic {
                    severity: Severity::Gap,
                    code: "NJ-HOLLOW-TARGET",
                    title: "this test target noticed none of the changes it was put to".to_owned(),
                    at: None,
                    notes: vec![
                        "core/test/smoke answered about 31 mutations and noticed none \
                         of them"
                            .to_owned(),
                    ],
                    actions: Vec::new(),
                }],
                limitations: Vec::new(),
            },
        ),
        (
            "a seam the suite carried on through",
            Told {
                headline: headline(Verdict::Insufficient, 40, 0, 0),
                places: Vec::new(),
                diagnostics: vec![Diagnostic {
                    severity: Severity::Gap,
                    code: "NJ-WIRE",
                    title: "the suite carried on through what a seam was asked".to_owned(),
                    at: None,
                    notes: vec![
                        "nothing noticed when the run was told to answer 500 where the \
                         upstream answered otherwise, answering POST /orders on the \
                         payments seam"
                            .to_owned(),
                    ],
                    actions: Vec::new(),
                }],
                limitations: Vec::new(),
            },
        ),
        (
            "a run where the type system caught most of it",
            told(
                Vec::new(),
                refused(headline(Verdict::Assured, 12, 0, 0), 31),
                Vec::new(),
            ),
        ),
    ]
}

#[test]
fn every_shape_a_person_is_shown_is_one_somebody_has_looked_at() {
    let mut out = String::from(
        "Every shape njutest draws, at every terminal it answers for.\n\
         Written by crates/njutest-cli/tests/gallery.rs; UPDATE_GOLDEN=1 rewrites it.\n",
    );
    for (name, told) in cases() {
        for (shape, terminal) in SHAPES {
            let _written = writeln!(out, "\n=== {name} — {shape}\n");
            out.push_str(&human::draw(&told, terminal));
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/gallery.golden");
    if let Err(error) = njutest_devkit::golden::golden(&path, out.as_bytes()) {
        panic!("the gallery: {error}");
    }
}
