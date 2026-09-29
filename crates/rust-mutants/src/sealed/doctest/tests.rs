// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use super::{
    Alone, Captured, Expects, Held, Listed, Printed, Uncaptured, captured, listed, printed,
};

const EDITION_2021: &str = "
running 6 tests
test src/lib.rs - add (line 11) - compile ... ok
test src/lib.rs - add (line 15) ... ignored
test src/lib.rs - add (line 19) - compile fail ... ok
test src/lib.rs - add (line 3) ... FAILED
test src/lib.rs - add (line 7) ... ok
test src/lib.rs - sub (line 26) ... FAILED

failures:

---- src/lib.rs - add (line 3) stdout ----
Test executable failed (exit status: 1).

stdout:
rust-mutants-captured 0


---- src/lib.rs - sub (line 26) stdout ----
Test executable failed (exit status: 1).

stdout:
rust-mutants-captured 2



failures:
    src/lib.rs - add (line 3)
    src/lib.rs - sub (line 26)

test result: FAILED. 3 passed; 2 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.42s

";

const EDITION_2024: &str = "rust-mutants-captured 0

running 1 test
test src/lib.rs - add (line 19) - compile fail ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s

all doctests ran in 0.21s; merged doctests compilation took 0.18s
";

const ONLY_MERGED: &str = "rust-mutants-captured 0
all doctests ran in 0.40s; merged doctests compilation took 0.40s
";

const MERGE_REFUSED: &str = "
running 7 tests
test src/lib.rs - add (line 11) - compile ... ok
test src/lib.rs - add (line 15) ... ignored
test src/lib.rs - add (line 19) - compile fail ... ok
test src/lib.rs - add (line 3) ... FAILED
test src/lib.rs - add (line 7) ... ok
test src/lib.rs - mul (line 34) ... FAILED
test src/lib.rs - sub (line 26) ... FAILED

failures:

---- src/lib.rs - add (line 3) stdout ----
Test executable failed (exit status: 1).

stdout:
rust-mutants-captured 0


---- src/lib.rs - mul (line 34) stdout ----
error: not on wasm
  --> src/lib.rs:37:1
   |
