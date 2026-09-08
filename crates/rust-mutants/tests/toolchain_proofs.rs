// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The branch proofs a real workspace earns, and the ones it does not.

#![expect(
    clippy::expect_used,
    clippy::panic,
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
    discovered_in(&[(path, source)])
}

/// The candidates of several files, in one catalog, as discovery would report them.
fn discovered_in(files: &[(&str, &str)]) -> rust_mutants::discover::Discovery {
    use rust_mutants::catalog::Builder;
    use rust_mutants::rule::Registry;
    use rust_mutants::syntax::{Selection, discover_file};

    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let mut builder = Builder::new();
    let mut candidates = Vec::new();
    for (path, source) in files {
        let found = discover_file(path, source.as_bytes(), &selection).expect("the source parses");
        for one in found.candidates {
            builder.add(one.candidate.clone()).expect("add");
            candidates.push(rust_mutants::discover::Located {
                found: one,
                package: "fixture".to_owned(),
            });
        }
    }
    rust_mutants::discover::Discovery {
        files: Vec::new(),
        candidates,
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
    let recorder = rust_mutants::trace::Recorder::wall(rust_mutants::trace::Sink::Memory(
        rust_mutants::trace::MemorySink::unbounded(),
    ));
    let workspace = Workspace::open(
        fixture.root(),
        rust_mutants::workspace::OpenOptions {
            trace: recorder.clone(),
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
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
        &recorder,
    )
    .expect("the pass runs");

    assert!(
        !established.comparable.is_empty(),
        "a comparison the compiler vouched for is one a guard may make, whether or not a branch \
         proof rests on the same condition: {established:?}"
    );
    said(&recorder, &established);
    workspace.close().expect("close");
}

/// What the pass left in the recording, held to being a sentence somebody can act on.
///
/// The note is the only place a reader learns how much of what the syntax
/// offered the compiler took, so a note that says nothing is a layer nobody
/// can see the yield of.
fn said(recorder: &rust_mutants::trace::Recorder, established: &rust_mutants::prove::Established) {
    use rust_mutants::trace::Payload;

    let events = recorder.events();
    let notes: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::Note { note } if note.kind == "witness" => Some(note.detail.clone()),
            _ => None,
        })
        .collect();
    let [note] = notes.as_slice() else {
        panic!("the pass says once what it established: {notes:?}");
    };
    assert!(
        !note.contains("  ") && !note.trim_end().ends_with(':'),
        "a sentence with a hole where a number should be shows it as two spaces or a colon          promising what never came: {note:?}"
    );
    assert!(
        note.contains(&format!("{} vouched for", established.comparable.len())),
        "and it says the number a reader would otherwise have to take on trust: {note:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            &event.payload,
            Payload::Witness { witness } if witness.checked
        )),
        "a claim the compiler took is recorded as taken, per candidate, because a tally cannot          say which one"
    );
    let unpaired: Vec<rust_mutants::trace::Problem> = rust_mutants::trace::check(&events)
        .into_iter()
        .filter(|problem| !matches!(problem, rust_mutants::trace::Problem::MissingRunEnd))
        .collect();
    assert!(
        unpaired.is_empty(),
        "and the phase that began ended, whatever the run this recording is a fragment of went \
         on to do: {unpaired:?}"
    );
}

/// One `establish` over the files given, with `sources` holding only the ones named.
fn established_over(
    fixture: &Fixture,
    files: &[(&str, &str)],
    holding: &[&str],
) -> rust_mutants::prove::Established {
    recorded_over(fixture, files, holding).0
}

/// [`established_over`], with the recording the pass left.
fn recorded_over(
    fixture: &Fixture,
    files: &[(&str, &str)],
    holding: &[&str],
) -> (
    rust_mutants::prove::Established,
    rust_mutants::trace::Recorder,
) {
    for (path, source) in files {
        if holding.contains(path) {
            std::fs::write(fixture.root().join(path), source).expect("the fixture's own source");
        }
    }
    let discovery = discovered_in(files);
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let mut sources = std::collections::BTreeMap::new();
    for (path, source) in files {
        if holding.contains(path) {
            drop(sources.insert((*path).to_owned(), source.as_bytes().to_vec()));
        }
    }
    let recorder = rust_mutants::trace::Recorder::wall(rust_mutants::trace::Sink::Memory(
        rust_mutants::trace::MemorySink::unbounded(),
    ));
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
        &recorder,
    )
    .expect("the pass runs");
    workspace.close().expect("close");
    (established, recorder)
}

