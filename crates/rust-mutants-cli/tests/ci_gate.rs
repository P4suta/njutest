// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `ci gate` leaves where a continuous integration job shows it: the step summary, the step outputs, and one annotation per survivor.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its own fixture has nothing to say about the command"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::id::{Identity, digest};
use rust_mutants::outcome::Outcome;
use rust_mutants::report::catalog::{PlatformDocument, SelectionDocument, WorkspaceDocument};
use rust_mutants::run::FindingKind;
use rust_mutants::runner::Cancel;
use rust_mutants::span::Span;
use rust_mutants_cli::report::run::{
    Accounting, FindingDocument, RunDocument, RunMeta, RunMutantDocument, ScoreDocument,
};
use rust_mutants_cli::{CiHost, Environment, Streams};

include!("support/missing.rs");

/// One mutation of `src/lib.rs`, on its own line, so every row is a place a reviewer can be pointed at.
fn mutant(index: u32, outcome: Outcome) -> RunMutantDocument {
    let source_digest = format!("{index:064x}");
    let original = ">".to_owned();
    let replacement = ">=".to_owned();
    let id = Identity {
        path: "src/lib.rs".to_owned(),
        rule_name: "gt-to-ge".to_owned(),
        rule_version: 1,
        span: Span {
            start: 100,
            end: 101,
        },
        source_digest: source_digest.clone(),
        original_digest: digest(original.as_bytes()),
        replacement_digest: digest(replacement.as_bytes()),
    }
    .id()
    .expect("the fixture identity is complete");
    RunMutantDocument {
        index,
        display_id: id.display().into_inner(),
        id: id.into_inner(),
        path: "src/lib.rs".to_owned(),
        package: "demo".to_owned(),
        family: "comparison".to_owned(),
        rule: "gt-to-ge".to_owned(),
        item: "demo".to_owned(),
        rule_version: 1,
        line: index.saturating_add(10),
        column: 8,
        start_byte: 100,
        end_byte: 101,
        source_digest,
        original,
        replacement,
        outcome,
        target: "demo/lib/demo".to_owned(),
        exit_code: 0,
        duration_ms: 41,
        tests_run: Some(1),
        killed_by: Vec::new(),
        signal: None,
        not_run_reason: None,
        declined: Vec::new(),
        route: None,
        identical: rust_mutants::run::CodegenIdentity::NotMeasured,
        retried: false,
        lingered: false,
        expected: false,
        unreached: false,
        source_run_id: None,
        step_notice: None,
    }
}

/// A coherent run: one killed mutant, then `survivors` mutants no test noticed.
fn document(survivors: u32) -> RunDocument {
    let mut mutants = vec![mutant(0, Outcome::Killed)];
    let mut findings = Vec::new();
    for index in 1..=survivors {
        let survivor = mutant(index, Outcome::Survived);
        findings.push(FindingDocument {
            kind: FindingKind::SurvivingMutant,
            mutant: Some(survivor.id.clone()),
            detail: format!(
                "no test noticed {}; 1 tests ran and passed",
                survivor.display_id
            ),
        });
        mutants.push(survivor);
    }
    let decided = survivors.saturating_add(1);
    RunDocument {
        document_type: "rust-mutants/run-report".to_owned(),
        schema_version: rust_mutants_cli::report::run::SCHEMA_VERSION,
        tool_version: "0.1.0".to_owned(),
        run: RunMeta {
            id: "20260905T120000000Z".to_owned(),
            started_at: "2026-09-05T12:00:00Z".to_owned(),
            finished_at: "2026-09-05T12:00:01Z".to_owned(),
            duration_ms: 1000,
            interrupted: false,
            exit_code: if survivors == 0 {
                rust_mutants::run::EXIT_DETECTED
            } else {
                rust_mutants::run::EXIT_FOUND
            },
            shard: None,
            jobs: rust_mutants_cli::report::run::JobsDocument {
                asked: "auto".to_owned(),
                used: 1,
            },
        },
        workspace: WorkspaceDocument {
            root_name: "demo".to_owned(),
            toolchain: "rustc 1.98.0".to_owned(),
            workspace_digest: "a".repeat(64),
            catalog_digest: "b".repeat(64),
            platform: PlatformDocument {
                os: "linux".to_owned(),
                arch: "x86_64".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
            },
        },
        selection: SelectionDocument {
            build: Vec::new(),
            tier: "balanced".to_owned(),
            operators: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            mutant_steps: None,
        },
        targets: Vec::new(),
        established_tests: 0,
        accounting: accounting(survivors),
        score: Some(ScoreDocument {
            detected: 1,
            decided,
            value: 1.0 / f64::from(decided),
        }),
        mutants,
        rejections: Vec::new(),
        skips: Vec::new(),
        expectations: Vec::new(),
        findings,
        facts: Vec::new(),
    }
}

