// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A fixture with two real dependencies, against what its README says every question about them came to.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::cli::Environment;
use njutest_devkit::fixture::{Fixture, Seam};
use rust_mutants::runner::Cancel;

/// The fixture this suite is about.
const FIXTURE: &str = "fixture-wired";

/// The variable that rewrites the block rather than refusing it, as it does for the mutation fates.
const UPDATE: &str = "UPDATE_FATES";

/// The token the committed configuration holds where the provider's path goes.
///
/// A provider is a program, and where this workspace builds one is not something a committed file can know.
/// Everything else about the two seams —
/// which variable is interposed, which protocol is read, how long an answer is held up — is in the fixture where a reader finds it, because a run whose questions were composed by the test driving it would be a run nobody could reproduce by reading the tree.
const PLACEHOLDER: &str = "UPSTREAM";

#[test]
fn every_question_the_two_seams_licensed_came_to_what_the_readme_says() {
    let mut attempt = 1;
    let (fixture, report, found, stated) = loop {
        let Some((fixture, report, found, stated)) = attempt_once() else {
            return;
        };
        if attempt < ATTEMPTS && only_the_machine_moved(&report, &found, &stated) {
            attempt += 1;
            continue;
        }
        break (fixture, report, found, stated);
    };
    if let Some(directory) = std::env::var_os("KEEP_REPORT") {
        let run =
            njutest::app::reports::pointed_at(fixture.root(), njutest::app::reports::Index::Any)
                .expect("the index is readable")
                .expect("the index names a run");
        let source = fixture
            .root()
            .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs")
            .join(run.as_str());
        let kept = Path::new(&directory).join(format!("wire-{}", std::process::id()));
        std::fs::create_dir_all(&kept).expect("a place to keep the failing report");
        copy_tree(&source, &kept);
    }
    let drawn = found
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join("\n");
    if std::env::var_os(UPDATE).is_some() {
        rewrite(&drawn);
        return;
    }
    let could_not_attribute = finding(&report, njutest::assure::wire::ALREADY_FAILING);
    panic!(
        "what a run of {FIXTURE} established is not what its README states. A seam fate that \
         moved is either a defect or a decision, and the README is where the decision is \
         recorded, so {UPDATE}=1 rewrites the block and the diff is the review.\n\n\
         stated:\n{}\n\nfound:\n{drawn}\n\n\
         Before reading this as a fate that moved: where the rows say `unreached` and the \
         run reports it could not attribute, the suite on this machine was already failing \
         without a fault, so what moved is the machine. Rewriting the block would then \
         write that machine into the oracle.\ncould not attribute: {}\n\n\
         what the run said it could not do:\n{}\n\n\
         where each exchange the orders test made stopped, from the run's recorded output:\n{}\n",
        stated
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join("\n"),
        could_not_attribute.unwrap_or_else(|| "(nothing said so)".to_owned()),
        every_absence(&report).join("\n"),
        stages(fixture.root()).join("\n")
    );
}

/// How many whole runs the comparison is given when every row that moved is one the run itself says this machine kept it from answering.
const ATTEMPTS: usize = 3;

