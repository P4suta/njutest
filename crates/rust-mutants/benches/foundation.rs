// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the byte foundation costs.

use std::fmt::Write as _;

use criterion::Criterion;
use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::flatten::flatten;
use rust_mutants::id::{Identity, digest};
use rust_mutants::span::Span;
use rust_mutants::splice::{Splice, apply};

/// A file about the size of a real module, with an edit every few lines.
fn source() -> String {
    let mut text = String::from("//! A module.\n\n");
    for index in 0..200 {
        let appended = write!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    if a > b {{ a + b }} else {{ a - b }}\n}}\n"
        );
        assert!(
            matches!(appended, Ok(())),
            "writing to a String is infallible"
        );
    }
    text
}

fn benchmarks(criterion: &mut Criterion) {
    let text = source();

    criterion.bench_function("splice/200 edits", |bencher| {
        let mut splices = Vec::new();
        for (at, _) in text.match_indices("> ") {
            let start = u32::try_from(at);
            assert_eq!(
                result_state(&start),
                Returned,
                "the generated input fits u32"
            );
            let Ok(start) = start else { return };
            let Some(after) = at.checked_add(1) else {
                return;
            };
            let end = u32::try_from(after);
            assert_eq!(result_state(&end), Returned, "the generated input fits u32");
            let Ok(end) = end else { return };
            splices.push(Splice {
                span: Span { start, end },
                original: b">".to_vec(),
                replacement: b">=".to_vec(),
            });
        }
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

/// `harness = false`, so this is the whole program: the generated harness would put an undocumented public function in a crate that documents everything.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