/// What a run of one killed mutant and `survivors` survivors counted.
fn accounting(survivors: u32) -> Accounting {
    let decided = survivors.saturating_add(1);
    Accounting {
        cataloged: decided,
        refused: 0_u32.into(),
        skipped: 0_u32.into(),
        executed: decided.into(),
        killed: 1_u32.into(),
        survived: survivors.into(),
        step_limit_reached: 0_u32.into(),
        waited: 0_u32.into(),
        inconclusive: 0_u32.into(),
        errored: 0_u32.into(),
        unreached: 0_u32.into(),
        discharged: 0_u32.into(),
        declined: 0_u32.into(),
        not_run: 0_u32.into(),
        expected: 0_u32.into(),
    }
}

/// A checkout holding the workspace, and the files a GitHub runner names for one step.
struct Job {
    scratch: tempfile::TempDir,
}

impl Job {
    fn new() -> Self {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        std::fs::create_dir_all(scratch.path().join("checkout/crates/demo")).expect("a checkout");
        Self { scratch }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.scratch.path().join(relative)
    }

    fn checkout(&self) -> PathBuf {
        self.path("checkout")
    }

    fn github(&self) -> CiHost {
        CiHost::GitHub {
            summary: self.path("summary.md"),
            output: self.path("output.txt"),
            workspace: self.checkout(),
        }
    }

    fn report(&self, document: &RunDocument) -> PathBuf {
        let path = self.path("run-report.json");
        std::fs::write(
            &path,
            serde_json::to_string(document).expect("a document serialises"),
        )
        .expect("the report is written");
        path
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative)).expect("the gate wrote the file")
    }
}

/// What one command said, driven in this process.
struct Said {
    code: u8,
    out: String,
    err: String,
}

impl Said {
    fn annotations(&self) -> Vec<&str> {
        self.out
            .lines()
            .filter(|line| line.starts_with("::error "))
            .collect()
    }
}

fn gate(host: CiHost, root: &Path, args: &[&OsString]) -> Said {
    let environment = Environment {
        vars: njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect(),
        temp_directory: std::env::temp_dir(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: root.join("cache"),
        working_directory: root.to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        cargo: None,
        ci: host,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        [OsString::from("rust-mutants"), "ci".into(), "gate".into()]
            .into_iter()
            .chain(args.iter().map(|argument| (*argument).clone())),
        &environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: String::from_utf8(out).expect("stdout is UTF-8"),
        err: String::from_utf8(err).expect("stderr is UTF-8"),
    }
}

fn os(text: &str) -> OsString {
    OsString::from(text)
}

#[test]
fn a_gate_on_github_writes_the_summary_the_outputs_and_an_annotation_per_survivor() {
    let job = Job::new();
    std::fs::write(job.path("summary.md"), "an earlier step's summary\n")
        .expect("an earlier step wrote the summary");
    std::fs::write(job.path("output.txt"), "earlier=1\n").expect("an earlier output");
    let document = document(2);
    let report = job.report(&document);
    let sarif = job.path("results.sarif");
    let said = gate(
        job.github(),
        &job.checkout(),
        &[
            &os("--report"),
            &report.clone().into_os_string(),
            &os("--sarif"),
            &sarif.clone().into_os_string(),
            &os("--host"),
            &os("github"),
        ],
    );
    assert_eq!(
        said.code, 1,
        "the gate exits with the verdict the run earned, so the job fails where the run \
         found something: {}",
        said.err
    );
    let summary = job.read("summary.md");
    assert!(
        summary.starts_with("an earlier step's summary\n")
            && summary.contains(&rust_mutants_cli::report::markdown::document(&document)),
        "the run's Markdown is appended to the summary the runner named, after what an \
         earlier step put there: {summary}"
    );
    let outputs = job.read("output.txt");
    assert_eq!(
        outputs,
        format!(
            "earlier=1\nverdict=found\nreport={}\nsarif={}\n",
            report.display(),
            sarif.display()
        ),
        "a later step reads the verdict and where the documents are from the outputs"
    );
    let expected: Vec<String> = document
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome == Outcome::Survived)
        .map(|mutant| {
            format!(
                "::error file=src/lib.rs,line={},col=8,title=surviving mutant gt-to-ge::no test \
                 noticed `>` \u{2192} `>=`; `rust-mutants explain {}` says what it is",
                mutant.line, mutant.display_id
            )
        })
        .collect();
    assert_eq!(
        said.annotations(),
        expected,
        "every survivor is put on the line it lives on, and nothing else is"
    );
    assert_eq!(
        expected.first().map(|line| line.contains("line=11,")),
        Some(true),
        "the first survivor is the row after the killed one: {expected:?}"
    );
    let log: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&sarif).expect("the SARIF log is written"),
    )
    .expect("the SARIF log is JSON");
    assert_eq!(
        log.pointer("/runs/0/results")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2),
        "the SARIF log holds every finding: {log}"
    );
}

