// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Each knob breaks the one target of `fixture-environment` that depends on what it sets, and moves no other.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::collections::{BTreeMap, BTreeSet};

use njutest::assure::knobs::{Place, measured};
use njutest::report::knobs::{Knob, NotPut, Standing};
use njutest::trace::Recorder;
use njutest::watch::Watch;
use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

fn prepare(fixture: &Fixture) -> Session {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            failing: rust_mutants::session::Failing::Exclude,
            ..njutest::assure::engine::switches()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

/// The one target each knob depends on in the fixture, by the knob.
const fn dependent(knob: Knob) -> &'static str {
    match knob {
        Knob::Timezone => "environment/test/timezone",
        Knob::Locale => "environment/test/locale",
        Knob::TempDirectory => "environment/test/temp",
        Knob::Home => "environment/test/home",
        Knob::Umask => "environment/test/umask",
        Knob::Columns => "environment/test/columns",
        Knob::Threads => "environment/test/threads",
    }
}

#[test]
fn each_knob_breaks_the_one_target_that_depends_on_it_and_moves_no_other() {
    let fixture = Fixture::copy("fixture-environment");
    let session = prepare(&fixture);
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let vars: rust_mutants::vars::Variables = std::env::vars_os().collect();
    let place = Place::probed(fixture.temp(), &vars, &cancel).expect("the knobs' directories");
    let baseline = njutest::assure::baseline::observe(
        &session,
        njutest::assure::baseline::Reporting {
            notes: &mut njutest::ui::Notes::Silent,
            watch: Watch::new(&cancel, &trace),
        },
    )
    .expect("the fixture's baseline is read");
    let passed: BTreeSet<String> = njutest::assure::knobs::passing(&baseline);
    assert!(
        passed.contains("environment/test/steady") && passed.contains("environment/test/timezone"),
        "the baseline passes the targets the knobs are about: {passed:?}"
    );
    let records = measured(
        &session,
        &Knob::ALL,
        (&passed, &place),
        Watch::new(&cancel, &trace),
    )
    .expect("every knob is measured");
    let standing: BTreeMap<(Knob, &str), &Standing> = records
        .iter()
        .map(|one| ((one.knob, one.target.as_str()), &one.standing))
        .collect();
    assert_eq!(
        standing.len(),
        Knob::ALL.len() * passed.len(),
        "one record for every knob and every target whose baseline passed: {records:?}"
    );
    for ((knob, target), one) in &standing {
        let depends = dependent(*knob) == *target;
        let fits = match one {
            Standing::Broke { .. } => depends,
            Standing::Stable | Standing::Passed => !depends,
            Standing::NotPut {
                why:
                    NotPut::ZoneMissing
                    | NotPut::LocaleMissing
                    | NotPut::ShellMissing
                    | NotPut::Platform,
            } => true,
            Standing::NotPut {
                why: NotPut::ThroughCargo,
            } => target.contains("/doc/"),
            Standing::NotPut {
                why: NotPut::NotLibtest,
            }
            | Standing::Moved { .. }
            | Standing::Uncompared { .. }
            | Standing::Unsettled { .. } => false,
        };
        assert!(
            fits,
            "{} on {target}: {one:?}; the knob breaks the one target that depends on it, holds \
             every other, and says why where this machine cannot put it",
            knob.name()
        );
    }
    session.close().expect("close");
}
