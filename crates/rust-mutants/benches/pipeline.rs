// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each stage of preparing a workspace costs, on inputs the size of a real one.
//!
//! [ADR 0004](../../../docs/adr/0004-proof-layers-not-budgets.md) says a run
//! that is too slow is a run missing a proof or doing work nothing reads. These
//! numbers are how to tell the two apart: a stage that got slower here is work
//! somebody added, and a run that got slower with these unchanged is a proof
//! layer that stopped removing executions.

#![expect(
    clippy::expect_used,
    reason = "a benchmark that cannot build its own input has nothing to measure, and saying so \
              by panicking is the whole of its error handling"
)]

use std::fmt::Write as _;

use criterion::Criterion;
use rust_mutants::cargo::config::configured;
use rust_mutants::catalog::Builder;
use rust_mutants::coverage::parse_export;
use rust_mutants::instrument::{instrument_file, plan_file};
use rust_mutants::rule::Tier;
use rust_mutants::syntax::{Selection, discover_file};

/// A file of `functions` functions, each with a comparison, a branch, and an arithmetic tail.
fn source(functions: usize) -> String {
    let mut text = String::from("//! A module the size of a real one.\n\n");
    for index in 0..functions {
        let _written = writeln!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    \
             let mut total = 0;\n    \
             if a > b {{\n        total += a - b;\n    }} else {{\n        total += b - a;\n    }}\n    \
             while total > 10 {{\n        total /= 2;\n    }}\n    \
             total + a * b\n}}\n"
        );
    }
    text
}

/// A coverage export naming `functions` functions with four regions each.
fn export(functions: usize) -> String {
    let mut regions = String::new();
    for index in 0..functions {
        let line = index.saturating_mul(4).saturating_add(1);
        for step in 0..4 {
            if !regions.is_empty() {
                regions.push(',');
            }
            let _written = write!(
                regions,
                "{{\"filenames\":[\"src/lib.rs\"],\"regions\":[[{},1,{},9,{},0,0,0]]}}",
                line.saturating_add(step),
                line.saturating_add(step),
                step
            );
        }
    }
    format!(
        "{{\"type\":\"llvm.coverage.json.export\",\"version\":\"2.0.1\",\"data\":[{{\"functions\":[{regions}]}}]}}"
    )
}

fn benchmarks(criterion: &mut Criterion) {
    let registry = rust_mutants::rule::Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);

    let big = source(2000);
    criterion.bench_function("discover/2000-function file", |bencher| {
        bencher.iter(|| {
            discover_file(
                "src/lib.rs",
                std::hint::black_box(big.as_bytes()),
                std::hint::black_box(&selection),
            )
        });
    });

    let one = source(200);
    let discovery =
        discover_file("src/lib.rs", one.as_bytes(), &selection).expect("the file discovers");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder
            .add(found.candidate.clone())
            .expect("the candidate is one");
    }
    let catalog = builder.build().expect("the catalog builds");
    let placements =
        plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("the plan is one");
    criterion.bench_function("instrument/200-function file", |bencher| {
        bencher.iter(|| {
            instrument_file(
                "src/lib.rs",
                std::hint::black_box(one.as_bytes()),
                std::hint::black_box(&placements),
                catalog.digest(),
            )
        });
    });

    let candidates: Vec<rust_mutants::catalog::Candidate> = discovery
        .candidates
        .iter()
        .map(|found| found.candidate.clone())
        .collect();
    criterion.bench_function("catalog/build from a file's candidates", |bencher| {
        bencher.iter(|| {
            let mut builder = Builder::new();
            for candidate in std::hint::black_box(&candidates) {
                let _added = builder.add(candidate.clone());
            }
            builder.build()
        });
    });

    let document = export(500);
    criterion.bench_function("coverage/parse export of 500 functions", |bencher| {
        bencher.iter(|| parse_export(std::hint::black_box(document.as_bytes())));
    });

    let nest = nested(10);
    let deepest = nest.path().join("d0/d1/d2/d3/d4/d5/d6/d7/d8/d9");
    criterion.bench_function("cargo_config/configured over 10 nested dirs", |bencher| {
        bencher.iter(|| configured(std::hint::black_box(&deepest), None));
    });
}

/// A tree `depth` directories deep, every level configuring its own flags.
fn nested(depth: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a temporary root");
    let mut at = root.path().to_path_buf();
    for level in 0..depth {
        at = at.join(format!("d{level}"));
        let directory = at.join(".cargo");
        std::fs::create_dir_all(&directory).expect("the directory");
        std::fs::write(
            directory.join("config.toml"),
            format!("[build]\nrustflags = [\"--cfg\", \"level{level}\"]\n"),
        )
        .expect("the file");
    }
    root
}

/// `harness = false`, so this is the whole program.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