/// One run of the fixture, and what it found where that is not what the README states.
fn attempt_once() -> Option<(Fixture, serde_json::Value, Vec<Seam>, Vec<Seam>)> {
    let fixture = Fixture::copy(FIXTURE);
    pointed_at_the_provider(&fixture);

    let output = verify(&fixture);
    let report = document(&fixture);
    let lost = not_watched(&report);
    assert!(
        lost.is_empty(),
        "a seam this fixture names is one the run did not watch, so comparing the block \
             below would compare a shorter answer against a whole one, and the rows that \
             remain carry the other seam's decisions. The report already says which of the \
             five ways it could not, and this is that sentence:\n{}\n\n{}",
        lost.join("\n"),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let not_green = limitation(&report, njutest::assure::wire::SUITE_NOT_GREEN);
    assert!(
        not_green.is_none(),
        "the fixture's own suite did not pass without a fault, so no question it licensed \
             could be answered by anything and the phase put none of them. That is the run \
             being honest; what it says about this machine is that the provider or the tests \
             it serves did not come up: {}\n\n{}",
        not_green.unwrap_or_default(),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let found = rows(&report);
    assert!(
        !found.is_empty(),
        "a run that recorded no seam at all established nothing this fixture is for: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );

    let stated = njutest_devkit::fixture::stated_seams(FIXTURE);
    if found == stated {
        return None;
    }
    Some((fixture, report, found, stated))
}

/// Whether every row that differs is `unreached` and the report itself says a question was not put, not reproduced, or not carried.
fn only_the_machine_moved(report: &serde_json::Value, found: &[Seam], stated: &[Seam]) -> bool {
    let said = finding(report, njutest::assure::wire::NOT_PUT).is_some()
        || finding(report, njutest::assure::wire::NOT_REPRODUCED).is_some()
        || limitation(report, njutest::assure::wire::TRANSPORT_FAILED).is_some();
    let moved: Vec<String> = found
        .iter()
        .filter(|row| !stated.contains(row))
        .map(ToString::to_string)
        .collect();
    said && !moved.is_empty() && moved.iter().all(|row| row.ends_with(" unreached"))
}

/// Every place the fixture's orders test said an exchange stopped, from what the run recorded of each target it ran.
fn stages(root: &Path) -> Vec<String> {
    let mut said = Vec::new();
    let mut pending = vec![root.join(".njutest").join("trace")];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                said.push(format!("(could not read {}: {error})", directory.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    said.push(format!("(could not list {}: {error})", directory.display()));
                    continue;
                }
            };
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(_file) => match std::fs::read_to_string(&path) {
                    Ok(text) => said.extend(
                        text.lines()
                            .filter(|line| {
                                line.contains("left: Err(") || line.contains("left: Ok(")
                            })
                            .map(|line| format!("{}\t{}", line.trim(), path.display())),
                    ),
                    Err(error) => {
                        said.push(format!("(could not read {}: {error})", path.display()));
                    }
                },
                Err(error) => said.push(format!("(could not inspect {}: {error})", path.display())),
            }
        }
    }
    let mut by_stage: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for line in said {
        let (stage, file) = line.split_once('\t').unwrap_or((line.as_str(), ""));
        by_stage
            .entry(stage.to_owned())
            .or_default()
            .push(file.to_owned());
    }
    if by_stage.is_empty() {
        return vec!["(the recorded output named no stage)".to_owned()];
    }
    by_stage
        .into_iter()
        .map(|(stage, files)| {
            let shown: Vec<&str> = files.iter().take(3).map(String::as_str).collect();
            format!("{stage} x{}: {}", files.len(), shown.join(", "))
        })
        .collect()
}

/// The copy's configuration, with the one thing a committed file cannot hold filled in.
fn pointed_at_the_provider(fixture: &Fixture) {
    let path = fixture.root().join(".njutest.toml");
    let written = std::fs::read_to_string(&path).expect("the fixture's configuration");
    assert!(
        written.contains(PLACEHOLDER),
        "the fixture no longer says where its provider goes, so this driver would run it \
         against whatever {PLACEHOLDER} was replaced with"
    );
    let provider = njutest_devkit::fake_cargo::example("fake_upstream");
    std::fs::write(
        &path,
        written.replace(
            PLACEHOLDER,
            provider.to_str().expect("test protocol paths are UTF-8"),
        ),
    )
    .expect("the configuration");
}

/// One run of the fixture, with everything it needs and nothing it does not.
fn verify(fixture: &Fixture) -> std::process::Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "verify", "--offline", "--locked", "--trace"]
            .into_iter()
            .map(OsString::from),
        &environment(fixture),
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let run = njutest::app::reports::pointed_at(fixture.root(), njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run");
    let path = fixture
        .root()
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(run.as_str())
        .join(njutest::app::reports::DOCUMENT_NAME);
    let whole: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&path).expect("the report"),
    )
    .expect("the report is a document");
    whole["report"]["builds"][0]["parts"][0].clone()
}

/// The detail of the one finding whose subject is `named`, when the report carries it.
fn finding(report: &serde_json::Value, named: &str) -> Option<String> {
    report["findings"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|one| one["subject"].as_str() == Some(named))
        .find_map(|one| one["detail"].as_str().map(str::to_owned))
}

/// Every limitation and every not-measured finding the report carries, so a failure names what the run could not do rather than only what it did.
fn every_absence(report: &serde_json::Value) -> Vec<String> {
    let named = |key: &str, label: &str| -> Vec<String> {
        report[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|one| {
                let name = one["name"].as_str().or_else(|| one["subject"].as_str())?;
                let detail = one["detail"].as_str()?;
                Some(format!("{label}\t{name}\t{detail}"))
            })
            .collect()
    };
    let mut all = named("limitations", "limitation");
    all.extend(named("findings", "finding"));
    all
}

