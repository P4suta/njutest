// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest verify`, end to end, against a real workspace.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A throwaway copy of a fixture, so the run writes its reports somewhere nothing else is reading.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture(name: &str) -> Fixture {
    let source = mjutest_devkit::paths::fixtures_dir().join(name);
    let dir = tempfile::Builder::new()
        .prefix("mjutest-verify-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join(name);
    copy(&source, &root);
    Fixture { root, _dir: dir }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the directory");
    for entry in std::fs::read_dir(from).expect("the fixture") {
        let entry = entry.expect("an entry");
        let kind = entry.file_type().expect("a file type");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).expect("a copy");
        }
    }
}

fn verify(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec!["verify", "--offline", "--locked"];
    args.extend_from_slice(extra);
    Command::new(env!("CARGO_BIN_EXE_mjutest"))
        .args(args)
        .current_dir(&fixture.root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            mjutest_devkit::paths::cache_beside(&fixture.root).expect("a cache directory"),
        )
        .env(
            "TMPDIR",
            mjutest_devkit::paths::temp_beside(&fixture.root).expect("a temporary directory"),
        )
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_string_lossy().as_ref(),
                "PATH" | "HOME" | "RUSTUP_HOME" | "CARGO_HOME" | "TMPDIR"
            )
        }))
        .output()
        .expect("mjutest runs")
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let index = fixture.root.join(mjutest_cli::app::reports::LATEST_ANY);
    let text = std::fs::read_to_string(&index).expect("the latest index");
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let directory = value["directory"].as_str().expect("a directory");
    let path = fixture
        .root
        .join(directory)
        .join(mjutest_cli::app::reports::DOCUMENT_NAME);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).expect("the report is JSON")
}

#[test]
fn a_suite_with_a_gap_it_cannot_see_is_insufficient() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "the verdict is the last record: {stdout}"
    );
    assert!(
        stdout.contains("FINDING\tsurviving-mutant"),
        "and the report names what nobody noticed: {stdout}"
    );
}

#[test]
fn the_report_is_written_where_a_reader_will_look_and_validates_against_the_schema() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let report = document(&fixture);
    let schema_path =
        mjutest_devkit::paths::workspace_root().join("schema/mjutest-assurance-report-v1.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(schema_path).expect("the schema"))
            .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let problems: Vec<String> = validator
        .iter_errors(&report)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(report["verdict"], "INSUFFICIENT");
    assert_eq!(report["accounting"]["targets"]["selected"], 3);
    assert_eq!(report["accounting"]["targets"]["passed"], 2);
    assert_eq!(report["accounting"]["targets"]["skipped"], 1);
    assert_eq!(report["findings"].as_array().expect("findings").len(), 2);
    assert_eq!(
        report["toolchain"]["target"].as_str().unwrap_or_default(),
        report["toolchain"]["target"].as_str().unwrap_or("x"),
        "the triple is recorded"
    );
    assert!(
        report["repository"]["configuration_digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "the effective configuration is identified: {report}"
    );
}

#[test]
fn the_targets_are_named_and_ordered_slowest_first() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let report = document(&fixture);
    let targets = report["targets"].as_array().expect("targets");
    assert_eq!(targets.len(), 3);

    let durations: Vec<u64> = targets
        .iter()
        .map(|target| target["duration_ms"].as_u64().unwrap_or_default())
        .collect();
    let mut sorted = durations.clone();
    sorted.sort_unstable();
    sorted.reverse();
    assert_eq!(durations, sorted, "slowest first: {durations:?}");

    let names: Vec<&str> = targets
        .iter()
        .filter_map(|target| target["name"].as_str())
        .collect();
    assert!(
        names.iter().any(|name| name.contains("sign_names")),
        "{names:?}"
    );
}

#[test]
fn a_run_that_asked_for_a_trace_leaves_one_that_reads_back() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &["--trace"]).status.code(), Some(2));

    let traces = fixture.root.join(".mjutest/trace");
    let recording = std::fs::read_dir(&traces)
        .expect("the trace directory")
        .flatten()
        .map(|entry| entry.path())
        .next()
        .expect("one recording");
    let stream = recording.join(mjutest_cli::trace::FILE_NAME);
    let events = mjutest_cli::trace::read_events(std::io::BufReader::new(
        std::fs::File::open(&stream).expect("the stream"),
    ))
    .expect("the events read back");

    let problems = mjutest_cli::trace::check(&events);
    assert!(problems.is_empty(), "{problems:?}");
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| event.payload.type_name())
        .collect();
    assert_eq!(kinds.first(), Some(&"run-start"));
    assert_eq!(kinds.last(), Some(&"run-end"));
    assert!(kinds.contains(&"exec"), "{kinds:?}");
    assert!(kinds.contains(&"phase-end"), "{kinds:?}");
}

