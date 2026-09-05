// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a report costs to audit and to write.
//!
//! Observations, not contracts: nothing fails when a number moves. They are
//! here because the audit runs on every report a run persists and the
//! projections run once per format, so a run that got slower at the end is
//! a run that got slower here, and a person should be able to see that
//! rather than infer it.

use criterion::Criterion;
use mjutest_cli::config::Contract;
use mjutest_cli::report::{
    Limitation, Report, RunKind, TargetAccounting, TargetRecord, TargetStatus, Verdict, audit,
    json, lines,
};

/// A report of a workspace with a few hundred tests, which is where the
/// audit's per-record work starts to be worth measuring.
fn report(targets: u32) -> Report {
    let mut report = Report::new(
        "20260905T081500Z-abcdef",
        RunKind::Full,
        Contract::StandardV1,
    );
    report.verdict = Verdict::Assured;
    "workspace".clone_into(&mut report.repository.root_name);
    report.repository.workspace_digest = "a".repeat(64);
    report.repository.configuration_digest = "b".repeat(64);
    report.repository.git.available = true;
    report.repository.git.commit = "0".repeat(40);
    "main".clone_into(&mut report.repository.git.branch);
    "rustc 1.98.0".clone_into(&mut report.toolchain.rustc);
    report.accounting.targets = TargetAccounting {
        selected: targets,
        passed: targets,
        failed: 0,
        skipped: 0,
        missing: 0,
    };
    report.targets = (0..targets)
        .map(|index| TargetRecord {
            id: format!("{index:016x}"),
            name: format!("core/lib/core tests::case_{index}"),
            package: "core".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: u64::from(targets.saturating_sub(index)),
            message: None,
        })
        .collect();
    report.limitations = vec![Limitation::new("doctests-not-routed", "doctests run once")];
    report
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
        bencher.iter(|| lines::stream(std::hint::black_box(&large)));
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
