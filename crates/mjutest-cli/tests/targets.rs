// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run measures: one test, named the way a person names it, with a
//! stable identity of its own.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use mjutest_cli::targets::{
    Entry, EntryKind, TARGET_DOMAIN, Unit, UnitKind, WHOLE_BINARY, enumerate, parse_list, target_id,
};
use mjutest_cli::trace::Recorder;
use mjutest_cli::watch::Watch;
use rust_mutants::runner::Cancel;

// --- the listing ------------------------------------------------------------------

#[test]
fn a_terse_listing_is_read_and_a_benchmark_is_not_a_test() {
    let listing = "tests::ignored_one: test\ntests::inner::deep: test\nbench_it: benchmark\n";
    assert_eq!(
        parse_list(listing.as_bytes()),
        [
            Entry {
                path: "tests::ignored_one".to_owned(),
                kind: EntryKind::Test,
            },
            Entry {
                path: "tests::inner::deep".to_owned(),
                kind: EntryKind::Test,
            },
            Entry {
                path: "bench_it".to_owned(),
                kind: EntryKind::Benchmark,
            },
        ]
    );
}

#[test]
fn a_listing_that_is_not_one_yields_nothing_rather_than_a_guess() {
    for text in [
        "",
        "\n\n",
        "4 tests, 0 benchmarks\n",
        "error: unrecognised option --list\n",
        "no colon here\n",
        ": test\n",
    ] {
        assert!(parse_list(text.as_bytes()).is_empty(), "{text:?}");
    }
    // The summary line of a non-terse listing is not a test either.
    let mixed = "a::b: test\n\n2 tests, 0 benchmarks\n";
    assert_eq!(parse_list(mixed.as_bytes()).len(), 1);
}

// --- identity ----------------------------------------------------------------------

#[test]
fn a_test_is_named_by_its_package_its_unit_and_its_path() {
    assert_eq!(TARGET_DOMAIN, "mjutest-target-v1");
    let id = target_id("core", UnitKind::Lib, "tests::plain");
    assert_eq!(id.len(), 16, "{id}");
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );

    // Every field is part of the name, and no two of them can be confused
    // for one another.
    let mut seen = std::collections::BTreeSet::new();
    for (package, unit, path) in [
        ("core", UnitKind::Lib, "tests::plain"),
        ("core", UnitKind::Lib, "tests::plainx"),
        ("core", UnitKind::Test, "tests::plain"),
        ("corex", UnitKind::Lib, "tests::plain"),
        ("core", UnitKind::Bin, "tests::plain"),
        ("core", UnitKind::Example, "tests::plain"),
        ("cor", UnitKind::Lib, "etests::plain"),
    ] {
        assert!(
            seen.insert(target_id(package, unit, path)),
            "{package}/{unit:?}/{path} collides"
        );
    }

    // It does not move when a line does.
    assert_eq!(target_id("core", UnitKind::Lib, "tests::plain"), id);
    for kind in UnitKind::ALL {
        assert_eq!(UnitKind::parse(kind.name()), Some(kind));
        assert!(!kind.name().is_empty());
    }
}

#[test]
fn a_binary_with_its_own_harness_is_one_target_and_says_so() {
    assert_eq!(WHOLE_BINARY, "");
    let id = target_id("core", UnitKind::Test, WHOLE_BINARY);
    assert_ne!(id, target_id("core", UnitKind::Test, "anything"));
}

// --- enumerating a real binary --------------------------------------------------------

/// Builds a fixture's test binaries the way a run does, and returns the
/// units they came from.
fn built_units(fixture: &str) -> (Vec<Unit>, tempfile::TempDir) {
    use rust_mutants::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};
    use rust_mutants::execute::{BuildOptions, build};

    let dir = mjutest_devkit::paths::fixtures_dir().join(fixture);
    let target = tempfile::Builder::new()
        .prefix("mjutest-targets-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let engine_trace = rust_mutants::trace::Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        &dir,
        &cancel,
    )
    .expect("locate");
    let driver = Driver {
        toolchain: &toolchain,
        dir: &dir,
        cancel: &cancel,
        trace: &engine_trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let targets = build(
        &driver,
        &metadata.packages,
        &BuildOptions {
            target_dir: Some(target.path().to_path_buf()),
            locked: true,
            offline: true,
        },
    )
    .expect("build");
    let units = targets
        .into_iter()
        .map(|target| Unit {
            package: target.package,
            kind: UnitKind::of(target.kind),
            name: target.name,
            executable: target.executable,
            cwd: target.cwd,
        })
        .collect();
    (units, target)
}

#[test]
fn a_built_binary_names_every_test_it_holds_with_its_own_identity() {
    let (units, _target) = built_units("fixture-simple");
    let mut named: Vec<(String, String, bool)> = Vec::new();
    for unit in &units {
        for target in
            enumerate(unit, Watch::new(&Cancel::new(), &Recorder::disabled())).expect("enumerate")
        {
            assert_eq!(target.id.len(), 16, "{target:?}");
            assert_eq!(
                target.id,
                target_id(&target.package, target.unit, &target.path),
                "the identity is a function of what it names"
            );
            assert!(!target.is_whole_binary());
            named.push((target.name(), target.path.clone(), target.ignored));
        }
    }
    named.sort();
    assert_eq!(
        named,
        [
            (
                "fixture-simple/lib/fixture_simple tests::max_picks_the_larger".to_owned(),
                "tests::max_picks_the_larger".to_owned(),
                false
            ),
            (
                "fixture-simple/test/parity even_numbers_are_even".to_owned(),
                "even_numbers_are_even".to_owned(),
                false
            ),
        ]
    );
}