#[test]
fn progress_goes_to_the_error_stream_so_a_redirected_report_is_a_report() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("== baseline"), "{stderr}");
    assert!(
        !stdout.contains("== baseline"),
        "the output stream carries the report alone: {stdout}"
    );
    for line in stdout.lines() {
        let kind = line.split('\t').next().unwrap_or_default();
        assert_eq!(kind, kind.to_uppercase(), "not a record: {line:?}");
    }
}

#[test]
fn the_jsonl_interface_writes_one_object_per_line() {
    let fixture = fixture("fixture-baseline");
    let output = verify(&fixture, &["--ui", "jsonl"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.trim().is_empty(), "it said something");
    for line in stderr.lines() {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("{line:?}: {error}"));
        assert!(value.get("type").is_some(), "{line}");
    }
}

#[test]
fn a_workspace_that_does_not_compile_is_a_defect_that_names_itself() {
    let fixture = fixture("fixture-baseline");
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        b"// SPDX-FileCopyrightText: 2026 mjutest contributors\n\
          // SPDX-License-Identifier: MIT OR Apache-2.0\n\
          //! Broken on purpose.\npub fn sign() -> i32 { \"not an integer\" }\n",
    )
    .expect("break it");

    let output = verify(&fixture, &[]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = document(&fixture);
    assert_eq!(report["verdict"], "DEFECT");
    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["kind"], "build-failure");
    assert!(
        findings[0]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("mismatched types")),
        "the compiler's own words: {findings:?}"
    );
}

#[test]
fn the_report_of_a_known_workspace_is_the_recorded_one() {
    let fixture = fixture("fixture-baseline");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(2));

    let normalized = mjutest_devkit::report::normalize(&document(&fixture));
    let mut text = serde_json::to_string_pretty(&normalized).expect("one document");
    text.push('\n');
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/verify.golden.json");
    mjutest_devkit::golden::golden(&golden, text.as_bytes()).expect("the recorded report");
}

#[test]
fn a_workspace_with_no_tests_at_all_observed_nothing_and_says_so() {
    let repo = mjutest_devkit::repo::Repo::new();
    repo.package("silent")
        .lib("/// Nothing tests this.\npub const fn one() -> i32 {\n    1\n}\n");
    let fixture = Fixture {
        root: repo.root().to_path_buf(),
        _dir: tempfile::Builder::new()
            .prefix("mjutest-unused-")
            .tempdir()
            .expect("a temporary directory"),
    };
    let output = verify(&fixture, &[]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TARGETS\tselected=0"), "{stdout}");
    assert!(
        stdout.ends_with("VERDICT\tINSUFFICIENT\n"),
        "a suite with nothing in it assures nothing: {stdout}"
    );
    drop(repo);
}

#[test]
fn a_second_run_of_the_same_work_reads_the_first_run_back_rather_than_doing_it_again() {
    let fixture = fixture("fixture-assured");
    let first = verify(&fixture, &[]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let established = document(&fixture);
    assert_eq!(established["provenance"]["cached"], false);
    assert_eq!(
        established["provenance"]["source_run_id"],
        serde_json::Value::Null
    );
    let identity = established["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();
    assert_eq!(identity.len(), 64, "{identity}");
    assert!(
        !established["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|one| one["name"] == "workspace-digest-not-computed"),
        "a run that measured the tree does not say it could not: {established}"
    );

    let second = verify(&fixture, &[]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).is_empty(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let reused = document(&fixture);
    assert_eq!(reused["provenance"]["cached"], true, "{reused}");
    assert_eq!(reused["provenance"]["identity"], identity);
    assert_eq!(
        reused["provenance"]["source_run_id"], established["run_id"],
        "a reused answer names the run that established it"
    );
    assert_ne!(reused["run_id"], established["run_id"]);
    assert_eq!(reused["verdict"], established["verdict"]);
    assert_eq!(reused["accounting"], established["accounting"]);

    let afresh = verify(&fixture, &["--no-cache"]);
    assert_eq!(afresh.status.code(), Some(0));
    assert_eq!(
        document(&fixture)["provenance"]["cached"],
        false,
        "a run told to establish everything afresh does"
    );
}

#[test]
fn a_tree_that_changed_is_a_different_question_and_is_answered_again() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let before = document(&fixture)["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();

    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("{source}\n// one more line\n")).expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let after = document(&fixture);
    assert_ne!(
        after["provenance"]["identity"], before,
        "a byte of the tree is part of what the run is about"
    );
    assert_eq!(
        after["provenance"]["cached"], false,
        "nothing was stored for this question yet"
    );
}

#[test]
fn a_run_leaves_nothing_in_the_tree_it_verified_but_its_own_reports() {
    let fixture = fixture("fixture-assured");
    let before = listing(&fixture.root);
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let after = listing(&fixture.root);
    let added: Vec<&String> = after
        .iter()
        .filter(|path| !before.contains(*path))
        .collect();
    assert!(
        added.iter().all(|path| path.starts_with("reports/")),
        "an instrumented build writes its own coverage profiles, and they belong in the \
         directory the run works in rather than in the tree it is about: {added:?}"
    );
}

/// Every file under `root`, as slash-separated relative paths.
fn listing(root: &Path) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push((entry.path(), relative));
            } else {
                found.insert(relative);
            }
        }
    }
    found
}

