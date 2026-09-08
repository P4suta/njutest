// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The branch proofs a real workspace earns, and the ones it does not.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use mjutest_devkit::fixture::Fixture;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::PrepareOptions;
use rust_mutants::testkit::measuring::Measuring;
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::Workspace;

/// A prepared session over `fixture`, measuring coverage or not.
fn prepared(fixture: &Fixture, coverage: bool) -> rust_mutants::session::Session {
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                coverage,
                branch_proofs: true,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares")
}

#[test]
fn the_compiler_vouches_for_a_condition_of_primitives_and_refuses_the_rest() {
    let fixture = Fixture::copy("fixture-coverage");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");

    let proven: Vec<String> = session
        .accepted()
        .iter()
        .filter(|index| session.branch(**index).is_some())
        .filter_map(|index| session.catalog().by_index(*index))
        .map(|mutant| {
            format!(
                "{} {}",
                mutant.candidate.rule.name,
                session.position(mutant).map_or(0, |position| position.line)
            )
        })
        .collect();
    assert!(
        proven.iter().any(|one| one.starts_with("le-to-lt 8")),
        "a condition of primitives earns its proof: {proven:?}"
    );
    assert!(
        proven.iter().any(|one| one.starts_with("le-to-lt 36")),
        "so does a comparison of text, whose comparison is the library's rather than the \
         program's: {proven:?}"
    );
    assert!(
        proven.iter().any(|one| one.starts_with("le-to-lt 44")),
        "and so does one beside a comparison between two different types, both of them the \
         standard library's: the operands are asked about separately: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 16")),
        "a comparison the compiler will not vouch for earns none: {proven:?}"
    );
    assert!(
        !proven.iter().any(|one| one.starts_with("le-to-lt 28")),
        "a condition that runs the program's code earns none: {proven:?}"
    );
    assert!(session.proven() > 0, "{proven:?}");

    let proof = session
        .accepted()
        .iter()
        .find_map(|index| session.branch(*index))
        .expect("a proof");
    assert!(
        proof.body_start.line < proof.body_end.line,
        "the proof names the body it gates: {proof:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_witnessed_tree_is_put_back_before_anything_is_instrumented() {
    let fixture = Fixture::copy("fixture-coverage");
    let before = std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("the source");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &cancel,
        )
        .expect("the session prepares");
    assert_eq!(
        std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("the source"),
        before,
        "the source tree is read-only, whatever the engine writes into its own copy"
    );
    let instrumented =
        std::fs::read_to_string(session.snapshot_root().join("src/lib.rs")).expect("the copy");
    assert!(
        !instrumented.contains("rust-mutants-witness-v1"),
        "the witness tree is taken out before anything is instrumented"
    );
    assert!(
        instrumented.contains("::active("),
        "and what is left is the instrumented tree"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_target_that_never_ran_the_body_is_discharged_by_the_branch_proof() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture, true);
    let mut discharged = 0usize;
    let mut named = Vec::new();
    for mutant in session.catalog().mutants() {
        let route = session.route(mutant);
        for one in route.discharged() {
            discharged = discharged.saturating_add(1);
            named.push(one.proof);
        }
    }
    assert!(
        session.proven() > 0,
        "the fixture holds branches the compiler vouches for"
    );
    assert!(
        discharged > 0,
        "a target during which no statement of the named body ran cannot have observed the \
         mutation, and running it proves nothing and costs a process"
    );
    assert!(
        named.contains(&rust_mutants::session::BRANCH_NEVER_TAKEN),
        "{named:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_mutation_whose_two_branches_never_parted_is_discharged_on_the_baseline_run() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture, false);
    let mut named = Vec::new();
    for mutant in session.catalog().mutants() {
        for one in session.route(mutant).discharged() {
            named.push(one.proof);
        }
    }
    assert!(
        named.contains(&rust_mutants::session::NEVER_INFECTED),
        "a guard the compiler vouched for holds both branches, and a run that never saw them \
         answer differently ran a program the mutation does not change: {named:?}"
    );
    session.close().expect("close");
}

