// SPDX-FileCopyrightText: 2026 njutest contributors
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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use criterion::Criterion;
use rust_mutants::cargo::config::configured;
use rust_mutants::catalog::Builder;
use rust_mutants::coverage::{Block, Point, parse_export};
use rust_mutants::instrument::{Instrumenting, instrument_file, plan_file};
use rust_mutants::reach::Reached;
use rust_mutants::rule::Tier;
use rust_mutants::session::{Route, Routing};
use rust_mutants::syntax::{Selection, discover_file};
use rust_mutants::touch::{Seen, TargetTouches, Touched};

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
/// A match of `arms` arms, half of them guarded, ending in a bare wildcard.
fn arms(count: usize) -> String {
    let mut text =
        String::from("//! A generated match.\n\npub fn pick(n: i32) -> i32 {\n    match n {\n");
    for index in 0..count {
        let _written = if index % 2 == 0 {
            writeln!(text, "        {index} => {index},")
        } else {
            writeln!(text, "        {index} if n > {index} => {index},")
        };
    }
    text.push_str("        _ => -1,\n    }\n}\n");
    text
}

/// A file of `lines` lines, a third of them `rust-mutants: skip` markers.
fn annotated(lines: usize) -> String {
    let mut text = String::from("//! A generated module.\n\n");
    for index in 0..lines.saturating_div(4) {
        let _written = writeln!(
            text,
            "// rust-mutants: skip generated {index}\npub fn f{index}(a: i32, b: i32) -> i32 {{\n    a + b\n}}"
        );
    }
    text
}

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
            instrument_file(&Instrumenting {
                path: "src/lib.rs",
                source: std::hint::black_box(one.as_bytes()),
                placements: std::hint::black_box(&placements),
                markers: &[],
                comparable: &BTreeSet::default(),
                probed: &BTreeMap::default(),
                catalog_digest: catalog.digest(),
            })
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
    let arms = arms(500);
    criterion.bench_function("instrument/a match of 500 arms", |bencher| {
        bencher.iter(|| {
            let found =
                discover_file("src/lib.rs", arms.as_bytes(), &selection).expect("the file walks");
            std::hint::black_box(found.candidates.len());
        });
    });
    let commented = annotated(2000);
    criterion.bench_function("annotate/a 2000-line file of markers", |bencher| {
        bencher.iter(|| {
            let found = discover_file("src/lib.rs", commented.as_bytes(), &selection)
                .expect("the file walks");
            std::hint::black_box(found.annotations.len());
        });
    });
    criterion.bench_function("cargo_config/configured over 10 nested dirs", |bencher| {
        bencher.iter(|| configured(std::hint::black_box(&deepest), None));
    });
    routes(criterion);
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

/// What deciding one mutant's route costs, by the guards' record and by a coverage measurement.
fn routes(criterion: &mut Criterion) {
    let record = recorded(16, 8, 2000);
    let names: Vec<String> = record.targets.keys().cloned().collect();
    let targets: Vec<&str> = names.iter().map(String::as_str).collect();
    criterion.bench_function("route/by-touch over 16 targets of 8 tests", |bencher| {
        bencher.iter(|| {
            let among = Routing {
                targets: &targets,
                measurable: &targets,
                also_reaching: &[],
            };
            for index in 0..200u32 {
                let route = Route::by_touch(
                    std::hint::black_box(&record),
                    std::hint::black_box(index),
                    &among,
                );
                let _reaching = std::hint::black_box(route.reaching().len());
            }
        });
    });

    let measurement = coverage(16, 2000);
    criterion.bench_function("route/decide over 16 targets of 2000 blocks", |bencher| {
        bencher.iter(|| {
            let among = Routing {
                targets: &targets,
                measurable: &targets,
                also_reaching: &[],
            };
            for line in 1..=200u32 {
                let route = Route::decide(
                    std::hint::black_box(&measurement),
                    std::path::Path::new("src/lib.rs"),
                    Point { line, column: 5 },
                    &among,
                );
                let _reaching = std::hint::black_box(route.reaching().len());
            }
        });
    });
}

/// A record of what the guards of `targets` tests reached, with `mutants` sites each.
///
/// Routing is the one decision made once per mutant of the catalog, so its cost
/// is multiplied by everything a run measures: a route that got slower here is
/// a run that got slower for every mutation, including the ones no proof
/// removed.
fn recorded(targets: usize, tests: usize, mutants: u32) -> Touched {
    let mut held = Touched::default();
    held.narrowing.compared = (0..mutants).collect();
    for target in 0..targets {
        let mut reached = Seen::default();
        let mut ran = Vec::with_capacity(tests);
        for test in 0..tests {
            let name = format!("reaches_{target}_{test}");
            let first = u32::try_from(test).unwrap_or(0);
            let seen: BTreeSet<u32> = (first..mutants).step_by(tests.max(1)).collect();
            drop(reached.tests.insert(name.clone(), seen));
            ran.push(name);
        }
        let mut touches = TargetTouches::default();
        touches.reached = reached;
        touches.ran = ran;
        drop(held.targets.insert(format!("demo/test/t{target}"), touches));
    }
    held
}

/// A coverage measurement of `targets` targets over `blocks` instrumented regions each.
fn coverage(targets: usize, blocks: u32) -> Reached {
    let block = |line: u32| Block {
        file: "src/lib.rs".into(),
        start: Point { line, column: 1 },
        end: Point { line, column: 80 },
    };
    let mut reached = Reached {
        instrumented: (1..=blocks).map(block).collect(),
        ..Reached::default()
    };
    for target in 0..targets {
        let first = u32::try_from(target).unwrap_or(0).saturating_add(1);
        let covered: BTreeSet<Block> = (first..=blocks)
            .step_by(targets.max(1))
            .map(block)
            .collect();
        drop(
            reached
                .targets
                .insert(format!("demo/test/t{target}"), covered),
        );
    }
    reached
}

/// `harness = false`, so this is the whole program.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
