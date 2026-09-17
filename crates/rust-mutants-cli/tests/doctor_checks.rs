// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the doctor answers, asked in this process about trees arranged to make it answer.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reads a document by the names the command it drove put there"
)]
#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "the helpers that drive one command and find one check in what it answered are \
              not themselves tests: a document that will not parse leaves nothing to assert, \
              and a check that is not there is best said by naming the ones that are"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What one `doctor` said, driven in this process.
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

fn asked(environment: &Environment, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// The document `doctor --json` answers with, for the tree `environment` names.
fn document(environment: &Environment) -> serde_json::Value {
    let root = environment.working_directory.to_string_lossy().into_owned();
    let said = asked(environment, &["doctor", "--json", "--root", &root]);
    serde_json::from_str(&said.out).unwrap_or_else(|error| {
        panic!(
            "the doctor answers as a document, and this one did not: {error}\n{}{}",
            said.out, said.err
        )
    })
}

/// The check named `name`, or a panic naming every check that was there instead.
fn check<'a>(document: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    let checks = document["checks"]
        .as_array()
        .expect("a doctor answers with its checks");
    checks
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| {
            let named: Vec<&str> = checks
                .iter()
                .filter_map(|check| check["name"].as_str())
                .collect();
            panic!("no check is called {name}; these are: {}", named.join(", "))
        })
}

fn standing(document: &serde_json::Value, name: &str) -> String {
    check(document, name)["status"]
        .as_str()
        .expect("a check says how it stands")
        .to_owned()
}

#[test]
fn a_doctor_names_every_check_a_run_needs_and_the_lines_say_what_the_document_does() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let document = document(&environment);

    assert_eq!(document["document_type"], "rust-mutants/doctor");
    assert_eq!(document["schema_version"], 1);
    assert_eq!(
        document["tool_version"],
        rust_mutants::VERSION,
        "and which engine answered, because a check that passed in another release \
         answers about that one"
    );

    let checks = document["checks"]
        .as_array()
        .expect("a doctor answers with its checks");
    let named: Vec<&str> = checks
        .iter()
        .filter_map(|check| check["name"].as_str())
        .collect();
    for wanted in [
        "cargo",
        "rustc",
        "host",
        "workspace",
        "config",
        "temp",
        "git",
        "targets",
        "environment",
        "cache",
        "disk",
        "snapshots",
        "llvm-tools",
        "guards",
    ] {
        assert!(
            named.contains(&wanted),
            "a run that will not start is usually one of these, and {wanted} is not among \
             the ones this doctor looked at: {}",
            named.join(", ")
        );
    }

    let root = environment.working_directory.to_string_lossy().into_owned();
    let lines = asked(&environment, &["doctor", "--root", &root]);
    for check in checks {
        let name = check["name"].as_str().expect("a check has a name");
        let status = check["status"].as_str().expect("a check has a standing");
        assert!(
            lines.out.contains(name) && lines.out.contains(&status.to_uppercase()),
            "the lines a person reads say what the document says, and this one lost \
             {name} standing {status}:\n{}",
            lines.out
        );
        if let Some(remedy) = check["remedy"].as_str() {
            assert!(
                lines.out.contains(remedy),
                "including what to do about it, which is the half of a check that is \
                 worth reading: {remedy}\n{}",
                lines.out
            );
        }
    }
}

