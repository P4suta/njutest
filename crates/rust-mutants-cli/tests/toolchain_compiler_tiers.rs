// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every fixture compared across compiler tiers, including verdicts, fuel and complete observations.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a differential oracle refuses incomplete observations"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use njutest_devkit::fixture::Fixture;
use rust_mutants::run::{Sealing, sealed_verdict};
use rust_mutants::runner::Cancel;
use rust_mutants::sealed::{CompilerTier, SealedRunner};
use rust_mutants::trace::Recorder;
use rust_mutants::workspace::Workspace;
use rust_mutants_cli::{Environment, cli, settings::Settings};
use serde_json::{Value, json};

/// All observations of one tier, excluding only the configuration-dependent identity envelope.
#[derive(Debug, PartialEq, Eq)]
struct Observation {
    controls: String,
    verdicts: BTreeMap<String, Value>,
    transcripts: Vec<String>,
}

/// Reads every full transcript, preserving multiplicity and every observable field.
fn transcripts(directory: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(directory).expect("the differential transcript store") {
        let path = entry.expect("a stored transcript").path();
        let bytes = std::fs::read(&path).expect("a complete transcript");
        let record: Value =
            njutest_devkit::strictjson::decode_slice(&bytes).expect("a valid transcript");
        let mut transcript = record
            .get("transcript")
            .expect("the full observation")
            .clone();
        let object = transcript
            .as_object_mut()
            .expect("the transcript is an object");
        assert!(object.remove("invocation").is_some());
        assert!(object.remove("digest").is_some());
        found.push(serde_json::to_string(&transcript).expect("an observation is JSON"));
        std::fs::remove_file(path).expect("tiers have separate stores");
    }
    found.sort();
    found
}

/// Executes listings, controls and every accepted mutant with the same prepared module bytes.
fn observe(
    session: &rust_mutants::session::Session,
    tier: CompilerTier,
    store: &Path,
    cancel: &Cancel,
) -> Observation {
    let runner = SealedRunner::with_compiler(Duration::from_secs(120), tier, None)
        .expect("the deterministic compiler tier");
    let bench = session
        .bench(&runner, cancel)
        .expect("the tier assembles the same sealed targets");
    let controls = format!(
        "{:?}",
        bench
            .stations
            .iter()
            .map(|(target, station)| (target, &station.controls))
            .collect::<BTreeMap<_, _>>()
    );
    let mut verdicts = BTreeMap::new();
    for mutant in session.catalog().mutants() {
        if !session.was_validated(mutant.index) {
            continue;
        }
        let verdict = match sealed_verdict(session, mutant, &bench)
            .expect("the tier answers every accepted mutant")
        {
            Sealing::Established(judged) => json!({
                "outcome": judged.outcome, "exit_code": judged.exit_code,
                "not_run_reason": judged.not_run_reason, "tests_run": judged.tests_run,
                "failed_tests": judged.failed_tests, "declined": judged.declined,
                "evidence": judged.evidence,
            }),
            Sealing::Unproven(evidence) => json!({"unproven": evidence}),
            Sealing::Interrupted => panic!("a differential run cannot be interrupted"),
        };
        let old = verdicts.insert(mutant.id.to_string(), verdict);
        assert!(old.is_none());
    }
    println!("{tier:?}: {:?}", runner.spent());
    Observation {
        controls,
        verdicts,
        transcripts: transcripts(store),
    }
}

/// Resolves fixture recipes against their complete copied sibling layout.
fn arguments(fixture: &Fixture, recipe: &[String]) -> Vec<OsString> {
    let mut args = vec![
        "rust-mutants".to_owned(),
        "run".to_owned(),
        "--offline".to_owned(),
        "--locked".to_owned(),
    ];
    if !recipe
        .iter()
        .any(|arg| arg == "--tier" || arg.starts_with("--tier="))
    {
        args.extend(["--tier".to_owned(), "all".to_owned()]);
    }
    args.extend(
        recipe
            .iter()
            .filter(|arg| arg.as_str() != "--no-seal")
            .map(|arg| {
                if arg.starts_with("../") && arg.trim_start_matches("../").starts_with("fixture-") {
                    njutest_devkit::paths::utf8(
                        &std::fs::canonicalize(fixture.root().join(arg)).expect("a copied sibling"),
                    )
                    .to_owned()
                } else {
                    arg.clone()
                }
            }),
    );
    args.into_iter().map(OsString::from).collect()
}