/// The detail of the one limitation `named`, when the report carries it.
fn limitation(report: &serde_json::Value, named: &str) -> Option<String> {
    report["limitations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|one| one["name"].as_str() == Some(named))
        .find_map(|one| one["detail"].as_str().map(str::to_owned))
}

/// Every seam the configuration named that this run could not put an interposer in front of, and why.
///
/// `assure::run::state_unwatched` already writes one of five sentences per lost seam.
/// Nothing read it here, so a run that lost a seam arrived as a block six rows short, and the shape of that -- one seam's capability against the other seam's decisions -- reads as a seam fate that moved rather than as a seam that was never watched.
/// The first invites `UPDATE_FATES=1`, which would write the loss into the oracle.
fn not_watched(report: &serde_json::Value) -> Vec<String> {
    report["limitations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|one| one["name"].as_str() == Some(njutest::limitation::SEAM_NOT_WATCHED))
        .filter_map(|one| one["detail"].as_str().map(str::to_owned))
        .collect()
}

/// Every seam the report names, in the order it names them.
fn rows(report: &serde_json::Value) -> Vec<Seam> {
    report["seams"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|row| {
            let text = |key: &str| row[key].as_str().map(str::to_owned);
            Seam {
                capability: text("capability").unwrap_or_default(),
                seq: row["seq"].as_u64().unwrap_or_default(),
                rule: text("rule").unwrap_or_default(),
                decision: row["answer"]["decision"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_default(),
                by: row["answer"]["noticed_by"]
                    .as_str()
                    .or_else(|| row["answer"]["proof"].as_str())
                    .map(str::to_owned),
            }
        })
        .collect()
}

/// Puts `drawn` in the README's seams block, leaving the rest of the page alone.
fn copy_tree(source: &Path, kept: &Path) {
    fn walk(source: &Path, kept: &Path) {
        for entry in std::fs::read_dir(source).expect("a report directory to keep") {
            let entry = entry.expect("a report entry to keep");
            let kind = entry.file_type().expect("a report entry type");
            let target = kept.join(entry.file_name());
            if kind.is_dir() {
                std::fs::create_dir_all(&target).expect("a place to keep report files");
                walk(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("the report file is copied");
            }
        }
    }
    walk(source, kept);
}

fn rewrite(drawn: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(FIXTURE)
        .join("README.md");
    let readme = std::fs::read_to_string(&path).expect("the README");
    let fence = njutest_devkit::fixture::SEAMS_FENCE;
    let (before, rest) = readme
        .split_once(fence)
        .unwrap_or_else(|| panic!("{} has no {fence} block", path.display()));
    let after = rest
        .split_once("\n```")
        .map_or("", |(_, tail)| tail)
        .to_owned();
    std::fs::write(&path, format!("{before}{fence}\n{drawn}\n```{after}")).expect("the README");
}

/// The run the report belongs to, which `why` is asked about by name.
fn newest_run(fixture: &Fixture) -> String {
    njutest::app::reports::pointed_at(fixture.root(), njutest::app::reports::Index::Any)
        .expect("the index is readable")
        .expect("the index names a run")
        .as_str()
        .to_owned()
}

#[test]
fn a_seam_question_a_real_run_recorded_is_one_why_renders_from_its_own_recording() {
    let fixture = Fixture::copy(FIXTURE);
    pointed_at_the_provider(&fixture);
    let output = verify(&fixture);
    let report = document(&fixture);
    let row = report["seams"]
        .as_array()
        .and_then(|rows| rows.first())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "the run recorded no seam question, so there is nothing to ask why about: {}",
                njutest_devkit::process::strict_utf8(&output.stderr)
            )
        });
    let asked = row["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a seam row names its question: {row}"))
        .to_owned();
    let capability = row["capability"]
        .as_str()
        .unwrap_or_else(|| panic!("a seam row names its capability: {row}"))
        .to_owned();
    let seq = row["seq"]
        .as_u64()
        .unwrap_or_else(|| panic!("a seam row names which exchange: {row}"));

    let run = newest_run(&fixture);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        ["njutest", "why", "--run", &run, "seam", &asked]
            .into_iter()
            .map(OsString::from),
        &environment(&fixture),
        &mut out,
        &mut err,
    );
    let rendered = njutest_devkit::process::strict_utf8(&out);
    let complaint = njutest_devkit::process::strict_utf8(&err);
    assert_eq!(
        code, 0,
        "a question the report names is one the recording holds, and `why` reads the \
         recording: a run that wrote one and cannot explain it has two accounts of \
         itself.\n{complaint}"
    );
    assert!(
        rendered.contains(&asked),
        "the answer is about the question that was asked: {rendered}"
    );
    assert!(
        rendered.contains(&format!("observed {capability} #{seq}")),
        "and the recording it read is about the same exchange the report names, which is the \
         pairing nothing checked: a hand-built trace proves the reader can read a shape \
         somebody wrote for it, and says nothing about the shape a run writes. Report says \
         {capability} #{seq}; rendered:\n{rendered}"
    );
}
