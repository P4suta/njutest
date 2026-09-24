// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every routing layer routes the mutant planted for it, and a planted expectation that does not hold is reported blind.

#![expect(
    clippy::expect_used,
    reason = "a helper that cannot locate the toolchain or write a script leaves no sentinel to test"
)]

use rust_mutants::cargo::{LocateOptions, Toolchain};
use rust_mutants::probe::Question;
use rust_mutants::runner::Cancel;
use rust_mutants::sentinel::{Expectation, Expected, Infection, KeptFor, Planted, Planting};
use rust_mutants::session::{PrepareOptions, Proof};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

/// The options a caller that measures by the guards alone prepares its own tree with.
fn touching() -> PrepareOptions {
    PrepareOptions {
        coverage: false,
        ..PrepareOptions::default()
    }
}

/// The toolchain a run of this workspace resolves to, located the way a run locates it.
fn located(cargo: &std::path::Path, env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Toolchain {
    Toolchain::locate(
        &LocateOptions {
            cargo: Some(cargo.to_path_buf()),
            search_path: std::env::var_os("PATH"),
            env: Some(env.to_vec()),
        },
        &njutest_devkit::paths::workspace_root(),
        &Cancel::new(),
    )
    .expect("the toolchain this suite runs under is located")
}

#[test]
fn every_layer_routes_the_mutant_planted_for_it_and_leaves_the_one_beside_it() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("planted");
    let open = opening(&njutest_devkit::paths::cargo_binary(), temp.path());
    let run = located(&njutest_devkit::paths::cargo_binary(), &open.env);
    let sighted = rust_mutants::sentinel::sighted(
        rust_mutants::sentinel::Run {
            toolchain: &run,
            open,
            options: &touching(),
            equivalence: false,
        },
        &root,
        &Cancel::new(),
    )
    .expect("the planted crate prepares");

    let said: Vec<String> = sighted
        .sightings
        .iter()
        .map(|one| {
            format!(
                "{} {} expected {} routed {}",
                one.expectation.planted,
                one.expectation.mutant,
                one.expectation.expected,
                one.routed()
            )
        })
        .collect();
    assert!(
        sighted.blind().is_none(),
        "a layer that does not route what was planted for it removes nothing a run may believe: {said:#?}"
    );
    let layers: Vec<Planted> = sighted
        .sightings
        .iter()
        .map(|one| one.expectation.planted)
        .collect();
    for planted in Planted::routing() {
        assert_eq!(
            layers.iter().filter(|one| **one == planted).count(),
            2,
            "{planted} is asked about the mutant it must remove and the one it must leave: {said:#?}"
        );
    }
    assert!(
        sighted.kept.is_empty(),
        "a workspace not opened to keep its directories keeps none: {:?}",
        sighted.kept
    );
}

#[test]
fn an_expectation_the_session_does_not_bear_out_is_blind_and_says_what_it_saw() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("planted");
    rust_mutants::sentinel::materialise(&root).expect("the planted crate is written");
    let cancel = Cancel::new();
    let session = Workspace::open(
        &root,
        opening(&njutest_devkit::paths::cargo_binary(), temp.path()),
        &cancel,
    )
    .expect("the workspace opens")
    .prepare(&rust_mutants::sentinel::routing(&touching()), &cancel)
    .expect("the session prepares");

    let unreached_as_uninfected = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Infection(Infection::Probe(Question::Default)),
            mutant: Planting::new("one", "return-default"),
            expected: Expected::Discharged(Proof::NeverInfected),
        },
    );
    assert!(
        !unreached_as_uninfected.sighted(),
        "a mutant nothing reaches was never infected by anything either, and a sentinel that \
         accepted that would pass a layer which discharges what it never measured"
    );
    assert_eq!(unreached_as_uninfected.routed(), "unreached");

    let removed_as_kept = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Branch,
            mutant: Planting::new("clamp", "le-to-lt"),
            expected: Expected::Kept(KeptFor::Tests),
        },
    );
    assert!(
        !removed_as_kept.sighted(),
        "a mutant a proof removed is not one the tests are asked about"
    );
    assert_eq!(
        removed_as_kept.routed(),
        "discharged: sentinel/test/planted by branch-never-taken"
    );

    let absent = rust_mutants::sentinel::sight(
        &session,
        Expectation {
            planted: Planted::Reach,
            mutant: Planting::new("one", "no-such-rule"),
            expected: Expected::Unreached,
        },
    );
    assert!(
        !absent.sighted(),
        "a planted mutant the catalog does not hold checks nothing, so it cannot vouch for a layer"
    );
    assert!(
        absent.routed().starts_with("not routed: "),
        "{}",
        absent.routed()
    );
    session.close().expect("the session closes");
}