#[test]
fn a_workspace_that_steps_outside_what_the_compiler_guarantees_says_so() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let safe = document(&fixture);
    assert_eq!(safe["accounting"]["soundness"]["unsafe_items"], 0);
    assert_eq!(safe["accounting"]["soundness"]["packages_with_unsafe"], 0);
    assert_eq!(safe["accounting"]["soundness"]["executed"], false);
    assert!(
        !names(&safe).contains(&"soundness-not-executed".to_owned()),
        "a workspace the compiler vouches for has nothing to say here: {safe}"
    );

    let path = fixture.root.join("tests/doubling.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(
        &path,
        format!(
            "{source}\n\
             /// Never called. The contract counts where the compiler stops vouching; it does \
             not execute it.\n\
             pub fn peek() -> u8 {{\n\
             \x20   unsafe {{ core::ptr::null::<u8>().read() }}\n\
             }}\n"
        ),
    )
    .expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let unsound = document(&fixture);
    assert_eq!(unsound["accounting"]["soundness"]["unsafe_items"], 1);
    assert_eq!(
        unsound["accounting"]["soundness"]["packages_with_unsafe"],
        1
    );
    assert_eq!(
        unsound["accounting"]["soundness"]["executed"], false,
        "this contract counts them rather than executing them"
    );
    assert!(
        names(&unsound).contains(&"soundness-not-executed".to_owned()),
        "a non-empty inventory is a limitation the report states: {unsound}"
    );
}