37 | compile_error!(\"not on wasm\");
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

error: aborting due to 1 previous error

Couldn't compile the test.
---- src/lib.rs - sub (line 26) stdout ----
Test executable failed (exit status: 1).

stdout:
rust-mutants-captured 2



failures:
    src/lib.rs - add (line 3)
    src/lib.rs - mul (line 34)
    src/lib.rs - sub (line 26)

test result: FAILED. 3 passed; 3 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.43s

all doctests ran in 0.48s; merged doctests compilation took 0.05s
";

const ALL_IN_ONE: &str = "
running 5 tests
test src/lib.rs - add (line 11) - compile ... ok
test src/lib.rs - add (line 15) ... ignored
test src/lib.rs - add (line 3) ... ok
test src/lib.rs - add (line 7) - should panic ... ignored
test src/lib.rs - sub (line 26) ... ok

test result: ok. 3 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s

";

fn held(claims: u64) -> Held {
    Held {
        claims,
        binaries: (0..claims)
            .map(|claim| (claim, PathBuf::from(format!("{claim}.wasm"))))
            .collect(),
    }
}

fn alone(name: &str, claim: u64, expects: Expects) -> Alone {
    Alone {
        name: name.to_owned(),
        binary: PathBuf::from(format!("{claim}.wasm")),
        expects,
    }
}

#[test]
fn doctests_compiled_alone_are_named_by_the_marker_rustdoc_printed_for_them_and_by_their_order() {
    assert_eq!(
        captured(EDITION_2021.as_bytes(), &held(3)),
        Ok(Captured {
            merged: Vec::new(),
            alone: vec![
                alone("src/lib.rs - add (line 3)", 0, Expects::Return),
                alone("src/lib.rs - add (line 7)", 1, Expects::Panic),
                alone("src/lib.rs - sub (line 26)", 2, Expects::Return),
            ],
            unbuilt: Vec::new(),
        }),
        "a doctest rustdoc passed although the capture failed it is one that should panic, and it holds the claim between its neighbours"
    );
}

#[test]
fn a_merged_compilation_is_the_claim_printed_before_rustdoc_announces_its_own() {
    assert_eq!(
        captured(EDITION_2024.as_bytes(), &held(1)),
        Ok(Captured {
            merged: vec![PathBuf::from("0.wasm")],
            alone: Vec::new(),
            unbuilt: Vec::new(),
        })
    );
    assert_eq!(
        captured(ONLY_MERGED.as_bytes(), &held(1)),
        Ok(Captured {
            merged: vec![PathBuf::from("0.wasm")],
            alone: Vec::new(),
            unbuilt: Vec::new(),
        }),
        "rustdoc announces nothing of its own when every doctest merged, and closes with its timing"
    );
}

#[test]
fn a_doctest_that_did_not_build_for_the_sealed_target_is_named_unbuilt() {
    assert_eq!(
        captured(MERGE_REFUSED.as_bytes(), &held(3)),
        Ok(Captured {
            merged: Vec::new(),
            alone: vec![
                alone("src/lib.rs - add (line 3)", 0, Expects::Return),
                alone("src/lib.rs - add (line 7)", 1, Expects::Panic),
                alone("src/lib.rs - sub (line 26)", 2, Expects::Return),
            ],
            unbuilt: vec!["src/lib.rs - mul (line 34)".to_owned()],
        })
    );
}

#[test]
fn a_capture_out_of_the_order_rustdoc_ran_the_doctests_in_is_refused() {
    let swapped = EDITION_2021
        .replace("rust-mutants-captured 0", "rust-mutants-captured x")
        .replace("rust-mutants-captured 2", "rust-mutants-captured 0")
        .replace("rust-mutants-captured x", "rust-mutants-captured 2");
    assert_eq!(
        captured(swapped.as_bytes(), &held(3)),
        Err(Uncaptured::OutOfOrder {
            name: "src/lib.rs - add (line 3)".to_owned(),
            said: 2,
            expected: 0,
        })
    );
}

#[test]
fn a_claim_the_report_does_not_account_for_is_refused() {
    assert_eq!(
        captured(EDITION_2021.as_bytes(), &held(4)),
        Err(Uncaptured::ClaimsDisagree {
            reported: 3,
            held: 4,
        }),
        "a binary rustdoc never named is a doctest nobody knows the name of"
    );
    let mut lost = held(3);
    lost.binaries.remove(&1);
    assert_eq!(
        captured(EDITION_2021.as_bytes(), &lost),
        Err(Uncaptured::Missing { claim: 1 }),
        "a claim whose copy failed holds no binary to run"
    );
}

#[test]
fn a_report_that_does_not_close_or_count_is_refused() {
    assert_eq!(captured(b"", &held(0)), Err(Uncaptured::Unreported));
    assert_eq!(
        captured(b"rust-mutants-captured 0\n", &held(1)),
        Err(Uncaptured::CountsDisagree {
            announced: 0,
            accounted: 0,
        }),
        "a merged run rustdoc never closed may have stopped before it handed on every binary"
    );
    let unclosed = EDITION_2021.replace(
        "test result: FAILED. 3 passed; 2 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.42s",
        "",
    );
    assert_eq!(
        captured(unclosed.as_bytes(), &held(3)),
        Err(Uncaptured::Unclosed { announced: 6 }),
        "a report with no closing line may have stopped before the last doctest, and counts \
         none of them rather than the most a count holds"
    );
}

#[test]
fn a_line_that_is_not_part_of_a_report_of_doctests_is_refused() {
    let interleaved = EDITION_2021.replace(
        "test src/lib.rs - add (line 3) ... FAILED",
        "something else",
    );
    assert_eq!(
        captured(interleaved.as_bytes(), &held(3)),
        Err(Uncaptured::Unread {
            line: "something else".to_owned(),
        })
    );
}

#[test]
fn a_merged_binary_lists_its_doctests_in_index_order_with_what_each_passes_by() {
    assert_eq!(
        listed(ALL_IN_ONE.as_bytes()),
        Some(vec![
            Listed {
                name: "src/lib.rs - add (line 11) - compile".to_owned(),
                expects: Expects::Return,
                ignored: false,
            },
            Listed {
                name: "src/lib.rs - add (line 15)".to_owned(),
                expects: Expects::Return,
                ignored: true,
            },
            Listed {
                name: "src/lib.rs - add (line 3)".to_owned(),
                expects: Expects::Return,
                ignored: false,
            },
            Listed {
                name: "src/lib.rs - add (line 7)".to_owned(),
                expects: Expects::Panic,
                ignored: true,
            },
            Listed {
                name: "src/lib.rs - sub (line 26)".to_owned(),
                expects: Expects::Return,
                ignored: false,
            },
        ])
    );
}

#[test]
fn a_listing_that_filtered_a_doctest_out_or_did_not_close_names_nothing() {
    let filtered = ALL_IN_ONE.replace("0 filtered out", "1 filtered out");
    assert_eq!(
        listed(filtered.as_bytes()),
        None,
        "a doctest filtered out moves every index after it"
    );
    let unclosed = ALL_IN_ONE.replace("test result:", "text result:");
    assert_eq!(listed(unclosed.as_bytes()), None);
    let short = ALL_IN_ONE.replace("running 5 tests", "running 6 tests");
    assert_eq!(listed(short.as_bytes()), None);
}

const STOPPED: &str = "
running 3 tests
test src/lib.rs - after (line 8) ... ok
test src/lib.rs - double (line 17) ... ";

#[test]
fn a_merged_binary_stopped_inside_a_doctest_names_each_it_finished_and_the_one_it_stopped_in() {
    let returning = |name: &str| Listed {
        name: name.to_owned(),
        expects: Expects::Return,
        ignored: false,
    };
    assert_eq!(
        printed(STOPPED.as_bytes()),
        Some(Printed {
            announced: 3,
            finished: vec![returning("src/lib.rs - after (line 8)")],
            stopped: Some(returning("src/lib.rs - double (line 17)")),
            whole: false,
        })
    );
    assert_eq!(
        listed(STOPPED.as_bytes()),
        None,
        "a listing that stopped names only the doctests before the one it stopped in"
    );
    let resumed = format!("{STOPPED}\ntest src/lib.rs - half (line 27) ... ok\n");
    assert_eq!(
        printed(resumed.as_bytes()),
        None,
        "a harness that began another doctest after one it never finished is not one run in order"
    );
}

#[test]
fn doctests_past_a_stopped_listing_are_named_from_the_native_run_in_the_order_rustdoc_indexes_them()
{
    let listed = |name: &str, expects: Expects| Listed {
        name: name.to_owned(),
        expects,
        ignored: false,
    };
    let captured = Captured {
        merged: vec![PathBuf::from("0.wasm")],
        alone: vec![alone("src/lib.rs - kept (line 40)", 1, Expects::Return)],
        unbuilt: vec!["src/lib.rs - unbuilt (line 50)".to_owned()],
    };
    let printed = [
        listed("src/lib.rs - after (line 8)", Expects::Return),
        listed("src/lib.rs - double (line 17)", Expects::Return),
    ];
    let native: Vec<String> = [
        "src/lib.rs - half (line 3)",
        "src/lib.rs - after (line 8)",
        "src/lib.rs - half (line 27) - should panic",
        "src/lib.rs - example (line 60) - compile",
        "src/lib.rs - failing (line 70) - compile fail",
        "src/lib.rs - kept (line 40)",
        "src/lib.rs - unbuilt (line 50)",
        "src/lib.rs - double (line 17)",
    ]
    .map(str::to_owned)
    .to_vec();
    assert_eq!(
        super::unprinted(&native, &captured, &printed),
        Some(vec![
            listed("src/lib.rs - example (line 60) - compile", Expects::Return),
            listed("src/lib.rs - half (line 27)", Expects::Panic),
            listed("src/lib.rs - half (line 3)", Expects::Return),
        ]),
        "a doctest held apart, compiled only, or printed is not past the listing"
    );
    let earlier = [
        native.clone(),
        vec!["src/lib.rs - before (line 1)".to_owned()],
    ]
    .concat();
    assert_eq!(
        super::unprinted(&earlier, &captured, &printed),
        None,
        "a doctest that sorts before one the binary printed is not one this binary holds past it"
    );
    let two = Captured {
        merged: vec![PathBuf::from("0.wasm"), PathBuf::from("2.wasm")],
        ..captured
    };
    assert_eq!(
        super::unprinted(&native, &two, &printed),
        None,
        "two merged binaries leave which one holds a doctest unsaid"
    );
}