/// Both tiers see every fixture's content, recipe, controls and accepted mutations.
fn compare(name: &str) {
    let readme = std::fs::read_to_string(
        njutest_devkit::paths::fixtures_dir()
            .join(name)
            .join("README.md"),
    )
    .expect("the fixture recipe");
    let recipe = njutest_devkit::fixture::fates(&readme);
    let siblings: Vec<_> = recipe
        .args
        .iter()
        .filter(|arg| arg.starts_with("../"))
        .map(|arg| arg.trim_start_matches("../"))
        .filter(|arg| arg.starts_with("fixture-") && !arg.contains('/'))
        .collect();
    let fixture = Fixture::copy_with_siblings(name, &siblings);
    let environment = Environment {
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[])
            .into_iter()
            .collect(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::env::current_exe().expect("the test executable"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    };
    let args = arguments(&fixture, &recipe.args);
    let parsed = cli::parse(args).expect("the fixture recipe parses");
    let cli::Command::Run { scope, .. } = parsed.command else {
        panic!("the recipe is a run");
    };
    let settings = Settings::resolve(&scope, &environment).expect("the fixture's settings");
    let cancel = Cancel::new();
    let options = settings
        .open_options(&scope, &environment, Recorder::disabled())
        .expect("open options");
    let mut preparation = settings.prepare_options().expect("prepare options");
    preparation.verify = false;
    let store = fixture.cache().join("tier-transcripts");
    std::fs::create_dir_all(&store).expect("the observations can be stored");
    preparation.transcripts = Some(store.clone());
    let workspace = Workspace::open(fixture.root(), options, &cancel).expect("the fixture opens");
    let session = match workspace.prepare(&preparation, &cancel) {
        Ok(session) => session,
        Err(error) => {
            assert!(
                recipe.rows.is_empty() || recipe.rows.iter().all(|row| row.outcome == "refused"),
                "{name} unexpectedly refused both tiers before compiling guests: {error}"
            );
            println!("{name}: both tiers refused before any guest exists: {error}");
            return;
        }
    };
    let optimized = observe(&session, CompilerTier::Optimized, &store, &cancel);
    let unoptimized = observe(&session, CompilerTier::Unoptimized, &store, &cancel);
    if optimized == unoptimized {
        println!("{name}: both tiers have identical verdicts, fuel and full transcripts");
    } else {
        assert_eq!(
            CompilerTier::faithful(),
            CompilerTier::Optimized,
            "the cheaper compiler changes an observation of {name}: {optimized:#?} / {unoptimized:#?}"
        );
        println!(
            "{name}: unoptimized differs; controls={}, verdicts={}, transcripts={}",
            optimized.controls != unoptimized.controls,
            optimized.verdicts != unoptimized.verdicts,
            optimized.transcripts != unoptimized.transcripts
        );
    }
    let kept = session
        .close()
        .expect("the private execution state is removed");
    assert!(kept.is_empty());
}

const FIXTURES: &[&str] = &[
    "fixture-2021",
    "fixture-absolute-path",
    "fixture-annotated",
    "fixture-apparatus",
    "fixture-assured",
    "fixture-balanced-fails-then-hangs",
    "fixture-bare-cargo",
    "fixture-baseline",
    "fixture-build-script",
    "fixture-carry",
    "fixture-child-refuses",
    "fixture-cleared-child",
    "fixture-cleared-under-mutant",
    "fixture-climbs-dep-lib",
    "fixture-const-fn",
    "fixture-const-items",
    "fixture-coverage",
    "fixture-custom-harness",
    "fixture-declines",
    "fixture-doctest",
    "fixture-doctest-alone",
    "fixture-doctest-host-only",
    "fixture-doctest-refused",
    "fixture-drifts",
    "fixture-drifts-sealed",
    "fixture-durable",
    "fixture-durable-calls",
    "fixture-edits",
    "fixture-entered",
    "fixture-environment",
    "fixture-equivalent",
    "fixture-escapes",
    "fixture-fails-then-hangs",
    "fixture-families",
    "fixture-faulted",
    "fixture-faulted-failure-writes",
    "fixture-faulted-ignore",
    "fixture-faulted-writes",
    "fixture-features",
    "fixture-forbid",
    "fixture-guarded-or",
    "fixture-hang",
    "fixture-hollow",
    "fixture-hollow-only",
    "fixture-home",
    "fixture-ignored",
    "fixture-include",
    "fixture-integration-bodies",
    "fixture-item-reach",
    "fixture-killer-last",
    "fixture-links-nowhere",
    "fixture-macros",
    "fixture-modern",
    "fixture-no-std",
    "fixture-no-std-freestanding",
    "fixture-order-dependent",
    "fixture-outside",
    "fixture-outside-dep",
    "fixture-outside-dep-lib",
    "fixture-panics",
    "fixture-probeable",
    "fixture-reads-its-source",
    "fixture-reads-tree",
    "fixture-rejectable",
    "fixture-scheduled",
    "fixture-scripted",
    "fixture-shared-path",
    "fixture-silent-kill",
    "fixture-simple",
    "fixture-stop-status",
    "fixture-strict-lints",
    "fixture-subprocess",
    "fixture-target-tmpdir",
    "fixture-targets",
    "fixture-temporary",
    "fixture-threaded",
    "fixture-two-bodies",
    "fixture-uncompiled",
    "fixture-unicode",
    "fixture-unreached",
    "fixture-verify-fails",
    "fixture-wired",
    "fixture-witness-downstream",
    "fixture-workspace",
    "fixture-writes-tree",
    "nested/fixture-climbs-dep",
];

#[test]
fn fixture_2021() {
    compare("fixture-2021");
}

#[test]
fn fixture_absolute_path() {
    compare("fixture-absolute-path");
}

#[test]
fn fixture_annotated() {
    compare("fixture-annotated");
}

#[test]
fn fixture_apparatus() {
    compare("fixture-apparatus");
}

#[test]
fn fixture_assured() {
    compare("fixture-assured");
}

#[test]
fn fixture_balanced_fails_then_hangs() {
    compare("fixture-balanced-fails-then-hangs");
}

#[test]
fn fixture_bare_cargo() {
    compare("fixture-bare-cargo");
}

#[test]
fn fixture_baseline() {
    compare("fixture-baseline");
}

#[test]
fn fixture_build_script() {
    compare("fixture-build-script");
}

#[test]
fn fixture_carry() {
    compare("fixture-carry");
}

#[test]
fn fixture_child_refuses() {
    compare("fixture-child-refuses");
}

#[test]
fn fixture_cleared_child() {
    compare("fixture-cleared-child");
}

#[test]
fn fixture_cleared_under_mutant() {
    compare("fixture-cleared-under-mutant");
}

#[test]
fn fixture_climbs_dep_lib() {
    compare("fixture-climbs-dep-lib");
}

#[test]
fn fixture_const_fn() {
    compare("fixture-const-fn");
}

#[test]
fn fixture_const_items() {
    compare("fixture-const-items");
}

#[test]
fn fixture_coverage() {
    compare("fixture-coverage");
}

#[test]
fn fixture_custom_harness() {
    compare("fixture-custom-harness");
}

#[test]
fn fixture_declines() {
    compare("fixture-declines");
}

#[test]
fn fixture_doctest() {
    compare("fixture-doctest");
}

#[test]
fn fixture_doctest_alone() {
    compare("fixture-doctest-alone");
}

#[test]
fn fixture_doctest_host_only() {
    compare("fixture-doctest-host-only");
}

#[test]
fn fixture_doctest_refused() {
    compare("fixture-doctest-refused");
}

#[test]
fn fixture_drifts() {
    compare("fixture-drifts");
}

#[test]
fn fixture_drifts_sealed() {
    compare("fixture-drifts-sealed");
}

#[test]
fn fixture_durable() {
    compare("fixture-durable");
}

#[test]
fn fixture_durable_calls() {
    compare("fixture-durable-calls");
}

#[test]
fn fixture_edits() {
    compare("fixture-edits");
}

#[test]
fn fixture_entered() {
    compare("fixture-entered");
}

#[test]
fn fixture_environment() {
    compare("fixture-environment");
}

#[test]
fn fixture_equivalent() {
    compare("fixture-equivalent");
}

#[test]
fn fixture_escapes() {
    compare("fixture-escapes");
}

#[test]
fn fixture_fails_then_hangs() {
    compare("fixture-fails-then-hangs");
}

#[test]
fn fixture_families() {
    compare("fixture-families");
}

#[test]
fn fixture_faulted() {
    compare("fixture-faulted");
}

#[test]
fn fixture_faulted_failure_writes() {
    compare("fixture-faulted-failure-writes");
}

#[test]
fn fixture_faulted_ignore() {
    compare("fixture-faulted-ignore");
}

#[test]
fn fixture_faulted_writes() {
    compare("fixture-faulted-writes");
}

#[test]
fn fixture_features() {
    compare("fixture-features");
}

#[test]
fn fixture_forbid() {
    compare("fixture-forbid");
}

#[test]
fn fixture_guarded_or() {
    compare("fixture-guarded-or");
}

#[test]
fn fixture_hang() {
    compare("fixture-hang");
}

#[test]
fn fixture_hollow() {
    compare("fixture-hollow");
}

#[test]
fn fixture_hollow_only() {
    compare("fixture-hollow-only");
}

#[test]
fn fixture_home() {
    compare("fixture-home");
}

#[test]
fn fixture_ignored() {
    compare("fixture-ignored");
}

#[test]
fn fixture_include() {
    compare("fixture-include");
}

#[test]
fn fixture_integration_bodies() {
    compare("fixture-integration-bodies");
}

#[test]
fn fixture_item_reach() {
    compare("fixture-item-reach");
}

#[test]
fn fixture_killer_last() {
    compare("fixture-killer-last");
}

#[test]
fn fixture_links_nowhere() {
    compare("fixture-links-nowhere");
}

#[test]
fn fixture_macros() {
    compare("fixture-macros");
}

#[test]
fn fixture_modern() {
    compare("fixture-modern");
}

#[test]
fn fixture_no_std() {
    compare("fixture-no-std");
}

#[test]
fn fixture_no_std_freestanding() {
    compare("fixture-no-std-freestanding");
}

#[test]
fn fixture_order_dependent() {
    compare("fixture-order-dependent");
}

#[test]
fn fixture_outside() {
    compare("fixture-outside");
}

#[test]
fn fixture_outside_dep() {
    compare("fixture-outside-dep");
}

#[test]
fn fixture_outside_dep_lib() {
    compare("fixture-outside-dep-lib");
}

#[test]
fn fixture_panics() {
    compare("fixture-panics");
}

#[test]
fn fixture_probeable() {
    compare("fixture-probeable");
}

#[test]
fn fixture_reads_its_source() {
    compare("fixture-reads-its-source");
}

#[test]
fn fixture_reads_tree() {
    compare("fixture-reads-tree");
}

#[test]
fn fixture_rejectable() {
    compare("fixture-rejectable");
}

#[test]
fn fixture_scheduled() {
    compare("fixture-scheduled");
}

#[test]
fn fixture_scripted() {
    compare("fixture-scripted");
}

#[test]
fn fixture_shared_path() {
    compare("fixture-shared-path");
}

#[test]
fn fixture_silent_kill() {
    compare("fixture-silent-kill");
}

#[test]
fn fixture_simple() {
    compare("fixture-simple");
}

#[test]
fn fixture_stop_status() {
    compare("fixture-stop-status");
}

#[test]
fn fixture_strict_lints() {
    compare("fixture-strict-lints");
}

#[test]
fn fixture_subprocess() {
    compare("fixture-subprocess");
}

#[test]
fn fixture_target_tmpdir() {
    compare("fixture-target-tmpdir");
}

#[test]
fn fixture_targets() {
    compare("fixture-targets");
}

#[test]
fn fixture_temporary() {
    compare("fixture-temporary");
}

#[test]
fn fixture_threaded() {
    compare("fixture-threaded");
}

#[test]
fn fixture_two_bodies() {
    compare("fixture-two-bodies");
}

#[test]
fn fixture_uncompiled() {
    compare("fixture-uncompiled");
}

#[test]
fn fixture_unicode() {
    compare("fixture-unicode");
}

#[test]
fn fixture_unreached() {
    compare("fixture-unreached");
}

#[test]
fn fixture_verify_fails() {
    compare("fixture-verify-fails");
}

#[test]
fn fixture_wired() {
    compare("fixture-wired");
}

#[test]
fn fixture_witness_downstream() {
    compare("fixture-witness-downstream");
}

#[test]
fn fixture_workspace() {
    compare("fixture-workspace");
}

#[test]
fn fixture_writes_tree() {
    compare("fixture-writes-tree");
}

#[test]
fn nested_fixture_climbs_dep() {
    compare("nested/fixture-climbs-dep");
}

#[test]
fn every_fixture_is_compared_by_the_compiler_oracle() {
    assert_eq!(njutest_devkit::fixture::names(), FIXTURES);
    assert_eq!(
        CompilerTier::ALL,
        [CompilerTier::Optimized, CompilerTier::Unoptimized]
    );
}
