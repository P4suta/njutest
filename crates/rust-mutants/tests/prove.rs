// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a branch proof and a measurement together say about one target.

use std::path::Path;

use rust_mutants::coverage::{Block, Point};
use rust_mutants::prove::discharges;
use rust_mutants::syntax::Position;
use rust_mutants::syntax::branch::Proof;

const fn at(line: u32, column: u32) -> Position {
    Position {
        line,
        byte_column: column,
        char_column: column,
    }
}

fn block(file: &str, line: u32) -> Block {
    Block {
        file: file.into(),
        start: Point { line, column: 1 },
        end: Point { line, column: 80 },
    }
}

/// A body from line 10 to line 20 of `src/lib.rs`.
const fn proof() -> Proof {
    Proof {
        marker: None,
        body_start: at(10, 5),
        body_end: at(20, 5),
    }
}

#[test]
fn discharges_is_a_pure_function_of_the_proof_and_the_covered_regions() {
    let path = Path::new("src/lib.rs");
    assert!(
        discharges(&proof(), path, &[block("src/lib.rs", 30)]),
        "a target during which no block of the body ran cannot have observed a mutation the \
         compiler says changes nothing outside it"
    );
    assert!(
        !discharges(&proof(), path, &[block("src/lib.rs", 12)]),
        "a target that ran a line of the body might have observed it, and might is not a proof"
    );
    assert!(
        discharges(&proof(), path, &[block("src/other.rs", 12)]),
        "a line of another file is not a line of this body"
    );
    assert!(
        discharges(&proof(), path, &[]),
        "a target that covered nothing of this file ran nothing of this body; whether the \
         measurement could read it at all is the caller's premise, not this function's"
    );
}

#[test]
fn a_region_that_only_contains_the_body_says_nothing_about_it() {
    let path = Path::new("src/lib.rs");
    assert!(
        !discharges(&proof(), path, &[block("src/lib.rs", 11)]),
        "a region that begins inside the body is the body's own, and its count says it ran"
    );
    assert!(
        discharges(&proof(), path, &[block("src/lib.rs", 9)]),
        "a line before the body is not the body"
    );
    assert!(discharges(&proof(), path, &[block("src/lib.rs", 21)]));
    assert!(
        discharges(
            &proof(),
            path,
            &[Block {
                file: "src/lib.rs".into(),
                start: Point {
                    line: 20,
                    column: 5
                },
                end: Point {
                    line: 20,
                    column: 6
                },
            }]
        ),
        "the region at the body's closing brace is the one the compiler emits for what follows \
         the branch, and a run that went past the branch without taking it has it"
    );

    let whole_function = Block {
        file: "src/lib.rs".into(),
        start: Point { line: 8, column: 1 },
        end: Point {
            line: 22,
            column: 1,
        },
    };
    assert!(
        discharges(&proof(), path, &[whole_function]),
        "coverage regions nest, so the region that holds the branch says the function ran and \
         not that the body did"
    );
}
