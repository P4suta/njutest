// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the byte foundation costs.
//!
//! These are observations, not contracts: nothing fails when a number
//! moves, and no verdict depends on one ([ADR 0004] asks a proof layer to
//! report what it saved, and this is how a person answers "did that change
//! make discovery slower" without guessing).
//!
//! The three measured here are the ones every run pays per file — a splice
//! of the edits, the flattening of an alternative onto one line, and the
//! identity of a mutant — so a regression in any of them is a regression in
//! the whole engine.
//!
//! [ADR 0004]: https://github.com/P4suta/mjutest/blob/main/docs/adr/0004-proof-layers-not-budgets.md

use std::fmt::Write as _;

use criterion::Criterion;
use rust_mutants::flatten::flatten;
use rust_mutants::id::{Identity, digest};
use rust_mutants::span::Span;
use rust_mutants::splice::{Splice, apply};

/// A file about the size of a real module, with an edit every few lines.
fn source() -> String {
    let mut text = String::from("//! A module.\n\n");
    for index in 0..200 {
        let _written = writeln!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    if a > b {{ a + b }} else {{ a - b }}\n}}\n"
        );
    }
    text
}

fn benchmarks(criterion: &mut Criterion) {
    let text = source();

    criterion.bench_function("splice/200 edits", |bencher| {
        let splices: Vec<Splice> = text
            .match_indices("> ")
            .map(|(at, _)| Splice {
                span: Span {
                    start: u32::try_from(at).unwrap_or(u32::MAX),
                    end: u32::try_from(at.saturating_add(1)).unwrap_or(u32::MAX),
                },
                original: b">".to_vec(),
                replacement: b">=".to_vec(),
            })
            .collect();
        let bytes = text.as_bytes();
        bencher.iter(|| apply(std::hint::black_box(bytes), std::hint::black_box(&splices)));
    });

    criterion.bench_function("flatten/one function", |bencher| {
        let one = "pub fn f(a: i32) -> i32 {\n    // a comment\n    if a > 0 {\n        a + 1\n    } else {\n        a - 1\n    }\n}";
        bencher.iter(|| flatten(std::hint::black_box(one)));
    });

    criterion.bench_function("identity/one mutant", |bencher| {
        let identity = Identity {
            path: "crates/core/src/lib.rs".to_owned(),
            rule_name: "gt-to-ge".to_owned(),
            rule_version: 1,
            span: Span {
                start: 1024,
                end: 1025,
            },
            source_digest: digest(text.as_bytes()),
            original_digest: digest(b">"),
            replacement_digest: digest(b">="),
        };
        bencher.iter(|| std::hint::black_box(&identity).id());
    });
}

/// `harness = false`, so this is the whole program: the generated harness
/// would put an undocumented public function in a crate that documents
/// everything.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