/// Writes an executable shell script at `path`.
#[cfg(unix)]
fn script(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("a script");
    let mut permissions = std::fs::metadata(path).expect("the script").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("an executable script");
}

#[cfg(unix)]
#[test]
fn a_planted_crate_another_compiler_would_build_stops_the_run_rather_than_vouching_for_it() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let open = opening(&njutest_devkit::paths::cargo_binary(), temp.path());
    let real = located(&njutest_devkit::paths::cargo_binary(), &open.env);
    let sysroot = real
        .sysroot()
        .expect("the real toolchain names its sysroot");
    let (cargo, rustc) = (
        sysroot.join("bin/cargo").display().to_string(),
        sysroot.join("bin/rustc").display().to_string(),
    );
    let named = temp.path().join("named/bin");
    let other = temp.path().join("other");
    std::fs::create_dir_all(&named).expect("the named toolchain");
    std::fs::create_dir_all(other.join("bin")).expect("the other sysroot");
    script(&named.join("cargo"), &format!("exec {cargo} \"$@\""));
    script(
        &named.join("rustc"),
        &format!(
            "if [ \"$1\" = --print ] && [ \"$2\" = sysroot ]; then echo {}; exit 0; fi\nexec {rustc} \"$@\"",
            other.display()
        ),
    );
    script(&other.join("bin/cargo"), &format!("exec {cargo} \"$@\""));
    script(
        &other.join("bin/rustc"),
        &format!(
            "if [ \"$1\" = -vV ]; then {rustc} -vV | sed 's/^release: .*/release: 0.0.0-other/'; exit 0; fi\nexec {rustc} \"$@\""
        ),
    );
    script(
        &other.join("bin/rustdoc"),
        &format!("exec {}/bin/rustdoc \"$@\"", sysroot.display()),
    );
    let run = located(&named.join("cargo"), &open.env);

    let refused = rust_mutants::sentinel::sighted(
        rust_mutants::sentinel::Run {
            toolchain: &run,
            open,
            options: &touching(),
            equivalence: false,
        },
        &temp.path().join("planted"),
        &Cancel::new(),
    )
    .expect_err("a planted session built by another compiler vouches for nothing about this run");
    assert_eq!(refused.code().code, "RM5008", "{refused}");
}

#[test]
fn a_run_that_routes_by_coverage_or_asks_equivalence_has_each_sighted_too() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let root = temp.path().join("planted");
    let open = opening(&njutest_devkit::paths::cargo_binary(), temp.path());
    let run = located(&njutest_devkit::paths::cargo_binary(), &open.env);
    let sighted = rust_mutants::sentinel::sighted(
        rust_mutants::sentinel::Run {
            toolchain: &run,
            open,
            options: &PrepareOptions::default(),
            equivalence: true,
        },
        &root,
        &Cancel::new(),
    )
    .expect("the planted crates prepare");
    let said: Vec<String> = sighted
        .sightings
        .iter()
        .map(|one| {
            format!(
                "{} {} expected {} routed {}",
                one.expectation.planted,
                one.expectation.mutant,
                one.expectation.expected,
                one.routed()
            )
        })
        .collect();
    assert!(sighted.blind().is_none(), "{said:#?}");
    if njutest_devkit::reproducible::builds_the_same_twice() {
        let compared: Vec<String> = sighted
            .sightings
            .iter()
            .filter(|one| one.expectation.planted == Planted::Equivalence)
            .map(rust_mutants::sentinel::Sighting::routed)
            .collect();
        assert_eq!(
            compared,
            ["identical", "differs"],
            "a machine that builds one tree the same way twice keeps the layer in service, so \
             the pair is told apart by what the compiler did and not passed because nothing was \
             said: {said:#?}"
        );
    }
    for planted in [Planted::Coverage, Planted::Equivalence] {
        assert_eq!(
            sighted
                .sightings
                .iter()
                .filter(|one| one.expectation.planted == planted)
                .count(),
            2,
            "a run that measures coverage could route by it, and one that asks the equivalence \
             layer removes survivors by it, so each is asked about its pair: {said:#?}"
        );
    }
}