#[test]
fn a_claim_that_names_no_body_does_not_stop_the_pass_looking_at_the_rest() {
    let source = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition whose first comparison proves nothing and whose second proves something.

/// Writes one where both hold.
pub fn both(a: i32, b: i32, c: i32, d: i32, out: &mut i32) {
    if a < b && c <= d {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let established = established_over(&fixture, &[("src/lib.rs", source)], &["src/lib.rs"]);
    assert!(
        established
            .proofs
            .values()
            .any(|proof| proof.marker.is_some()),
        "widening `<` names no body and narrowing `<=` does; a pass that stopped at the first \
         would leave the second without the marker its proof rests on: {established:?}"
    );
}

#[test]
fn a_file_the_pass_cannot_read_does_not_stop_it_writing_the_others() {
    let refusable = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A comparison between a type of the program's own, which the compiler refuses to vouch for.

/// A number the program compares its own way.
#[derive(PartialEq, PartialOrd)]
pub struct Own(pub i32);

/// Writes one where `a` is under `b`.
pub fn under(a: Own, b: Own, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let gone = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A file with a claim of its own, which no source is held for.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let files = [("src/gone.rs", gone), ("src/lib.rs", refusable)];
    assert!(
        discovered_in(&[("src/gone.rs", gone)])
            .candidates
            .iter()
            .any(|one| one.found.comparable.is_some()),
        "and the first file has a claim of its own, so the pass meets a file it cannot read \
         before it reaches the second"
    );
    assert!(
        discovered_in(&files)
            .candidates
            .iter()
            .any(|one| one.found.comparable.is_some()),
        "the second file holds a comparison the syntax offers and the compiler will refuse, \
         which is what makes the difference visible at all"
    );
    let established = established_over(&fixture, &files, &["src/lib.rs"]);
    assert!(
        established.comparable.is_empty(),
        "the first file has no source to write, and a pass that stopped there would leave the \
         second unwritten too - so the compiler would see a pristine tree, say nothing, and \
         every claim of it would be granted on a question nobody put: {established:?}"
    );
}

#[test]
fn a_proof_names_the_braces_of_the_body_and_the_byte_after_the_first() {
    let source = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One narrowing comparison over one body, whose braces are where they look.

/// Writes one where `a` is at most `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a <= b {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let established = established_over(&fixture, &[("src/lib.rs", source)], &["src/lib.rs"]);
    let proof = established
        .proofs
        .values()
        .next()
        .expect("the narrowing comparison names the body it gates");

    let opening = source
        .find("if a <= b {")
        .map_or(0, |at| at + "if a <= b ".len());
    let closing = source.rfind("    }\n}\n").map_or(0, |at| at + "    ".len());
    let index = rust_mutants::syntax::LineIndex::new(source);
    assert_eq!(
        (proof.body_start.line, proof.body_start.byte_column),
        {
            let at = index.position(source, u32::try_from(opening).unwrap_or(0));
            (at.line, at.byte_column)
        },
        "the body starts at its opening brace, which is where a coverage region beginning \
         inside it begins"
    );
    assert_eq!(
        (proof.body_end.line, proof.body_end.byte_column),
        {
            let at = index.position(source, u32::try_from(closing).unwrap_or(0));
            (at.line, at.byte_column)
        },
        "and ends at its closing brace, which the region the compiler emits for what follows \
         the branch sits on; one byte either way and a run that went past the branch reads as \
         one that took it"
    );
    let marker = proof
        .marker
        .expect("the compiler took a marker in this body");
    assert_eq!(
        marker.at,
        u32::try_from(opening).unwrap_or(0).saturating_add(1),
        "and the call sits on the byte after the opening brace, so entering the body is what \
         records it rather than reaching the branch"
    );
}

#[test]
fn what_the_pass_says_it_claimed_is_every_file_added_up() {
    let one = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One condition, so one claim.

/// Writes one where `a` is at most `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a <= b {
        *out = 1;
    }
}
";
    let two = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two conditions, so more than one claim, which is what tells a sum from a product.

/// Writes one where both hold.
pub fn both(a: i32, b: i32, c: i32, d: i32, out: &mut i32) {
    if a <= b {
        *out = 1;
    }
    if c <= d {
        *out = 2;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let files = [("src/lib.rs", one), ("src/other.rs", two)];
    let claimed = discovered_in(&files)
        .candidates
        .iter()
        .filter(|found| found.found.branch.is_some() || found.found.comparable.is_some())
        .count();
    let (_established, recorder) = recorded_over(&fixture, &files, &["src/lib.rs"]);
    let said = recorder
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            rust_mutants::trace::Payload::Note { note } if note.kind == "witness" => {
                Some(note.detail.clone())
            }
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        said.starts_with(&format!("{claimed} claimed")),
        "the tally is every file's claims added up, and a file with one claim beside a file \
         with two is what tells adding from multiplying: {said:?} for {claimed} claims"
    );
}

#[test]
fn a_snapshot_the_pass_cannot_write_into_is_a_failure_rather_than_a_silence() {
    use std::os::unix::fs::PermissionsExt as _;

    let source = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One condition, which the pass would witness if it could write.

/// Writes one where `a` is at most `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a <= b {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let files = [("src/lib.rs", source), ("src/other.rs", source)];
    for (path, text) in files {
        std::fs::write(fixture.root().join(path), text).expect("the fixture's own source");
    }
    let discovery = discovered_in(&files);
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");

    let held = workspace.snapshot_root().join("src/other.rs");
    let before = std::fs::metadata(&held)
        .expect("the snapshot holds it")
        .permissions();
    std::fs::set_permissions(&held, std::fs::Permissions::from_mode(0o444))
        .expect("a file nothing may write");
    let mut sources = std::collections::BTreeMap::new();
    for (path, text) in files {
        drop(sources.insert(path.to_owned(), text.as_bytes().to_vec()));
    }
    let asked = rust_mutants::prove::Asking {
        workspace: &workspace,
        discovery: &discovery,
        sources: &sources,
        options: &PrepareOptions {
            tier: Tier::All,
            branch_proofs: true,
            ..PrepareOptions::default()
        },
    };
    let refused =
        rust_mutants::prove::establish(&asked, &cancel, &rust_mutants::trace::Recorder::disabled());
    drop(std::fs::set_permissions(&held, before));

    let error = refused.expect_err(
        "a tree the pass could not write into is one it must not carry on with: the witnesses \
         are half there, and putting the sources back is what every later phase rests on",
    );
    assert!(
        matches!(
            error,
            rust_mutants::EngineError::Session(
                rust_mutants::workspace::SessionError::WriteFailed { .. }
            )
        ),
        "and it says which file it could not write rather than which claim it could not make: \
         {error}"
    );
    for (path, source) in &sources {
        assert_eq!(
            std::fs::read(workspace.snapshot_root().join(path)).unwrap_or_default(),
            *source,
            "and every file it did write is put back before it reports, because a tree left \
             witnessed is one every later phase would be about the wrong program"
        );
    }
    workspace.close().expect("close");
}

#[test]
fn a_file_no_source_is_held_for_does_not_stop_the_pass_vouching_for_the_rest() {
    let takeable = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A comparison between primitives, which the compiler vouches for.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let files = [("src/gone.rs", takeable), ("src/lib.rs", takeable)];
    let established = established_over(&fixture, &files, &["src/lib.rs"]);
    assert!(
        !established.comparable.is_empty(),
        "the first file has claims and no source, and reading what the compiler took is a walk \
         over every file that has one: stopping at the first without would leave a tree the \
         compiler did take unvouched for: {established:?}"
    );
    assert!(
        established.comparable.len() < 2,
        "and the file it could not write vouches for nothing, which is the other half of the \
         same rule: {established:?}"
    );
}

#[test]
fn a_snapshot_the_pass_cannot_put_back_is_a_failure_rather_than_a_silence() {
    use std::os::unix::fs::PermissionsExt as _;

    let claimed = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition the pass witnesses, in a file it can write.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let fixture = Fixture::copy("fixture-ignored");
    let files = [("src/lib.rs", claimed), ("src/plain.rs", claimed)];
    for (path, text) in files {
        std::fs::write(fixture.root().join(path), text).expect("the fixture's own source");
    }
    let discovery = discovered_in(&[files[0]]);
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");

    let held = workspace.snapshot_root().join("src/plain.rs");
    let before = std::fs::metadata(&held)
        .expect("the snapshot holds it")
        .permissions();
    std::fs::set_permissions(&held, std::fs::Permissions::from_mode(0o444))
        .expect("a file nothing may write");
    let mut sources = std::collections::BTreeMap::new();
    for (path, text) in files {
        drop(sources.insert(path.to_owned(), text.as_bytes().to_vec()));
    }
    let answered = rust_mutants::prove::establish(
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
    );
    drop(std::fs::set_permissions(&held, before));

    assert!(
        answered.is_err(),
        "the file with the claim was written and put back, and one without a claim was not \
         writable at all - so the pass cannot say the tree is the one it was handed, and a \
         pass that answered anyway would hand every later phase a tree nobody checked"
    );
    workspace.close().expect("close");
}

#[test]
fn a_claim_in_a_file_the_tree_could_not_be_given_is_refused_rather_than_vouched_for() {
    let asked = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition discovery found, at an offset a shorter source does not have.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let held = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Nothing.
";
    assert!(
        held.len() < asked.len(),
        "the held source is the shorter one"
    );
    let path = "src/lib.rs";
    let discovery = discovered_in(&[(path, asked)]);
    assert!(
        !discovery.candidates.is_empty(),
        "the longer source is the one with the condition in it"
    );
    let fixture = Fixture::copy("fixture-ignored");
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(path.to_owned(), held.as_bytes().to_vec()));
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
        established.comparable.is_empty(),
        "the tree could not be given the file, so the check that passed never asked about the \
         condition in it, and a claim vouched for on a question nobody put is a mutant removed \
         without a run: {established:?}"
    );
    workspace.close().expect("close");
}

