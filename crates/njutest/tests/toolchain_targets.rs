// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run measures: one test, named the way a person names it, with a stable identity of its own.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::targets::{
    Entry, EntryKind, TARGET_DOMAIN, Target, TargetId, Unit, UnitKind, WHOLE_BINARY, enumerate,
    parse_list, target_id,
};
use njutest::trace::Recorder;
use njutest::watch::Watch;
use rust_mutants::runner::Cancel;

#[test]
fn a_terse_listing_is_read_and_a_benchmark_is_not_a_test() {
    let listing = "tests::ignored_one: test\ntests::inner::deep: test\nbench_it: benchmark\n";
    assert_eq!(
        parse_list(listing.as_bytes()).expect("utf-8 listing"),
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
    assert!(
        parse_list(b"")
            .expect("an empty stream names no entries")
            .is_empty()
    );
    for text in [
        "\n",
        "\n\n",
        " \n",
        "4 tests, 0 benchmarks\n",
        "error: unrecognised option --list\n",
        "no colon here\n",
        ": test\n",
        "a::b: future-kind\n",
        "a::b: test \n",
    ] {
        assert!(parse_list(text.as_bytes()).is_err(), "{text:?}");
    }
    let mixed = "a::b: test\n\n2 tests, 0 benchmarks\n";
    assert!(
        parse_list(mixed.as_bytes()).is_err(),
        "one valid line cannot hide one malformed line"
    );
    assert!(
        parse_list(&[b'a', b':', b' ', b't', b'e', b's', b't', b'\n', 0xff]).is_err(),
        "invalid protocol bytes are a typed refusal rather than replacement text"
    );
}

#[test]
fn a_test_is_named_by_its_package_its_unit_and_its_path() {
    assert_eq!(TARGET_DOMAIN, "njutest-target-v1");
    let id = target_id("core", UnitKind::Lib, "core", "tests::plain").expect("bounded fields");
    assert_eq!(id.as_str().len(), 64, "{id}");
    assert!(
        id.as_str()
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );

    let mut seen = std::collections::BTreeSet::new();
    for (package, unit, unit_name, path) in [
        ("core", UnitKind::Lib, "core", "tests::plain"),
        ("core", UnitKind::Lib, "core", "tests::plainx"),
        ("core", UnitKind::Test, "core", "tests::plain"),
        ("corex", UnitKind::Lib, "core", "tests::plain"),
        ("core", UnitKind::Bin, "core", "tests::plain"),
        ("core", UnitKind::Example, "core", "tests::plain"),
        ("cor", UnitKind::Lib, "core", "etests::plain"),
        ("core", UnitKind::Test, "one", "tests::plain"),
        ("core", UnitKind::Test, "two", "tests::plain"),
        ("core", UnitKind::Test, "on", "etests::plain"),
    ] {
        assert!(
            seen.insert(target_id(package, unit, unit_name, path).expect("bounded fields")),
            "{package}/{unit:?}/{unit_name} {path} collides"
        );
    }

    assert_eq!(
        target_id("core", UnitKind::Lib, "core", "tests::plain").expect("bounded fields"),
        id
    );
    for kind in UnitKind::ALL {
        assert_eq!(UnitKind::parse(kind.name()), Some(kind));
        assert!(!kind.name().is_empty());
    }
}

#[test]
fn two_binaries_of_one_package_that_hold_the_same_test_are_two_targets() {
    let of = |unit_name: &str| Target {
        id: target_id("core", UnitKind::Test, unit_name, "works").expect("bounded fields"),
        package: "core".to_owned(),
        unit: UnitKind::Test,
        unit_name: unit_name.to_owned(),
        path: "works".to_owned(),
        ignored: false,
        executable: std::path::PathBuf::from("/nowhere"),
        cwd: std::path::PathBuf::from("/nowhere"),
        env: Vec::new(),
    };
    let alpha = of("alpha");
    let beta = of("beta");
    assert_ne!(
        alpha.name(),
        beta.name(),
        "a person tells the two apart by the binary they are in"
    );
    assert_ne!(
        alpha.id, beta.id,
        "and so must every record keyed by identity: a report row, an evidence \
         key, and the lookup that decides which target a route names"
    );
}

#[test]
fn a_binary_with_its_own_harness_is_one_target_and_says_so() {
    assert_eq!(WHOLE_BINARY, "");
    let id = target_id("core", UnitKind::Test, "core", WHOLE_BINARY).expect("bounded fields");
    assert_ne!(
        id,
        target_id("core", UnitKind::Test, "core", "anything").expect("bounded fields")
    );
}

#[test]
fn a_target_identity_has_one_canonical_spelling() {
    let id = target_id("core", UnitKind::Test, "core", "anything").expect("bounded fields");
    assert_eq!(TargetId::parse(id.as_str()), Ok(id));
    for refused in [
        "",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeF",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
    ] {
        assert!(TargetId::parse(refused).is_err(), "{refused:?}");
    }
}

#[test]
#[cfg(unix)]
fn an_ignored_listing_failure_refuses_the_whole_enumeration() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let executable = temporary.path().join("listing");
    std::fs::write(
        &executable,
        "#!/bin/sh\nif [ \"$2\" = \"--ignored\" ]; then exit 9; fi\nprintf 'works: test\\n'\n",
    )
    .expect("write listing program");
    let mut permissions = std::fs::metadata(&executable)
        .expect("listing metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).expect("make listing executable");

    let unit = Unit {
        package: "fixture".to_owned(),
        kind: UnitKind::Test,
        name: "listing".to_owned(),
        harness: true,
        executable,
        cwd: temporary.path().to_path_buf(),
        env: Vec::new(),
    };
    assert!(
        enumerate(&unit, Watch::new(&Cancel::new(), &Recorder::disabled())).is_err(),
        "a failed ignored listing cannot relabel every listed test as non-ignored"
    );
}

/// Builds a fixture's test binaries the way a run does, and returns the units they came from.
fn built_units(fixture: &str) -> (Vec<Unit>, tempfile::TempDir) {
    use njutest::build::{BuildOptions, Cargo, Flavour, Selection, build};
    use rust_mutants::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};

    let dir = njutest_devkit::paths::fixtures_dir().join(fixture);
    let target = tempfile::Builder::new()
        .prefix("njutest-targets-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let engine_trace = rust_mutants::trace::Recorder::disabled();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        &dir,
        &cancel,
    )
    .expect("locate");
    let metadata = Metadata::load(
        &Driver {
            toolchain: &toolchain,
            dir: &dir,
            cancel: &cancel,
            trace: &engine_trace,
        },
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let built = build(
        &toolchain,
        &metadata.packages,
        &BuildOptions {
            root: dir,
            selection: Selection::default(),
            flavour: Flavour::Native,
            target_dir: target.path().join("layer"),
            scratch_build_dir: target.path().join("scratch"),
            env: std::env::vars_os().collect(),
            cargo: Cargo {
                offline: true,
                locked: true,
            },
            timeout: None,
        },
        Watch::new(&cancel, &trace),
    )
    .expect("build");
    assert!(built.failure.is_none(), "{:?}", built.failure);
    (built.units, target)
}

#[test]
fn a_built_binary_names_every_test_it_holds_with_its_own_identity() {
    let (units, target_directory_owner) = built_units("fixture-simple");
    let mut named: Vec<(String, String, bool)> = Vec::new();
    for unit in &units {
        for target in
            enumerate(unit, Watch::new(&Cancel::new(), &Recorder::disabled())).expect("enumerate")
        {
            assert_eq!(target.id.as_str().len(), 64, "{target:?}");
            assert_eq!(
                target.id,
                target_id(
                    &target.package,
                    target.unit,
                    &target.unit_name,
                    &target.path
                )
                .expect("bounded fields"),
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
    drop(target_directory_owner);
}

#[test]
fn a_plan_and_a_run_name_a_whole_binary_the_same_way() {
    use njutest::targets::whole_binary;

    let unit = Unit {
        package: "core".to_owned(),
        kind: UnitKind::Test,
        name: "harnessed".to_owned(),
        harness: false,
        executable: std::path::PathBuf::from("/tmp/harnessed"),
        cwd: std::path::PathBuf::from("/tmp"),
        env: Vec::new(),
    };
    let named = whole_binary(&unit).expect("bounded fields");

    assert_eq!(
        named.id,
        target_id("core", UnitKind::Test, "harnessed", WHOLE_BINARY).expect("bounded fields"),
        "the identity a run puts a mutation to is the identity a plan says it would: a \
         plan that named it any other way is about a run nobody made"
    );
    assert_eq!(named.path, WHOLE_BINARY);
    assert!(
        named.is_whole_binary(),
        "and it is the one target the binary has rather than one test of it"
    );
    assert!(
        !named.ignored,
        "a binary with its own harness is never `#[ignore]`d, because nothing here read \
         an attribute: the harness is asked to run and it runs"
    );
    assert_eq!(named.package, unit.package);
    assert_eq!(named.unit_name, unit.name);
    assert_eq!(named.executable, unit.executable);
    assert_eq!(named.cwd, unit.cwd);
}