#[test]
fn a_root_below_the_checkout_is_named_from_the_checkout() {
    let job = Job::new();
    let report = job.report(&document(1));
    let said = gate(
        job.github(),
        &job.checkout().join("crates/demo"),
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("github"),
        ],
    );
    assert_eq!(said.code, 1, "{}", said.err);
    assert!(
        said.annotations()
            .iter()
            .all(|line| line.starts_with("::error file=crates/demo/src/lib.rs,")),
        "the runner places an annotation relative to the checkout, not to the workspace \
         inside it: {:?}",
        said.annotations()
    );
}

#[test]
fn a_root_outside_the_checkout_is_refused_rather_than_misplaced() {
    let job = Job::new();
    let report = job.report(&document(1));
    let elsewhere = job.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("another directory");
    let said = gate(
        job.github(),
        &elsewhere,
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("github"),
        ],
    );
    assert_eq!(said.code, 2, "{}", said.out);
    assert!(
        said.err.contains("RM0015") && said.annotations().is_empty(),
        "an annotation the runner cannot place is refused by its code, and no annotation \
         is written: {}",
        said.err
    );
}

#[test]
fn github_asked_for_where_the_runner_named_no_files_is_refused() {
    let job = Job::new();
    let report = job.report(&document(1));
    let said = gate(
        CiHost::None,
        &job.checkout(),
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("github"),
        ],
    );
    assert_eq!(said.code, 2, "{}", said.out);
    assert!(
        said.err.contains("RM0014"),
        "a host that is asked for and not there is a refusal, not a plain run: {}",
        said.err
    );
}

#[test]
fn more_survivors_than_a_step_can_show_are_counted_rather_than_dropped() {
    for (sarif, said_where) in [
        (true, "all 12 are in SARIF and the report"),
        (false, "all 12 are in the report"),
    ] {
        let job = Job::new();
        let report = job.report(&document(12));
        let written = job.path("results.sarif").into_os_string();
        let mut args = vec![
            os("--report"),
            report.into_os_string(),
            os("--host"),
            os("github"),
        ];
        if sarif {
            args.extend([os("--sarif"), written]);
        }
        let said = gate(
            job.github(),
            &job.checkout(),
            &args.iter().collect::<Vec<_>>(),
        );
        assert_eq!(said.code, 1, "{}", said.err);
        assert_eq!(
            said.annotations().len(),
            10,
            "GitHub Actions shows ten error annotations per step, so no more are written"
        );
        let summary = job.read("summary.md");
        assert!(
            summary.contains(&format!("Shown 10 of 12 survivors; {said_where}.")),
            "the summary says how many a reader did not see and where they are: {summary}"
        );
    }
}

#[test]
fn a_plain_gate_writes_the_lines_and_exits_with_the_verdict() {
    let job = Job::new();
    let document = document(0);
    let report = job.report(&document);
    let said = gate(
        job.github(),
        &job.checkout(),
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("plain"),
        ],
    );
    assert_eq!(said.code, 0, "{}", said.err);
    assert_eq!(
        said.out,
        rust_mutants_cli::report::lines(&document).expect("the lines render"),
        "a plain gate says what `report` says"
    );
    assert!(
        test_missing(&job.path("summary.md")) && test_missing(&job.path("output.txt")),
        "and writes nothing where a runner would look, because it was told there is none"
    );
}