#[test]
fn a_source_the_pass_cannot_write_stops_it_rather_than_letting_it_vouch() {
    use std::os::unix::fs::PermissionsExt as _;

    let claimed = "\
// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A condition the pass would witness, in a file it may not write.

/// Writes one where `a` is under `b`.
pub fn under(a: i32, b: i32, out: &mut i32) {
    if a < b {
        *out = 1;
    }
}
";
    let path = "src/lib.rs";
    let fixture = Fixture::copy("fixture-ignored");
    std::fs::write(fixture.root().join(path), claimed).expect("the fixture's own source");
    let discovery = discovered_in(&[(path, claimed)]);
    let cancel = Cancel::new();
    let workspace = Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &cancel,
    )
    .expect("the workspace opens");

    let held = workspace.snapshot_root().join(path);
    let before = std::fs::metadata(&held)
        .expect("the snapshot holds it")
        .permissions();
    std::fs::set_permissions(&held, std::fs::Permissions::from_mode(0o444))
        .expect("a file nothing may write");
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(path.to_owned(), claimed.as_bytes().to_vec()));
    let answered = rust_mutants::prove::establish(
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
    );
    drop(std::fs::set_permissions(&held, before));

    let Err(refused) = answered else {
        panic!(
            "the tree could not be given the file the claim is in, so the check that would have \
             passed says nothing about it, and vouching for it would remove a mutant on a \
             question nobody put"
        );
    };
    assert!(
        refused.to_string().contains(path),
        "the refusal names the file it could not write: {refused}"
    );
    workspace.close().expect("close");
}