#[test]
fn exec_never_discharges_and_keeps_its_coverage_narrowing() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = prepared(&fixture, true);
    let discharging = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| !session.route(one).discharged().is_empty())
        .expect("a mutant a proof removes a target from");
    let route = session.route(discharging);
    let ran = session
        .exec(
            &rust_mutants::session::Request::new(discharging.display_id.clone()),
            &Cancel::new(),
        )
        .expect("exec");
    let narrowing = route.narrowing().expect("a measured route");
    assert!(
        narrowing.iter().any(|target| target == &ran.target),
        "exec is the question a caller with its own evidence asks, and a discharge is a proof \
         it may not share: {narrowing:?} ran {}",
        ran.target
    );
    assert!(
        route
            .discharged()
            .iter()
            .all(|one| narrowing.contains(&one.target)),
        "so what exec narrows to still holds every target the measurement placed"
    );
    session.close().expect("close");
}

#[test]
fn a_target_whose_probe_never_infected_the_mutant_is_discharged() {
    let fixture = Fixture::copy("fixture-probeable");
    let session = probing(&fixture, true);
    let mut proofs = Vec::new();
    for mutant in session.catalog().mutants() {
        for one in session.route(mutant).discharged() {
            proofs.push(one.proof);
        }
    }
    assert!(
        proofs.contains(&rust_mutants::session::NEVER_INFECTED),
        "a test that ran the mutation and whose value never differed cannot have noticed it: \
         {proofs:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_probe_discharges_without_a_coverage_build_because_the_guards_are_the_measurement() {
    let fixture = Fixture::copy("fixture-probeable");
    let session = probing(&fixture, false);
    let mut proofs = Vec::new();
    for mutant in session.catalog().mutants() {
        for one in session.route(mutant).discharged() {
            proofs.push(one.proof);
        }
    }
    assert!(
        proofs.contains(&rust_mutants::session::NEVER_INFECTED),
        "the premise a proof needs is that this target ran the mutation, and the guards say so \
         on the run that verifies the baseline: {proofs:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_proof_with_nothing_measured_at_all_removes_nothing() {
    let fixture = Fixture::copy("fixture-probeable");
    let session = unmeasured(&fixture);
    for mutant in session.catalog().mutants() {
        assert!(
            session.route(mutant).discharged().is_empty(),
            "a proof without a measurement removes nothing: the lemma is the compiler's or the \
             probe's, and the premise is a measurement's"
        );
    }
    session.close().expect("close");
}

/// A prepared session that measures coverage or does not.
fn probing(fixture: &Fixture, coverage: bool) -> rust_mutants::session::Session {
    measuring(
        fixture,
        if coverage {
            Measuring::BOTH
        } else {
            Measuring::GUARDS
        },
    )
}

/// A prepared session that measures nothing at all, so a proof has no premise to rest on.
fn unmeasured(fixture: &Fixture) -> rust_mutants::session::Session {
    measuring(fixture, Measuring::NOTHING)
}

fn measuring(fixture: &Fixture, measuring: Measuring) -> rust_mutants::session::Session {
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    workspace
        .prepare(&measuring.options(Tier::All), &cancel)
        .expect("the session prepares")
}

#[test]
fn a_target_that_never_entered_the_body_is_discharged_without_a_coverage_build() {
    let fixture = Fixture::copy("fixture-coverage");
    let session = measuring(
        &fixture,
        Measuring {
            coverage: false,
            touch: true,
        },
    );
    let mut proofs = Vec::new();
    for mutant in session.catalog().mutants() {
        for one in session.route(mutant).discharged() {
            proofs.push((one.target.clone(), one.proof));
        }
    }
    assert!(
        proofs
            .iter()
            .any(|(target, proof)| target == "fixture-coverage/test/upper"
                && *proof == rust_mutants::session::BRANCH_NEVER_TAKEN),
        "upper runs the condition of clamp and never the branch it gates, and the marker at the \
         body's first statement says so with no coverage build at all: {proofs:?}"
    );
    session.close().expect("close");
}

/// The candidates of one file, as discovery would report them, with a catalog of them.
fn discovered(path: &str, source: &str) -> rust_mutants::discover::Discovery {
    use rust_mutants::catalog::Builder;
    use rust_mutants::rule::Registry;
    use rust_mutants::syntax::{Selection, discover_file};

    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let found = discover_file(path, source.as_bytes(), &selection).expect("the source parses");
    let mut builder = Builder::new();
    for one in &found.candidates {
        builder.add(one.candidate.clone()).expect("add");
    }
    rust_mutants::discover::Discovery {
        files: Vec::new(),
        candidates: found
            .candidates
            .into_iter()
            .map(|one| rust_mutants::discover::Located {
                found: one,
                package: "fixture-probeable".to_owned(),
            })
            .collect(),
        skips: Vec::new(),
        claims: Vec::new(),
        decisions: Vec::new(),
        catalog: builder.build().expect("catalog"),
    }
}

#[test]
fn a_file_the_witness_tree_does_not_hold_vouches_for_nothing() {
    let fixture = Fixture::copy("fixture-probeable");
    let path = "src/lib.rs";
    let source =
        std::fs::read_to_string(fixture.root().join(path)).expect("the fixture's own source");
    let discovery = discovered(path, &source);
    assert!(
        discovery
            .candidates
            .iter()
            .any(|one| one.found.probe.is_some()),
        "this fixture is the one with probes in it"
    );

    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let nothing = std::collections::BTreeMap::new();
    let established = rust_mutants::prove::establish(
        &rust_mutants::prove::Asking {
            workspace: &workspace,
            discovery: &discovery,
            sources: &nothing,
            options: &PrepareOptions {
                tier: Tier::All,
                branch_proofs: true,
                ..PrepareOptions::default()
            },
        },
        &cancel,
        &rust_mutants::trace::Recorder::disabled(),
    )
    .expect("the pass runs");

    assert!(
        established.probed.is_empty() && established.comparable.is_empty(),
        "with no source to write, the tree the compiler checked was the pristine one, and a \
         check that passes over a file says nothing about anything in it: {established:?}"
    );
    workspace.close().expect("close");
}

#[test]
fn a_condition_the_compiler_takes_is_one_the_pass_vouches_for() {
    let fixture = Fixture::copy("fixture-coverage");
    let path = "src/lib.rs";
    let source =
        std::fs::read_to_string(fixture.root().join(path)).expect("the fixture's own source");
    let discovery = discovered(path, &source);
    assert!(
        discovery
            .candidates
            .iter()
            .any(|one| one.found.comparable.is_some()),
        "this fixture holds conditions the syntax offers a comparison for"
    );

    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(path.to_owned(), source.into_bytes()));
    let established = rust_mutants::prove::establish(
        &rust_mutants::prove::Asking {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            options: &PrepareOptions {
                tier: Tier::All,
                branch_proofs: true,
                ..PrepareOptions::default()
            },
        },
        &cancel,
        &rust_mutants::trace::Recorder::disabled(),
    )
    .expect("the pass runs");

    assert!(
        !established.comparable.is_empty(),
        "a pass that vouched for nothing is one whose every discharge would rest on a question \
         nobody put, and this tree's conditions are ones the compiler takes: {established:?}"
    );
    assert!(
        !established.proofs.is_empty(),
        "and a narrowing comparison names the body it gates, which is what a branch proof rests \
         on: {established:?}"
    );
    workspace.close().expect("close");
}

#[test]
fn a_widening_comparison_is_vouched_for_without_naming_a_body() {
    let fixture = Fixture::copy("fixture-ignored");
    let path = "src/lib.rs";
    let source = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition that proves nothing about the body it gates, and no value to probe.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_is_under_two() {
        let mut out = 0;
        super::under(1, 2, &mut out);
        assert_eq!(out, 1);
    }
}
";
    std::fs::write(fixture.root().join(path), source).expect("the source is the fixture's now");
    let discovery = discovered(path, source);
    assert!(
        discovery
            .candidates
            .iter()
            .all(|one| one.found.probe.is_none()),
        "nothing here returns a value, so the pass is asked no probe at all"
    );
    assert!(
        discovery
            .candidates
            .iter()
            .any(|one| one.found.branch.is_none() && one.found.comparable.is_some()),
        "widening `<` to `<=` proves nothing about the body and still compares"
    );

    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(path.to_owned(), source.as_bytes().to_vec()));
    let established = rust_mutants::prove::establish(
        &rust_mutants::prove::Asking {
            workspace: &workspace,
            discovery: &discovery,
            sources: &sources,
            options: &PrepareOptions {
                tier: Tier::All,
                branch_proofs: true,
                ..PrepareOptions::default()
            },
        },
        &cancel,
        &rust_mutants::trace::Recorder::disabled(),
    )
    .expect("the pass runs");

    assert!(
        !established.comparable.is_empty(),
        "a comparison the compiler vouched for is one a guard may make, whether or not a branch \
         proof rests on the same condition: {established:?}"
    );
    workspace.close().expect("close");
}