#[test]
fn a_reserved_variable_that_is_already_set_is_a_failure_that_names_it() {
    let fixture = Fixture::copy("fixture-simple");
    let mut environment = environment(&fixture);
    environment.vars.push((
        OsString::from("RUST_MUTANTS_CATALOG"),
        OsString::from("/nowhere/somebody-elses-catalog.json"),
    ));

    let document = document(&environment);
    assert_eq!(
        standing(&document, "environment"),
        "fail",
        "a run composes the activation itself, so one already in the environment makes \
         every answer an answer about something else: {document}"
    );
    let environment_check = check(&document, "environment");
    assert!(
        environment_check["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("RUST_MUTANTS_CATALOG")),
        "and the check names the variable, because there are three of them and unsetting \
         the wrong one leaves the run refusing: {environment_check}"
    );
    assert_eq!(
        document["ok"], false,
        "a document is well only when every check is"
    );

    let root = environment.working_directory.to_string_lossy().into_owned();
    let said = asked(&environment, &["doctor", "--root", &root]);
    assert_eq!(
        said.code,
        rust_mutants_cli::EXIT_USAGE,
        "and a doctor that found something a run cannot start with says so in the code \
         a script reads: {}{}",
        said.out,
        said.err
    );
}

#[test]
fn no_reserved_variable_set_is_the_check_passing_rather_than_the_check_not_looking() {
    let fixture = Fixture::copy("fixture-simple");
    let document = document(&environment(&fixture));
    assert_eq!(standing(&document, "environment"), "ok");
    assert!(
        check(&document, "environment")["remedy"].is_null(),
        "a check with nothing wrong carries no remedy, or a person reads a list of \
         things to do that are already done"
    );
}

#[test]
fn a_root_below_the_workspace_is_a_failure_that_names_the_workspace_above_it() {
    let fixture = Fixture::copy("fixture-workspace");
    let mut environment = environment(&fixture);
    environment.working_directory = fixture.root().join("crates").join("core");

    let document = document(&environment);
    assert_eq!(
        standing(&document, "workspace"),
        "fail",
        "a run at a member measures that member's tests against that member's mutations \
         and calls the workspace measured: {document}"
    );
    let detail = check(&document, "workspace")["detail"]
        .as_str()
        .expect("the check says what it found")
        .to_owned();
    assert!(
        detail.contains("member of") && detail.contains("fixture-workspace"),
        "and says which workspace it is a member of, which is the argument to --root: \
         {detail}"
    );
}

#[test]
fn a_root_that_is_the_workspace_is_the_check_passing() {
    let fixture = Fixture::copy("fixture-workspace");
    let document = document(&environment(&fixture));
    assert_eq!(standing(&document, "workspace"), "ok");
}

#[test]
fn a_root_with_no_manifest_in_it_is_a_failure_naming_the_manifest() {
    let fixture = Fixture::copy("fixture-simple");
    let empty = fixture.temp().join("no-project-here");
    std::fs::create_dir_all(&empty).expect("a directory with no cargo project in it");
    let mut environment = environment(&fixture);
    environment.working_directory = empty;

    let document = document(&environment);
    assert_eq!(standing(&document, "workspace"), "fail");
    assert!(
        check(&document, "workspace")["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("Cargo.toml")),
        "and names the file that is not there rather than the directory, because the \
         directory is the argument the person just passed: {document}"
    );
}

#[test]
fn a_configuration_nobody_can_read_is_a_failure_carrying_the_code_it_refused_with() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(
        rust_mutants_cli::config::FILE_NAME,
        b"version = 1\n[mutation]\ntiers = \"not a list\"\n",
    );

    let document = document(&environment(&fixture));
    assert_eq!(
        standing(&document, "config"),
        "fail",
        "a configuration a run refuses is a run that will not start, and the doctor is \
         asked before the run is spent finding out: {document}"
    );
    let config = check(&document, "config");
    assert!(
        config["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("RM")),
        "and carries the code the refusal has, which is what a person greps for: {config}"
    );
}

#[test]
fn a_tree_with_no_configuration_is_told_what_writes_one() {
    let fixture = Fixture::copy("fixture-simple");
    let path = fixture.root().join(rust_mutants_cli::config::FILE_NAME);
    drop(std::fs::remove_file(&path));

    let document = document(&environment(&fixture));
    assert_eq!(standing(&document, "config"), "ok");
    let detail = check(&document, "config")["detail"]
        .as_str()
        .expect("the check says what it found")
        .to_owned();
    assert!(
        detail.contains("defaults") && detail.contains("init"),
        "no configuration is not a problem, and the line still says what would write \
         one, because that is the next thing a person does: {detail}"
    );
}

#[test]
fn a_configuration_a_run_reads_is_named_by_its_path() {
    let fixture = Fixture::copy("fixture-simple");
    fixture.write(rust_mutants_cli::config::FILE_NAME, b"version = 1\n");

    let document = document(&environment(&fixture));
    assert_eq!(standing(&document, "config"), "ok");
    assert!(
        check(&document, "config")["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains(rust_mutants_cli::config::FILE_NAME)),
        "and the path is the answer, because a run that read a configuration from \
         somewhere else answers about that tree: {document}"
    );
}

#[test]
fn what_earlier_runs_left_in_the_temporary_directory_is_counted() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let clean = document(&environment);
    assert_eq!(
        standing(&clean, "snapshots"),
        "ok",
        "nothing left over is nothing to say: {clean}"
    );

    for prefix in [
        rust_mutants::snapshot::DIR_PREFIX,
        rust_mutants::workspace::TARGET_DIR_PREFIX,
    ] {
        std::fs::create_dir_all(fixture.temp().join(format!("{prefix}abandoned")))
            .expect("a directory an interrupted run left behind");
    }

    let document = document(&environment);
    assert_eq!(
        standing(&document, "snapshots"),
        "warn",
        "a snapshot and a target directory are the two things a run makes and an \
         interrupted one leaves, and they are the largest things on the disk: {document}"
    );
    let snapshots = check(&document, "snapshots");
    assert!(
        snapshots["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains('2')),
        "the check counts them rather than saying some are there, because the number is \
         how a person tells one interrupted run from a month of them: {snapshots}"
    );
    assert!(
        snapshots["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("--gc")),
        "and names what removes them: {snapshots}"
    );
}

#[test]
fn a_cache_that_cannot_be_written_is_a_warning_and_never_a_run_that_will_not_start() {
    let fixture = Fixture::copy("fixture-simple");
    let mut environment = environment(&fixture);
    let file = fixture.temp().join("a-file-where-the-cache-goes");
    std::fs::write(&file, b"not a directory").expect("a file where the cache directory goes");
    environment.cache_directory = file;

    let document = document(&environment);
    assert_eq!(
        standing(&document, "cache"),
        "warn",
        "what earlier runs established is a saving and never a requirement, so a cache \
         nobody can write is a slower run rather than no run: {document}"
    );
    assert!(
        check(&document, "cache")["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("--no-cache")),
        "and the line says how to run without it: {document}"
    );
}

#[test]
fn a_temporary_directory_that_is_not_there_is_a_failure_naming_what_a_run_writes_in_it() {
    let fixture = Fixture::copy("fixture-simple");
    let mut environment = environment(&fixture);
    environment.temp_directory = fixture.temp().join("was-never-made");

    let document = document(&environment);
    assert_eq!(
        standing(&document, "temp"),
        "fail",
        "every snapshot and every target directory a run makes goes here, so a run \
         without one does not start: {document}"
    );
    let temp = check(&document, "temp");
    assert!(
        temp["detail"].as_str().is_some_and(|detail| {
            detail.contains(rust_mutants::snapshot::DIR_PREFIX)
                && detail.contains(rust_mutants::workspace::TARGET_DIR_PREFIX)
        }),
        "and names what it would have written there, because those are the names a \
         person looks for when the disk fills: {temp}"
    );
    assert!(
        temp["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("TMPDIR")),
        "and the variable that moves it: {temp}"
    );
}

#[test]
fn a_package_with_nothing_that_tests_is_a_check_that_says_which_one() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let tested = document(&environment);
    assert_eq!(
        standing(&tested, "targets"),
        "ok",
        "a package with a test target is what a run measures: {tested}"
    );

    let manifest = fixture.root().join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest");
    std::fs::write(
        &manifest,
        format!("{text}\n[lib]\npath = \"src/lib.rs\"\ntest = false\n"),
    )
    .expect("a package that tests nothing");
    std::fs::remove_dir_all(fixture.root().join("tests")).expect("and no test beside it");

    let document = document(&environment);
    assert_eq!(
        standing(&document, "targets"),
        "fail",
        "a package whose targets do not test answers nothing about its mutations, and a \
         run over it scores every one of them survived: {document}"
    );
    assert!(
        check(&document, "targets")["remedy"]
            .as_str()
            .is_some_and(|remedy| remedy.contains("--package")),
        "and says how to narrow to one that does: {document}"
    );
}

#[test]
fn the_host_a_run_compiles_for_decides_whether_a_touch_names_a_test() {
    let fixture = Fixture::copy("fixture-simple");
    let document = document(&environment(&fixture));
    let guards = check(&document, "guards");
    assert_eq!(
        guards["status"], "ok",
        "this host gives each test a thread of its own: {guards}"
    );
    assert!(
        guards["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("thread")),
        "and the line says why that is what routing rests on, because on a host without \
         threads every mutation goes to every test and a person reading the work ledger \
         afterwards has no way to know why: {guards}"
    );
}

#[test]
fn what_a_doctor_is_asked_about_is_the_root_it_was_given_and_not_the_directory_it_was_started_in() {
    let fixture = Fixture::copy("fixture-workspace");
    let elsewhere = Fixture::copy("fixture-simple");
    let mut environment = environment(&elsewhere);
    environment.working_directory = fixture.root().to_path_buf();

    let named = asked(
        &environment,
        &[
            "doctor",
            "--json",
            "--root",
            &fixture.root().join("crates").join("core").to_string_lossy(),
        ],
    );
    let document: serde_json::Value =
        serde_json::from_str(&named.out).expect("the doctor answers as a document");
    assert_eq!(
        standing(&document, "workspace"),
        "fail",
        "--root is the tree the answer is about; a doctor that answered about the \
         working directory would pass here and the run would still refuse: {}{}",
        named.out,
        named.err
    );
}

#[test]
fn a_reserved_variable_whose_value_is_empty_is_not_one_that_is_set() {
    let fixture = Fixture::copy("fixture-simple");
    let mut environment = environment(&fixture);
    environment
        .vars
        .push((OsString::from("RUST_MUTANTS_ACTIVE"), OsString::new()));

    let root = environment.working_directory.to_string_lossy().into_owned();
    let said = asked(&environment, &["doctor", "--root", &root]);
    assert!(
        said.code <= rust_mutants_cli::EXIT_USAGE,
        "an empty value activates nothing, and a shell that exports a variable it never \
         assigned is a shell everybody has: {}{}",
        said.out,
        said.err
    );
    assert!(
        !said.out.contains("unset it"),
        "so the run is not told to unset it, and the doctor answers about the same rule \
         the run refuses on: a check that failed where the run says nothing sends a \
         person to unset a variable that was never in the way: {}",
        said.out
    );

    let ran = asked(
        &environment,
        &["list", "--offline", "--locked", "--root", &root],
    );
    assert!(
        !ran.err.contains("RM0006"),
        "which is the rule this one is about: {}{}",
        ran.out,
        ran.err
    );
}

#[test]
fn a_root_spelled_as_a_relative_path_is_relative_to_where_the_command_was_told_it_is() {
    let fixture = Fixture::copy("fixture-workspace");
    let mut environment = environment(&fixture);
    environment.working_directory = fixture.root().to_path_buf();

    let said = asked(&environment, &["doctor", "--json", "--root", "crates/core"]);
    let document: serde_json::Value = serde_json::from_str(&said.out)
        .unwrap_or_else(|error| panic!("the doctor answers as a document: {error}\n{}", said.err));
    assert_eq!(
        standing(&document, "workspace"),
        "fail",
        "a relative root is relative to the directory the command was told it is in, not \
         to the one the process happens to be in: a caller that says where it is and \
         then gets an answer about somewhere else has been told about a tree it did not \
         name: {}{}",
        said.out,
        said.err
    );
    let detail = check(&document, "workspace")["detail"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        detail.contains("member of") && detail.contains("crates/core"),
        "and the answer is about the member it was asked about rather than about a \
         directory of that name below wherever the process happens to be, which is a \
         different tree and usually is not there at all: {detail}"
    );
}
