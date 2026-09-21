// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What deciding a run's identity costs, and what settling the layer that removes findings costs.

use std::collections::BTreeMap;

use criterion::Criterion;
use njutest_cli::assure::equivalence::{Decided, settle};
use njutest_cli::assure::mutation::{Disposition, Judged};
use njutest_cli::config::Contract;
use njutest_cli::evidence::{digest, key};
use rust_mutants::runner::Cancel;
use rust_mutants::session::Route;

/// A workspace's worth of environment: the variables a run selected, as it read them.
fn environment(count: u32) -> Vec<(String, String)> {
    (0..count)
        .map(|index| (format!("NJUTEST_VAR_{index}"), format!("value-{index}")))
        .collect()
}

fn inputs(packages: u32) -> digest::Inputs {
    digest::Inputs {
        tree: "a".repeat(64),
        corpus: "b".repeat(64),
        dependencies: "c".repeat(64),
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: environment(packages),
        contract: Contract::StandardV1,
        configuration: "d".repeat(64),
        test_args: vec!["--test-threads".to_owned(), "1".to_owned()],
        mode: digest::Mode::Full,
        shard: None,
    }
}

fn linked(packages: u32) -> key::Linked {
    key::Linked {
        packages: (0..packages)
            .map(|index| format!("package-{index}@1.0.{index}"))
            .collect(),
        sources: (0..packages)
            .map(|index| (format!("package-{index}"), format!("{index:064x}")))
            .collect::<BTreeMap<String, String>>(),
        dependencies: "c".repeat(64),
        reads_directories: false,
        tree: "a".repeat(64),
    }
}

fn common() -> key::Common {
    key::Common {
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: environment(20),
        contract: "standard-v1".to_owned(),
        test_args: Vec::new(),
        build: rust_mutants::cargo::BuildConfig {
            features: vec!["default".to_owned()],
            ..rust_mutants::cargo::BuildConfig::default()
        }
        .selection(),
        timeout_ms: 60_000,
        steps: 50_000_000,
        versions: vec!["njutest 0.1.0".to_owned(), "rust-mutants 0.1.0".to_owned()],
        corpus: "b".repeat(64),
    }
}

/// Every mutation a run judged, all of them survivors, as the layer that removes findings is handed them.
fn survivors(count: u32) -> (Vec<Judged>, Vec<Decided>) {
    let judged = (0..count)
        .map(|index| Judged {
            catalog_index: index,
            id: format!("{index:064x}"),
            display_id: format!("crates/core/src/lib.rs:{index}:comparison-swap"),
            path: "crates/core/src/lib.rs".to_owned(),
            rule: "le-to-lt".to_owned(),
            item: "demo".to_owned(),
            original: ">".to_owned(),
            replacement: String::new(),
            position: None,
            disposition: Disposition::Survived {
                route: Route::All {
                    reaching: vec!["core/lib/core".to_owned()],
                    fallback: rust_mutants::session::Fallback::NotMeasured,
                },
            },
            source_run_id: None,
            routing: None,
        })
        .collect();
    let decided = (0..count)
        .map(|index| Decided {
            display_id: format!("crates/core/src/lib.rs:{index}:comparison-swap"),
            equivalent: index % 8 == 0,
            detail: "the compiler renders it identically".to_owned(),
        })
        .collect();
    (judged, decided)
}

fn benchmarks(criterion: &mut Criterion) {
    let small = inputs(20);
    let large = inputs(500);
    criterion.bench_function("identity/20 variables", |bencher| {
        bencher.iter(|| digest::identity(std::hint::black_box(&small)));
    });
    criterion.bench_function("identity/500 variables", |bencher| {
        bencher.iter(|| digest::identity(std::hint::black_box(&large)));
    });

    let common = common();
    let few = linked(10);
    let many = linked(400);
    criterion.bench_function("behaviour/10 packages", |bencher| {
        bencher.iter(|| key::behaviour(std::hint::black_box(&few), std::hint::black_box(&common)));
    });
    criterion.bench_function("behaviour/400 packages", |bencher| {
        bencher.iter(|| key::behaviour(std::hint::black_box(&many), std::hint::black_box(&common)));
    });

    let cancel = Cancel::new();
    let trace = njutest_cli::trace::Recorder::disabled();
    let watch = njutest_cli::watch::Watch::new(&cancel, &trace);
    for count in [100u32, 1_000] {
        let (judged, decided) = survivors(count);
        criterion.bench_function(
            &format!("equivalence-settle/{count} survivors"),
            |bencher| {
                bencher.iter_batched_ref(
                    || judged.clone(),
                    |judged| settle(judged, std::hint::black_box(&decided), watch),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
}

/// `harness = false`, so this is the whole program: the generated harness would put an undocumented public function in a crate that documents everything.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