#[test]
fn a_runner_is_recognised_only_by_every_file_it_names() {
    let vars = |pairs: &[(&str, &str)]| -> rust_mutants::vars::Variables {
        pairs
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    };
    assert_eq!(
        Environment::ci_host_of(&vars(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_STEP_SUMMARY", "/s"),
            ("GITHUB_OUTPUT", "/o"),
            ("GITHUB_WORKSPACE", "/w"),
        ])),
        CiHost::GitHub {
            summary: PathBuf::from("/s"),
            output: PathBuf::from("/o"),
            workspace: PathBuf::from("/w"),
        }
    );
    assert_eq!(
        Environment::ci_host_of(&vars(&[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_OUTPUT", "/o"),
            ("GITHUB_WORKSPACE", "/w"),
        ])),
        CiHost::None,
        "a runner that names no summary is not one a step can report through"
    );
    assert_eq!(
        Environment::ci_host_of(&vars(&[("GITLAB_CI", "true")])),
        CiHost::GitLab
    );
    assert_eq!(
        Environment::ci_host_of(&rust_mutants::vars::Variables::empty()),
        CiHost::None
    );
}

#[test]
fn an_annotation_cannot_be_ended_early_by_what_it_quotes() {
    let mut odd = mutant(1, Outcome::Survived);
    odd.path = "src/a,b:c%.rs".to_owned();
    odd.original = "a\r\nb".to_owned();
    let said = rust_mutants_cli::report::annotations::surviving("crates/x/", &odd);
    assert!(
        said.starts_with("::error file=crates/x/src/a%2Cb%3Ac%25.rs,line=11,col=8,")
            && said.contains("`a%0D%0Ab`")
            && !said.contains(['\r', '\n']),
        "a comma or a colon in a property, and a line break anywhere, would end the \
         command where the runner reads it: {said}"
    );
}

#[test]
fn every_verdict_is_one_code_and_one_word() {
    use rust_mutants_cli::app::ci::Verdict;
    for verdict in Verdict::ALL {
        assert_eq!(
            Verdict::of(verdict.code()),
            Some(verdict),
            "{} reads back from its own code",
            verdict.word()
        );
    }
    assert_eq!(
        Verdict::ALL.map(Verdict::word),
        ["detected", "found", "failed", "interrupted"]
    );
    assert_eq!(
        Verdict::of(143),
        None,
        "a code no verdict carries has no word, so a later step is never told one"
    );
}

/// A file of `count` lines, each saying which it is.
fn numbered(count: u32, changed: &[u32]) -> String {
    (1..=count)
        .map(|line| {
            if changed.contains(&line) {
                format!("let changed_{line} = {line};\n")
            } else {
                format!("let line_{line} = {line};\n")
            }
        })
        .collect()
}

#[test]
fn with_a_change_only_the_survivors_on_its_lines_are_annotated() {
    let repo = njutest_devkit::repo::Repo::new();
    repo.write("src/lib.rs", &numbered(20, &[]));
    repo.commit();
    repo.write("src/lib.rs", &numbered(20, &[12]));
    let job = Job::new();
    let report = job.report(&document(3));
    let said = gate(
        CiHost::GitHub {
            summary: job.path("summary.md"),
            output: job.path("output.txt"),
            workspace: repo.root().to_path_buf(),
        },
        repo.root(),
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("github"),
            &os("--changed-from"),
            &os("HEAD"),
        ],
    );
    assert_eq!(said.code, 1, "{}", said.err);
    let annotations = said.annotations();
    assert!(
        annotations.len() == 1
            && annotations
                .iter()
                .all(|line| line.contains("file=src/lib.rs,line=12,")),
        "of the survivors on lines 11, 12 and 13, only the one on the line the change \
         touched is put in front of its author: {annotations:?}"
    );
    let summary = job.read("summary.md");
    assert!(
        summary.contains(
            "2 more survivors are on lines this change did not touch; they are in the report."
        ),
        "and the summary says the others exist rather than letting the annotations read as \
         all there is: {summary}"
    );
}

#[test]
fn a_change_git_cannot_be_asked_for_is_refused_rather_than_read_as_none() {
    let job = Job::new();
    let report = job.report(&document(1));
    let said = gate(
        job.github(),
        &job.checkout(),
        &[
            &os("--report"),
            &report.into_os_string(),
            &os("--host"),
            &os("github"),
            &os("--changed-from"),
            &os("HEAD"),
        ],
    );
    assert_eq!(said.code, 2, "{}", said.out);
    assert!(
        said.err.contains("RM0010") && said.annotations().is_empty(),
        "a checkout that is not a repository says nothing about which lines changed, and \
         that is not the same as no line changing: {}",
        said.err
    );
}

#[test]
fn the_survivors_off_the_change_are_counted_in_a_sentence_that_agrees_with_its_number() {
    use rust_mutants_cli::app::ci::untouched;
    assert_eq!(
        untouched(1),
        "1 more survivor is on a line this change did not touch; it is in the report."
    );
    assert_eq!(
        untouched(3),
        "3 more survivors are on lines this change did not touch; they are in the report."
    );
}
