// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a report costs to audit and to write.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a bench reports a fixture it cannot build by aborting"
)]

use criterion::Criterion;
use njutest_cli::config::Contract;
use njutest_cli::report::{
    BuildReport, Limitation, Report, RunKind, TargetRecord, TargetStatus, audit, json, lines,
};

/// A report of a workspace with a few hundred tests, which is where the audit's per-record work starts to be worth measuring.
fn report(targets: u32) -> Report {
    let mut source = BuildReport::new("bench-evidence", RunKind::Full, Contract::StandardV1);
    "workspace".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    source.repository.git = njutest_cli::report::Git::Said(njutest_cli::report::Said {
        commit: "0".repeat(40),
        branch: "main".to_owned(),
        dirty: false,
        against: None,
    });
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest_cli::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-09-05T08:15:00Z".clone_into(&mut source.timing.started);
    "2026-09-05T08:15:30Z".clone_into(&mut source.timing.finished);
    source.targets = (0..targets)
        .map(|index| TargetRecord {
            id: format!("{index:016x}"),
            name: format!("core/lib/core tests::case_{index}"),
            package: "core".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: u64::from(targets.saturating_sub(index)),
            message: None,
        })
        .collect();
    source.count_targets().expect("one exact target accounting");
    source.limitations = vec![Limitation::new("doctests-not-routed", "doctests run once")];
    source.verdict = source.concluded();
    let measurements = njutest_cli::report::across::BuildMeasurements::checked(vec![(
        njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let run =
        rust_mutants::id::RunId::try_from("20260905t081500z-abcdef").expect("a canonical run id");
    let latticed = njutest_cli::report::across::configured(&run, &measurements)
        .expect("one checked complete lattice");
    let njutest_cli::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog bench fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

fn benchmarks(criterion: &mut Criterion) {
    let small = report(20);
    let large = report(2_000);

    criterion.bench_function("audit/20 targets", |bencher| {
        bencher.iter(|| audit::validate_for_persistence(std::hint::black_box(&small)));
    });
    criterion.bench_function("audit/2000 targets", |bencher| {
        bencher.iter(|| audit::validate_for_persistence(std::hint::black_box(&large)));
    });
    criterion.bench_function("json/2000 targets", |bencher| {
        bencher.iter(|| json::render(std::hint::black_box(&large)));
    });
    criterion.bench_function("lines/2000 targets", |bencher| {
        bencher.iter(|| lines::stream(std::hint::black_box(&large)).is_ok());
    });
}

/// `harness = false`, so this is the whole program: the generated harness would put an undocumented public function in a crate that documents everything.
fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    benchmarks(&mut criterion);
    criterion.final_summary();
}
