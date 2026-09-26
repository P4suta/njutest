// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whole-workspace discovery over the fixtures: which files are mutable, which are skipped as a whole and why, and the catalog that results.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use njutest_devkit::result::{ResultState::Refused, result_state};
use rust_mutants::cargo::{
    CompileKind, CompileOptions, Driver, LocateOptions, Metadata, MetadataOptions, Toolchain,
    compile,
};
use rust_mutants::discover::{
    DiscoverError, DiscoverOptions, Discovery, Input, discover, freestanding,
};
use rust_mutants::glob::Pattern;
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::runner::Cancel;
use rust_mutants::syntax::{Selection, SkipReason};
use rust_mutants::trace::{DiscoverFileRecord, ExecRecord, MemorySink, Payload, Recorder, Sink};

static REGISTRY: Registry = Registry::canonical();

struct Prepared {
    dir: PathBuf,
    metadata: Metadata,
    checked: rust_mutants::cargo::Compiled,
    _target: tempfile::TempDir,
}

enum RelevantPayload<'a> {
    DiscoverFile(&'a DiscoverFileRecord),
    Exec(&'a ExecRecord),
    Other,
}

const fn relevant_payload(payload: &Payload) -> RelevantPayload<'_> {
    match payload {
        Payload::DiscoverFile { discover } => RelevantPayload::DiscoverFile(discover),
        Payload::Exec { exec } => RelevantPayload::Exec(exec),
        Payload::RunStart { .. }
        | Payload::PhaseStart { .. }
        | Payload::PhaseEnd { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::Instrument { .. }
        | Payload::ValidateRound { .. }
        | Payload::Bisect { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::PerturbedControl { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Route { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::MutantExec { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => RelevantPayload::Other,
    }
}

fn prepare(name: &str) -> Prepared {
    let dir = njutest_devkit::paths::fixtures_dir().join(name);
    let options = LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        ..LocateOptions::default()
    };
    let cancel = Cancel::new();
    let toolchain = Toolchain::locate(&options, &dir, &cancel).expect("locate");
    let trace = Recorder::disabled();
    let driver = Driver {
        toolchain: &toolchain,
        dir: &dir,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let target = tempfile::Builder::new()
        .prefix("rust-mutants-discover-")
        .tempdir()
        .expect("tempdir");
    let checked = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(rust_mutants::cargo::BuildDir::new(
                target.path().to_path_buf(),
                Vec::new(),
            )),
            locked: true,
            offline: true,
            timeout: None,
            env: rust_mutants::vars::Variables::empty(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("check");
    assert!(checked.success, "the fixture compiles");
    Prepared {
        dir,
        metadata,
        checked,
        _target: target,
    }
}

fn options<'r>() -> DiscoverOptions<'r> {
    DiscoverOptions {
        selection: Selection::tier(&REGISTRY, Tier::All),
        include: Vec::new(),
        exclude: Vec::new(),
        narrowing: Vec::new(),
        packages: Vec::new(),
        skips: Vec::new(),
    }
}

fn input(prepared: &Prepared) -> Input<'_> {
    Input {
        root: &prepared.dir,
        metadata: &prepared.metadata,
        units: &prepared.checked.units,
    }
}

fn run(prepared: &Prepared, options: &DiscoverOptions<'_>, trace: &Recorder) -> Discovery {
    discover(&input(prepared), options, trace).expect("discover")
}

/// `(path, package, candidates, "reason:count reason:count")` per file.
fn table(discovery: &Discovery) -> Vec<(String, String, usize, String)> {
    discovery
        .files
        .iter()
        .map(|file| {
            let skips: Vec<String> = file
                .skips
                .iter()
                .map(|skip| format!("{}:{}", skip.reason.name(), skip.count))
                .collect();
            (
                file.path.clone(),
                file.package.clone(),
                file.candidates,
                skips.join(" "),
            )
        })
        .collect()
}

fn row(
    path: &str,
    package: &str,
    candidates: usize,
    skips: &str,
) -> (String, String, usize, String) {
    (
        path.to_owned(),
        package.to_owned(),
        candidates,
        skips.to_owned(),
    )
}

#[test]
fn a_file_only_the_test_unit_compiles_is_a_test_only_file_skip() {
    let prepared = prepare("fixture-simple");
    let discovery = run(&prepared, &options(), &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row(
                "src/lib.rs",
                "fixture-simple",
                13,
                "test-code:22 let-condition:1"
            ),
            row("src/testutil.rs", "fixture-simple", 0, "test-only-file:6"),
        ]
    );
    assert_eq!(discovery.candidates.len(), 13);
    assert_eq!(discovery.catalog.len(), 13);
    let rules: Vec<&str> = discovery
        .candidates
        .iter()
        .map(|c| c.found.candidate.rule.name)
        .collect();
    assert_eq!(
        rules,
        [
            "return-default",
            "negate-condition",
            "condition-to-true",
            "condition-to-false",
            "gt-to-ge",
            "return-default",
            "return-default",
            "return-true",
            "rem-to-mul",
            "int-increment",
            "int-decrement",
            "eq-to-neq",
            "int-increment",
        ]
    );
    assert!(
        discovery
            .candidates
            .iter()
            .all(|c| c.package == "fixture-simple")
    );
    assert!(
        discovery
            .candidates
            .iter()
            .all(|c| c.found.candidate.path == "src/lib.rs")
    );
    let total: Vec<(&str, u32)> = discovery
        .skips
        .iter()
        .map(|s| (s.reason.name(), s.count))
        .collect();
    assert_eq!(
        total,
        [
            ("test-code", 22),
            ("test-only-file", 6),
            ("let-condition", 1)
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_logical_workspace_alias_accepts_the_physical_paths_the_compiler_reports() {
    let prepared = prepare("fixture-simple");
    let aliases = tempfile::tempdir().expect("a directory for the logical alias");
    let alias = aliases.path().join("logical-workspace");
    std::os::unix::fs::symlink(&prepared.dir, &alias).expect("a workspace alias");
    let input = Input {
        root: &alias,
        metadata: &prepared.metadata,
        units: &prepared.checked.units,
    };

    let discovery = discover(&input, &options(), &Recorder::disabled())
        .expect("physical compiler paths are still inside the logical workspace");
    assert_eq!(
        discovery
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["src/lib.rs", "src/testutil.rs"]
    );
}

#[test]
fn a_nested_member_reports_workspace_relative_paths_and_a_binary_is_mutable() {
    let prepared = prepare("fixture-workspace");
    let discovery = run(&prepared, &options(), &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row(
                "crates/app/src/main.rs",
                "fixture-app",
                15,
                "macro-invocation:1"
            ),
            row("crates/core/src/lib.rs", "fixture-core", 13, "test-code:14"),
            row("crates/core/src/util.rs", "fixture-core", 4, ""),
        ]
    );
    assert_eq!(discovery.catalog.len(), 32);
    assert!(discovery.files.iter().all(|f| !f.path.contains("tests/")));
}

#[test]
fn every_rule_fires_in_the_families_fixture_exactly_as_the_golden_says() {
    let prepared = prepare("fixture-families");
    let discovery = run(&prepared, &options(), &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [row(
            "src/lib.rs",
            "fixture-families",
            291,
            "test-code:1 open-range:2 unstated-return-type:2"
        ),]
    );
    assert_eq!(discovery.catalog.len(), 291);
    assert_eq!(discovery.catalog.duplicates().len(), 0);
}

#[test]
fn macro_invocations_count_once_and_a_proc_macro_crate_is_mutated_like_any_other() {
    let prepared = prepare("fixture-macros");
    let discovery = run(&prepared, &options(), &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row(
                "crates/derive/src/lib.rs",
                "fixture-macros-derive",
                21,
                "test-code:8"
            ),
            row("src/lib.rs", "fixture-macros", 5, "macro-invocation:2"),
        ],
        "a proc-macro crate's `--test` build is an ordinary executable, and what its own \
         tests reach is measurable exactly like anything else"
    );
    assert_eq!(discovery.catalog.len(), 26);
}

#[test]
fn a_no_std_crate_the_host_can_lend_std_to_is_measured_like_any_other() {
    let prepared = prepare("fixture-no-std");
    let discovery = run(&prepared, &options(), &Recorder::disabled());

    assert_eq!(
        table(&discovery),
        [row("src/lib.rs", "fixture-no-std", 4, "test-code:15"),],
        "`#![no_std]` withholds the implicit link to std and its prelude, and forbids \
         neither an explicit link nor an explicit path"
    );
    assert_eq!(discovery.catalog.len(), 4);
}

#[test]
fn a_crate_that_supplies_what_std_does_is_skipped_whole_with_its_candidates_counted() {
    let prepared = prepare("fixture-no-std-freestanding");
    let discovery = run(&prepared, &options(), &Recorder::disabled());

    assert_eq!(
        table(&discovery),
        [row(
            "src/lib.rs",
            "fixture-no-std-freestanding",
            0,
            "no-std-crate:2"
        )],
        "std supplies a panic handler too, and only one may exist"
    );
    assert!(discovery.catalog.is_empty());
}

#[test]
fn include_and_exclude_patterns_remove_files_and_count_what_they_hid() {
    let prepared = prepare("fixture-workspace");
    let mut opts = options();
    opts.exclude = vec![Pattern::compile("crates/app/**").expect("pattern")];
    let discovery = run(&prepared, &opts, &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row("crates/app/src/main.rs", "fixture-app", 0, "excluded:15"),
            row("crates/core/src/lib.rs", "fixture-core", 13, "test-code:14"),
            row("crates/core/src/util.rs", "fixture-core", 4, ""),
        ]
    );
    let mut opts = options();
    opts.include = vec![Pattern::compile("**/util.rs").expect("pattern")];
    let discovery = run(&prepared, &opts, &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row("crates/app/src/main.rs", "fixture-app", 0, "excluded:15"),
            row("crates/core/src/lib.rs", "fixture-core", 0, "excluded:13"),
            row("crates/core/src/util.rs", "fixture-core", 4, ""),
        ]
    );
    assert_eq!(discovery.catalog.len(), 4);
}

#[test]
fn selecting_packages_leaves_the_others_out_entirely() {
    let prepared = prepare("fixture-workspace");
    let mut opts = options();
    opts.packages = vec!["fixture-core".to_owned()];
    let discovery = run(&prepared, &opts, &Recorder::disabled());
    assert_eq!(
        table(&discovery),
        [
            row("crates/core/src/lib.rs", "fixture-core", 13, "test-code:14"),
            row("crates/core/src/util.rs", "fixture-core", 4, ""),
        ]
    );
    let mut opts = options();
    opts.packages = vec!["no-such-package".to_owned()];
    let error = discover(&input(&prepared), &opts, &Recorder::disabled());
    assert_eq!(result_state(&error), Refused, "unknown package: {error:?}");
    let Err(error) = error else { return };
    assert!(
        matches!(error, DiscoverError::UnknownPackage { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("RM2007"), "{error}");
}

#[test]
fn the_selection_limits_the_rules_across_the_workspace() {
    let prepared = prepare("fixture-workspace");
    let mut opts = options();
    opts.selection = Selection::rules(&REGISTRY, &["negate-condition"]).expect("rule");
    let discovery = run(&prepared, &opts, &Recorder::disabled());
    assert_eq!(discovery.catalog.len(), 3, "two in clamp, one in main");
    assert!(
        discovery
            .candidates
            .iter()
            .all(|c| c.found.candidate.rule.name == "negate-condition")
    );
}

#[test]
fn discovery_is_deterministic_and_traced_per_file() {
    let prepared = prepare("fixture-workspace");
    let first = run(&prepared, &options(), &Recorder::disabled());
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let second = run(&prepared, &options(), &recorder);
    recorder
        .run_end(rust_mutants::trace::RunOutcome::Completed, None)
        .expect("trace closes");
    assert_eq!(first, second);
    assert_eq!(first.catalog.digest(), second.catalog.digest());
    let files: Vec<(String, u32)> = recorder
        .events()
        .iter()
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::DiscoverFile(discover) => {
                Some((discover.path.clone(), discover.candidates))
            }
            RelevantPayload::Exec(_) | RelevantPayload::Other => None,
        })
        .collect();
    assert_eq!(
        files,
        [
            ("crates/app/src/main.rs".to_owned(), 15),
            ("crates/core/src/lib.rs".to_owned(), 13),
            ("crates/core/src/util.rs".to_owned(), 4),
        ]
    );
}

#[test]
fn a_unit_source_outside_the_root_is_a_whole_file_skip_not_an_error() {
    let prepared = prepare("fixture-simple");
    let mut units = prepared.checked.units.clone();
    units[0]
        .sources
        .push(Path::new("/somewhere/else/table.rs").to_path_buf());
    let discovery = discover(
        &Input {
            root: &prepared.dir,
            metadata: &prepared.metadata,
            units: &units,
        },
        &options(),
        &Recorder::disabled(),
    )
    .expect("a file the build wrote is not a reason to end the run");
    let generated: Vec<&rust_mutants::syntax::Skip> = discovery
        .skips
        .iter()
        .filter(|skip| skip.reason == SkipReason::GeneratedOutsideWorkspace)
        .collect();
    assert_eq!(generated.len(), 1, "{:?}", discovery.skips);
    assert_eq!(
        generated[0].path, "<generated>/table.rs",
        "the report names the file rather than the build directory this run happened to use"
    );
    assert!(
        !discovery.catalog.mutants().is_empty(),
        "and the rest of the tree is measured"
    );
}

#[test]
fn the_check_records_an_exec_event_and_keeps_the_messages() {
    let dir = njutest_devkit::paths::fixtures_dir().join("fixture-simple");
    let options = LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        ..LocateOptions::default()
    };
    let cancel = Cancel::new();
    let toolchain = Toolchain::locate(&options, &dir, &cancel).expect("locate");
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let target = tempfile::tempdir().expect("tempdir");
    let checked = compile(
        &Driver {
            toolchain: &toolchain,
            dir: &dir,
            cancel: &cancel,
            trace: &recorder,
        },
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(rust_mutants::cargo::BuildDir::new(
                target.path().to_path_buf(),
                Vec::new(),
            )),
            locked: true,
            offline: true,
            timeout: None,
            env: rust_mutants::vars::Variables::empty(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("check");
    assert!(checked.success);
    assert_eq!(checked.units.len(), 3);
    assert!(checked.messages.iter().any(|m| matches!(
        m,
        rust_mutants::cargo::Message::BuildFinished { success: true }
    )));
    let execs: Vec<Vec<String>> = recorder
        .events()
        .iter()
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::Exec(exec) => Some(exec.argv.clone()),
            RelevantPayload::DiscoverFile(_) | RelevantPayload::Other => None,
        })
        .collect();
    assert_eq!(execs.len(), 1);
    assert!(execs[0].iter().any(|a| a == "check"), "{execs:?}");
    assert!(
        execs[0].iter().any(|a| a == "--message-format=json"),
        "{execs:?}"
    );
}

#[test]
fn a_file_pasted_in_where_an_expression_goes_is_a_skip_and_not_the_end_of_the_run() {
    let prepared = prepare("fixture-include");
    let discovery = run(&prepared, &options(), &Recorder::disabled());

    assert_eq!(
        table(&discovery),
        [
            row("src/items.rs", "fixture-include", 4, ""),
            row(
                "src/lib.rs",
                "fixture-include",
                10,
                "const-context:1 macro-invocation:1 test-code:7"
            ),
            row(
                "src/table.rs",
                "fixture-include",
                0,
                "included-expression:1"
            ),
        ],
        "a fragment is a place that was not mutated, and a project the compiler is happy \
         with is not one this engine refuses to look at"
    );
}

#[test]
fn what_the_host_cannot_lend_std_to_is_read_out_of_the_crate_root() {
    assert!(
        !freestanding("#![no_std]\npub fn f() {}\n", "2024"),
        "the attribute withholds the implicit link and the prelude, and forbids neither an \
         explicit link nor an explicit path"
    );
    assert!(
        !freestanding("#![cfg_attr(not(test), no_std)]\npub fn f() {}\n", "2024"),
        "under cfg(test), which is how every test of the crate is built, the crate has std"
    );
    assert!(
        freestanding(
            "#![no_std]\n#[panic_handler]\nfn p(_: &core::panic::PanicInfo) -> ! { loop {} }\n",
            "2024"
        ),
        "std supplies a panic handler too, and only one may exist"
    );
    assert!(
        freestanding(
            "#![no_std]\n#[global_allocator]\nstatic A: X = X;\n",
            "2024"
        ),
        "and an allocator too"
    );
    assert!(
        freestanding("#![no_std]\n#![no_main]\npub fn f() {}\n", "2024"),
        "and the entry point"
    );
    assert!(
        freestanding("#![no_std]\npub fn f() {}\n", "2015"),
        "`extern crate` resolves differently in 2015, and the engine does not run what it \
         does not test"
    );
    assert!(
        !freestanding("pub fn f( {}\n", "2024"),
        "a root that does not parse is answered by the walk, which reports the failure"
    );
}

#[test]
fn a_crate_that_forbids_what_the_guards_allow_is_skipped_whole_and_a_crate_that_denies_is_measured()
{
    let prepared = prepare("fixture-forbid");
    let discovery = run(&prepared, &options(), &Recorder::disabled());

    assert_eq!(
        table(&discovery),
        [
            row("denies/src/lib.rs", "denies", 7, "test-code:6"),
            row("forbids/src/lib.rs", "forbids", 0, "forbidden-lints:7"),
        ],
        "forbid is the one level an allow cannot override, and every guard carries an allow"
    );
}

#[test]
fn a_configured_skip_is_counted_per_entry_and_an_unmatched_one_is_reported() {
    let prepared = prepare("fixture-simple");
    let mut opts = options();
    opts.skips = vec![
        rust_mutants::discover::SkipRule {
            path: Pattern::compile("src/lib.rs").expect("pattern"),
            lines: None,
            item: Some("is_even".to_owned()),
            reason: "parity is checked by the integration test".to_owned(),
        },
        rust_mutants::discover::SkipRule {
            path: Pattern::compile("src/nowhere.rs").expect("pattern"),
            lines: None,
            item: None,
            reason: "a file that is not there".to_owned(),
        },
    ];
    let discovery = run(&prepared, &opts, &Recorder::disabled());
    let configured: Vec<(&str, u32)> = discovery
        .skips
        .iter()
        .filter(|skip| skip.reason == SkipReason::Configured)
        .map(|skip| (skip.path.as_str(), skip.count))
        .collect();
    assert_eq!(
        configured,
        [("src/lib.rs", 6)],
        "what the entry hid is counted where the file's other skips are"
    );
    assert!(
        discovery
            .candidates
            .iter()
            .all(|located| located.found.item != "is_even"),
        "and nothing it hid is in the catalog"
    );
    let claims: Vec<(&str, bool)> = discovery
        .claims
        .iter()
        .map(|claim| (claim.reason.as_str(), claim.matched))
        .collect();
    assert_eq!(
        claims,
        [
            ("parity is checked by the integration test", true),
            ("a file that is not there", false),
        ],
        "an entry that hides nothing is a claim about code that has moved or gone"
    );
}
