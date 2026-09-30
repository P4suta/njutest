// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use super::{
    Alone, Captured, Expects, Held, Listing, Uncaptured, captured, listed, listing, merged_binaries,
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

/// What a build with `--list` among the doctests' test arguments printed, where one doctest was compiled alone and the rest merged.
const LISTED_MIXED: &str = "rust-mutants-captured 0
src/lib.rs - add (line 19) - compile fail: test

1 test, 0 benchmarks
";

/// What a build with `--list` among the doctests' test arguments printed, where every doctest merged.
const LISTED_MERGED: &str = "rust-mutants-captured 0
all doctests ran in 0.12s; merged doctests compilation took 0.11s
";

/// What a build with `--list` among the doctests' test arguments printed, where two doctests were compiled alone and none merged.
const LISTED_ALONE: &str = "src/lib.rs - add (line 3): test
src/lib.rs - sub (line 26) - should panic: test

2 tests, 0 benchmarks
";

/// What a merged binary built with `--list` among its test arguments prints when it runs with no index.
const MERGED_LISTING: &str = "src/lib.rs - add (line 3): test
src/lib.rs - add (line 7): test
src/lib.rs - sub (line 26): test

3 tests, 0 benchmarks
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
            ignored: Vec::new(),
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
            ignored: Vec::new(),
            merged: vec![PathBuf::from("0.wasm")],
            alone: Vec::new(),
            unbuilt: Vec::new(),
        })
    );
    assert_eq!(
        captured(ONLY_MERGED.as_bytes(), &held(1)),
        Ok(Captured {
            ignored: Vec::new(),
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
            ignored: Vec::new(),
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
fn a_build_with_list_names_the_merged_claims_and_the_doctests_rustdoc_listed_itself() {
    assert_eq!(
        listing(LISTED_MIXED.as_bytes()),
        Ok(Listing {
            merged: vec![0],
            standalone: vec!["src/lib.rs - add (line 19) - compile fail".to_owned()],
        }),
        "rustdoc runs the merged binary through the capture and lists the doctest it did not \
         merge itself, closing with their count"
    );
    assert_eq!(
        listing(LISTED_MERGED.as_bytes()),
        Ok(Listing {
            merged: vec![0],
            standalone: Vec::new(),
        }),
        "where every doctest merged, rustdoc closes with its timing and lists nothing itself"
    );
    assert_eq!(
        listing(LISTED_ALONE.as_bytes()),
        Ok(Listing {
            merged: Vec::new(),
            standalone: vec![
                "src/lib.rs - add (line 3)".to_owned(),
                "src/lib.rs - sub (line 26) - should panic".to_owned(),
            ],
        }),
        "a listing names a doctest that should panic as it names any other, so what each passes \
         by comes from the native run"
    );
}

#[test]
fn a_listing_that_never_closes_or_counts_is_refused() {
    assert_eq!(
        listing(b"rust-mutants-captured 0\n"),
        Err(Uncaptured::CountsDisagree {
            announced: 0,
            accounted: 0,
        }),
        "a merged compilation whose report never closed may have stopped before it handed on \
         every binary"
    );
    let unclosed = LISTED_MIXED.replace("1 test, 0 benchmarks", "");
    assert_eq!(
        listing(unclosed.as_bytes()),
        Err(Uncaptured::CountsDisagree {
            announced: 0,
            accounted: 1,
        }),
        "a doctest named and never counted may have stopped the report after it"
    );
    assert_eq!(listing(b""), Err(Uncaptured::Unreported));
    let short = LISTED_ALONE.replace("2 tests", "3 tests");
    assert_eq!(
        listing(short.as_bytes()),
        Err(Uncaptured::CountsDisagree {
            announced: 3,
            accounted: 2,
        })
    );
}

#[test]
fn a_line_after_a_listing_closed_or_a_claim_after_it_listed_is_refused() {
    let trailing = format!("{LISTED_ALONE}rust-mutants-captured 0\n");
    assert_eq!(
        listing(trailing.as_bytes()),
        Err(Uncaptured::Unread {
            line: "rust-mutants-captured 0".to_owned(),
        }),
        "a binary claimed after rustdoc began listing is not one this report accounts for"
    );
    let trailing = format!("{LISTED_ALONE}one more line\n");
    assert_eq!(
        listing(trailing.as_bytes()),
        Err(Uncaptured::Unread {
            line: "one more line".to_owned(),
        }),
        "a line after the count is no doctest's name"
    );
}

#[test]
fn the_merged_binaries_of_a_listing_are_its_claims_where_none_is_missing_or_beyond_it() {
    assert_eq!(
        merged_binaries(
            &Listing {
                merged: vec![0, 1],
                standalone: Vec::new(),
            },
            &held(2)
        ),
        Ok(vec![PathBuf::from("0.wasm"), PathBuf::from("1.wasm")])
    );
    assert_eq!(
        merged_binaries(&Listing::default(), &held(0)),
        Ok(Vec::new()),
        "a library whose doctests all merged into none claims nothing"
    );
    assert_eq!(
        merged_binaries(
            &Listing {
                merged: vec![1],
                standalone: Vec::new(),
            },
            &held(1)
        ),
        Err(Uncaptured::OutOfOrder {
            name: "a merged compilation".to_owned(),
            said: 1,
            expected: 0,
        })
    );
    let mut lost = held(2);
    lost.binaries.remove(&1);
    assert_eq!(
        merged_binaries(
            &Listing {
                merged: vec![0, 1],
                standalone: Vec::new(),
            },
            &lost
        ),
        Err(Uncaptured::Missing { claim: 1 })
    );
    assert_eq!(
        merged_binaries(
            &Listing {
                merged: vec![0],
                standalone: Vec::new(),
            },
            &held(2)
        ),
        Err(Uncaptured::ClaimsDisagree {
            reported: 1,
            held: 2,
        }),
        "a claim the capture gave out that the report does not account for is a binary nobody \
         knows the doctests of"
    );
}

#[test]
fn a_merged_binary_names_its_doctests_in_index_order_whatever_one_does_when_it_runs() {
    assert_eq!(
        listed(MERGED_LISTING.as_bytes()),
        Some(vec![
            "src/lib.rs - add (line 3)".to_owned(),
            "src/lib.rs - add (line 7)".to_owned(),
            "src/lib.rs - sub (line 26)".to_owned(),
        ]),
        "a listing is the harness's own, so it names every doctest the binary holds whether any \
         of them would refuse, panic or hang when it runs"
    );
}

#[test]
fn a_listing_that_filtered_a_doctest_out_did_not_close_or_names_one_twice_names_nothing() {
    let filtered = MERGED_LISTING.replace("3 tests", "2 tests");
    assert_eq!(
        listed(filtered.as_bytes()),
        None,
        "a count short of the names holds the listing to no index"
    );
    let unclosed = MERGED_LISTING.replace("3 tests, 0 benchmarks", "");
    assert_eq!(listed(unclosed.as_bytes()), None);
    let twice = MERGED_LISTING.replace(
        "src/lib.rs - add (line 7): test",
        "src/lib.rs - add (line 3): test",
    );
    assert_eq!(
        listed(twice.as_bytes()),
        None,
        "one doctest at two indexes is a listing no binary holds"
    );
    assert_eq!(listed(b""), None);
}