/// The names of a report's limitations.
fn names(document: &serde_json::Value) -> Vec<String> {
    document["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_run_about_a_change_set_mutates_what_changed_and_claims_no_more_than_that() {
    let fixture = fixture("fixture-assured");
    mjutest_devkit::repo::commit_tree(&fixture.root);

    let unchanged = verify(&fixture, &["--changed"]);
    assert_eq!(
        unchanged.status.code(),
        Some(2),
        "nothing changed, so nothing was asked of the tests: {}",
        String::from_utf8_lossy(&unchanged.stderr)
    );
    let empty = document(&fixture);
    assert_eq!(empty["run_kind"], "changed");
    assert_eq!(empty["accounting"]["mutants"]["cataloged"], 0);
    assert!(
        empty["repository"]["git"]["available"]
            .as_bool()
            .expect("a flag"),
        "{empty}"
    );

    let path = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&path).expect("the source");
    std::fs::write(&path, format!("{source}\n// changed\n")).expect("write");

    let changed = verify(&fixture, &["--changed"]);
    assert_eq!(
        changed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    let document = document(&fixture);
    assert_eq!(document["run_kind"], "changed");
    assert_eq!(
        document["verdict"], "CHANGE_ASSURED",
        "a run that looked at what changed claims no more than that: {document}"
    );
    assert!(
        document["accounting"]["mutants"]["cataloged"]
            .as_u64()
            .expect("a count")
            > 0,
        "{document}"
    );
    assert_eq!(
        document["repository"]["git"]["changed_files"],
        serde_json::json!(["src/lib.rs"])
    );
}

#[test]
fn a_run_about_a_change_set_it_cannot_see_refuses_rather_than_verifying_nothing() {
    let fixture = fixture("fixture-assured");
    let output = verify(&fixture, &["--changed"]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "a tree git cannot be asked about is not a tree in which nothing changed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot see what changed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_target_a_checkpoint_names_is_not_measured_again() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let established = document(&fixture);
    let identity = established["provenance"]["identity"]
        .as_str()
        .expect("an identity")
        .to_owned();
    let first = &established["targets"][0];
    let id = first["id"].as_str().expect("a target id").to_owned();

    let store = mjutest_devkit::paths::cache_beside(&fixture.root)
        .expect("a cache directory")
        .join("mjutest/outcomes-v1");
    std::fs::remove_file(store.join(format!("{identity}.json")))
        .expect("the answer the first run stored");
    let directory = store.join("checkpoints").join(&identity);
    std::fs::create_dir_all(&directory).expect("mkdir");
    std::fs::write(
        directory.join("checkpoint-v1.json"),
        serde_json::to_string(&serde_json::json!({
            "schema": "mjutest-assurance-checkpoint-v1",
            "identity": identity,
            "attempts": 1,
            "targets": [{
                "id": id,
                "status": "failed",
                "duration_ms": 1,
                "message": "what the interrupted run observed",
                "files": ["src/lib.rs"],
            }],
            "mutants": [],
        }))
        .expect("the state renders"),
    )
    .expect("write");

    let resumed = verify(&fixture, &[]);
    assert_eq!(
        resumed.status.code(),
        Some(1),
        "the run took the terminal state the checkpoint carried rather than measuring it \
         again: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let report = document(&fixture);
    assert!(
        names(&report).contains(&"resumed-from-checkpoint".to_owned()),
        "{report}"
    );
    let restored = report["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .find(|target| target["id"] == serde_json::Value::String(id.clone()))
        .expect("the target the checkpoint named");
    assert_eq!(restored["status"], "failed");
    assert_eq!(restored["message"], "what the interrupted run observed");
    assert_eq!(report["verdict"], "DEFECT");
}

#[test]
fn a_target_that_may_behave_differently_establishes_nothing_it_established_before() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let first = document(&fixture);
    assert_eq!(
        first["accounting"]["mutants"]["reused_killed"], 0,
        "the first run established everything itself"
    );
    assert!(
        first["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .all(|one| one["reused"] == false),
        "{first}"
    );

    // The tree is the same tree, so every mutant has the identity it had and
    // every record is there. What the harness is told is not the same, so no
    // target behaves the way the records say it did.
    let differently = verify(&fixture, &["--", "--test-threads=1"]);
    assert_eq!(
        differently.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&differently.stderr)
    );
    let second = document(&fixture);
    assert_ne!(
        second["provenance"]["identity"], first["provenance"]["identity"],
        "what the harness is told is part of what the run is about"
    );
    assert_eq!(second["provenance"]["cached"], false);
    assert_eq!(
        second["accounting"]["mutants"]["reused_killed"], 0,
        "a target that may behave differently established nothing it established before: \
         {second}"
    );
}

#[test]
fn what_changed_outside_a_package_does_not_make_its_own_evidence_stale() {
    let fixture = fixture("fixture-assured");
    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let first = document(&fixture);
    let killed = first["accounting"]["mutants"]["killed"]
        .as_u64()
        .expect("a count");
    assert!(killed > 0);

    // A file no package owns changes the tree and therefore the run's
    // identity, but not what any target links.
    std::fs::write(
        fixture.root.join("NOTES.md"),
        "nothing to do with the code\n",
    )
    .expect("write");

    assert_eq!(verify(&fixture, &[]).status.code(), Some(0));
    let second = document(&fixture);
    assert_ne!(
        second["provenance"]["identity"],
        first["provenance"]["identity"]
    );
    assert_eq!(
        second["accounting"]["mutants"]["reused_killed"], killed,
        "every kill was established by a test that still reaches the mutant and still \
         behaves the same: {second}"
    );
    assert!(
        second["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|one| one["outcome"] == "killed")
            .all(|one| one["reused"] == true && one["source_run_id"] == first["run_id"]),
        "a reused verdict names the run that established it: {second}"
    );
}

#[test]
fn the_documentation_of_a_library_is_run_as_one_target_and_routes_nothing() {
    let fixture = fixture("fixture-doctest");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let documentation: Vec<&serde_json::Value> = report["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter(|target| {
            target["name"]
                .as_str()
                .is_some_and(|name| name.contains("/doc/"))
        })
        .collect();
    assert_eq!(
        documentation.len(),
        1,
        "one target per library, whatever the library documents: {:?}",
        report["targets"]
    );
    assert_eq!(
        documentation[0]["status"], "passed",
        "a documented example that does not hold is a failing test rather than something \
         nobody looked at"
    );

    let stated: Vec<&str> = report["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        stated.contains(&"doctests-not-routed"),
        "the target carries no coverage, so nothing is routed to it and the report says so: \
         {stated:?}"
    );

    let killers: Vec<&str> = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter_map(|mutant| mutant["killed_by"].as_str())
        .collect();
    assert!(
        !killers.iter().any(|by| by.contains("/doc/")),
        "a target no mutation is routed to answers for no mutation: {killers:?}"
    );
}

#[test]
fn a_documented_example_that_does_not_hold_is_a_failing_test() {
    let fixture = fixture("fixture-doctest");
    let library = fixture.root.join("src/lib.rs");
    let source = std::fs::read_to_string(&library).expect("the library");
    std::fs::write(
        &library,
        source.replace(
            "/// assert_eq!(fixture_doctest::double(2), 4);",
            "/// assert_eq!(fixture_doctest::double(2), 5);",
        ),
    )
    .expect("an example that does not hold");

    verify(&fixture, &[]);
    let report = document(&fixture);

    assert_eq!(
        report["verdict"], "DEFECT",
        "documentation that lies about the library is a defect in the library or in the \
         documentation, and either way it is not something to measure mutations against"
    );
    let failing: Vec<&str> = report["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["kind"] == "failing-test")
        .filter_map(|finding| finding["subject"].as_str())
        .collect();
    assert!(
        failing.iter().any(|subject| subject.contains("/doc/")),
        "the finding names the documentation as the test that failed: {failing:?}"
    );
}

#[test]
fn a_library_that_documents_no_example_is_not_a_target_that_ran_nothing() {
    let fixture = fixture("fixture-baseline");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let names: Vec<&str> = report["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .filter_map(|target| target["name"].as_str())
        .collect();
    assert!(
        !names.iter().any(|name| name.contains("/doc/")),
        "one target per library is one target per library that documents something; \
         reporting a missing target would raise a finding about documentation nobody \
         wrote: {names:?}"
    );
    let stated: Vec<&str> = report["limitations"]
        .as_array()
        .expect("limitations")
        .iter()
        .filter_map(|one| one["name"].as_str())
        .collect();
    assert!(
        !stated.contains(&"doctests-not-routed"),
        "and nothing was left out of the routing either: {stated:?}"
    );
}

#[test]
fn a_mutation_only_another_process_reaches_is_settled_by_the_suite_that_reaches_it() {
    let fixture = fixture("fixture-subprocess");
    verify(&fixture, &[]);
    let report = document(&fixture);

    let mutants = report["mutants"].as_array().expect("mutants");
    let outcomes: Vec<&str> = mutants
        .iter()
        .filter_map(|mutant| mutant["outcome"].as_str())
        .collect();
    assert!(
        !outcomes.contains(&"unreached"),
        "nothing links the library into the test binary, so no region of it is \
         instrumented and the coverage is silent rather than empty: {outcomes:?}"
    );
    assert_eq!(
        outcomes.iter().filter(|one| **one == "killed").count(),
        2,
        "the test runs the binary, the binary calls the library, and two of the three \
         mutations make it say something else: {outcomes:?}"
    );
    assert_eq!(
        report["verdict"], "INSUFFICIENT",
        "one mutation nothing noticed is a gap in the suite, and neither of the other \
         two is a test that fails on the original code"
    );

    let killers: Vec<&str> = mutants
        .iter()
        .filter_map(|mutant| mutant["killed_by"].as_str())
        .collect();
    assert!(
        killers
            .iter()
            .all(|by| by.contains("through_the_binary") || by.contains("package-suite")),
        "a kill the suite found still names the target that found it: {killers:?}"
    );
}
