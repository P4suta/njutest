// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A file is drawn only as the bytes the run measured, on any surface, and one that cannot be drawn says why.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads a document by the names its writer put there"
)]

use std::path::Path;

use njutest::config::Contract;
use njutest::presentation::{Excerpt, MeasuredLine, Missing, Sources};
use njutest::report::{
    BuildReport, Finding, FindingKind, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Position, Report, RunKind, TargetRecord, TargetStatus,
};

/// The source the run measured.
const MEASURED: &str = "pub fn sign(n: i32) -> bool {\n    n > 0\n}\n";

/// The same file after somebody edited the line the run changed, which still holds the text it replaced.
const EDITED: &str = "pub fn sign(n: i32) -> bool {\n    n > 100 && n < 5\n}\n";

/// A run that measured `MEASURED` at `src/lib.rs` and found `>` on line 2 survived.
fn reported() -> Report {
    reading(digest_of(MEASURED.as_bytes()))
}

/// The same run, recording `read` as the digest of the bytes it read at `src/lib.rs`.
fn reading(read: rust_mutants::id::HexDigest) -> Report {
    completed(vec![(
        njutest::config::DEFAULT_CONFIGURATION,
        measured(Some(read)),
    )])
    .expect("a report of one build that read one file")
}

/// Why the builds `completed` was given did not make a report.
#[derive(Debug, thiserror::Error)]
enum UnmadeError {
    #[error(transparent)]
    Measurements(#[from] njutest::report::across::BuildMeasurementsError),
    #[error(transparent)]
    Configured(#[from] njutest::report::across::ConfiguredError),
    #[error(transparent)]
    Completion(#[from] njutest::report::CompletionError),
    #[error(transparent)]
    Named(#[from] rust_mutants::id::RunIdError),
    #[error("a whole-catalog fixture made a part of a catalog")]
    Part,
}

/// The whole report of the builds given, each named and measured.
fn completed(builds: Vec<(&str, BuildReport)>) -> Result<Report, UnmadeError> {
    let names: Vec<String> = builds.iter().map(|(name, _)| (*name).to_owned()).collect();
    let measurements = njutest::report::across::BuildMeasurements::checked(
        builds
            .into_iter()
            .map(|(name, mut source)| {
                source.scope.configured_builds.clone_from(&names);
                (
                    name.to_owned(),
                    rust_mutants::cargo::BuildConfig::default().selection(),
                    source,
                )
            })
            .collect(),
    )?;
    let run = rust_mutants::id::RunId::try_from("one")?;
    match njutest::report::across::configured(&run, &measurements)? {
        njutest::report::LatticedDocument::Complete(latticed) => {
            Ok(latticed.complete_without_models()?)
        }
        njutest::report::LatticedDocument::Shard(_) => Err(UnmadeError::Part),
    }
}

/// One build's draft of the run, recording `read` as the digest of `src/lib.rs` where it recorded one.
fn measured(read: Option<rust_mutants::id::HexDigest>) -> BuildReport {
    let mut source = BuildReport::new("source-one", RunKind::Full, Contract::StandardV1);
    "workspace".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "b".repeat(64);
    source.repository.configuration_digest = "c".repeat(64);
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-09-24T00:00:00Z".clone_into(&mut source.timing.started);
    "2026-09-24T00:00:00Z".clone_into(&mut source.timing.finished);
    source.timing.duration_ms = 1;
    source.targets.push(TargetRecord {
        id: "one".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    source.count_targets().expect("one exact target row");
    if let Some(read) = read {
        source.sources.insert("src/lib.rs".to_owned(), read);
    }
    source.mutants.push(MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "a".repeat(64),
        display_id: "a".repeat(20),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 2,
            column: 7,
            character_column: 7,
        },
        rule: "gt-to-ge".to_owned(),
        item: "sign".to_owned(),
        original: ">".to_owned(),
        replacement: ">=".to_owned(),
        outcome: njutest::report::Decided::Survived,
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    });
    source.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        survived: 1,
        observers: ObserverAccounting {
            unnoticed: 1,
            ..ObserverAccounting::default()
        },
        ..MutantAccounting::default()
    };
    source.findings.push(Finding::new(
        FindingKind::SurvivingMutant,
        &"a".repeat(20),
        "no test noticed gt-to-ge",
    ));
    source.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "this fixture is not a git repository",
    ));
    source.verdict = source.concluded();
    source
}

/// The SHA-256 of `bytes`, as the run records the file it read.
fn digest_of(bytes: &[u8]) -> rust_mutants::id::HexDigest {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, bytes);
    rust_mutants::id::HexDigest::finish(hasher)
}

/// A workspace holding `bytes` at `src/lib.rs`.
fn holding(bytes: impl AsRef<[u8]>) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::create_dir_all(root.path().join("src")).expect("src");
    std::fs::write(root.path().join("src/lib.rs"), bytes).expect("the file");
    root
}

/// What each surface says when `root` is the tree and `report` the run: the page, the briefing, and the language server's messages.
fn surfaces(root: &Path, report: &Report) -> [(&'static str, String); 3] {
    let sources = Sources::read(root, report).expect("the sources a report names");
    let told = njutest::presentation::Told::of(report, &sources, "the-report")
        .expect("what a person is told about the run");
    let noted = njutest::app::lsp::diagnostics(report, root, njutest::app::lsp::Encoding::Utf8)
        .expect("the checked report has representable diagnostics")
        .iter()
        .flat_map(|file| file.diagnostics.iter())
        .filter_map(|one| one["message"].as_str().map(str::to_owned))
        .collect::<Vec<String>>()
        .join("\n");
    [
        (
            "the page",
            njutest::presentation::human::draw(&told, njutest::presentation::Terminal::plain(160)),
        ),
        ("the briefing", njutest::presentation::agent::brief(&told)),
        ("the language server", noted),
    ]
}

fn excerpt(root: &Path) -> Excerpt {
    Sources::read(root, &reported())
        .expect("the sources a report names")
        .at("src/lib.rs", 2)
}

#[test]
fn a_file_edited_after_the_run_is_not_drawn_as_the_code_the_run_measured() {
    let measured = holding(MEASURED);
    assert_eq!(
        excerpt(measured.path()),
        Excerpt::Read(MeasuredLine::specimen("    n > 0")),
        "the file the run measured is drawn"
    );
    let edited = holding(EDITED);
    assert_eq!(
        excerpt(edited.path()),
        Excerpt::Instead(Missing::Edited),
        "an edited line that still holds the text the run replaced is not the line the run \
         measured, and drawing it would show a reader a mutation of code the run never saw"
    );
}

#[test]
fn the_language_server_places_nothing_in_a_file_edited_after_the_run() {
    let edited = holding(EDITED);
    let shown = njutest::app::lsp::diagnostics(
        &reported(),
        edited.path(),
        njutest::app::lsp::Encoding::Utf8,
    )
    .expect("the checked report has representable diagnostics");
    let placed: Vec<&serde_json::Value> = shown
        .iter()
        .flat_map(|file| file.diagnostics.iter())
        .filter(|one| one["code"] == "surviving-mutant")
        .collect();
    assert!(
        placed.is_empty(),
        "a finding placed by line and column in a file that has changed since the run points \
         at code the run never measured: {placed:?}"
    );
}

#[test]
fn the_language_server_says_which_run_measured_a_file_it_shows_nothing_in() {
    let edited = holding(EDITED);
    let shown = njutest::app::lsp::diagnostics(
        &reported(),
        edited.path(),
        njutest::app::lsp::Encoding::Utf8,
    )
    .expect("the checked report has representable diagnostics");
    let notes: Vec<&serde_json::Value> = shown
        .iter()
        .flat_map(|file| file.diagnostics.iter())
        .filter(|one| one["code"] == "not-yet-asked")
        .collect();
    let [note] = notes.as_slice() else {
        panic!("one note for the one file the run's findings are not shown in: {shown:?}");
    };
    let said = note["message"].as_str().unwrap_or_default();
    assert!(
        said.contains("the file has changed since the run read it")
            && said.contains(
                "nothing run one found in this file is shown until a run measures it again"
            ),
        "the note says why, which run, and what shows the findings again: {said}"
    );
    let measured = holding(MEASURED);
    let placed = njutest::app::lsp::diagnostics(
        &reported(),
        measured.path(),
        njutest::app::lsp::Encoding::Utf8,
    )
    .expect("the checked report has representable diagnostics");
    assert!(
        placed
            .iter()
            .flat_map(|file| file.diagnostics.iter())
            .any(|one| one["code"] == "surviving-mutant"),
        "a file that holds the bytes the run read shows what the run found in it: {placed:?}"
    );
}

#[test]
fn a_report_naming_a_file_it_recorded_no_digest_for_is_refused() {
    let refused = completed(vec![(
        njutest::config::DEFAULT_CONFIGURATION,
        measured(None),
    )])
    .expect_err("a row in a file with no digest");
    assert!(
        refused.to_string().contains(
            "src/lib.rs is named by a mutation or a finding and its part recorded no digest"
        ),
        "a report a reader cannot hold to the files it read is refused where it is made: {refused}"
    );
}

#[test]
fn two_builds_that_read_different_bytes_for_one_file_are_refused() {
    let mut other = measured(Some(digest_of(EDITED.as_bytes())));
    "source-two".clone_into(&mut other.run_id);
    let refused = completed(vec![
        (
            njutest::config::DEFAULT_CONFIGURATION,
            measured(Some(digest_of(MEASURED.as_bytes()))),
        ),
        ("release", other),
    ])
    .expect_err("two builds of one run that read two trees");
    assert!(
        refused
            .to_string()
            .contains("recorded different digests for src/lib.rs, so they did not read one tree"),
        "{refused}"
    );
}

#[test]
fn a_document_that_writes_one_file_twice_is_not_read() {
    let text = njutest::report::json::document(&reported()).expect("a sound report is written");
    let mut document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("JSON");
    let sources = document["report"]["builds"][0]["parts"][0]["sources"]
        .as_array_mut()
        .expect("the sources");
    let again = sources.first().cloned().expect("one source");
    sources.push(again);
    let repeated = serde_json::to_string(&document).expect("JSON");
    let refused = njutest::report::json::parse(&repeated)
        .expect_err("one file with two entries is two answers about it");
    assert!(
        refused
            .to_string()
            .contains("repeated or out of path order"),
        "{refused}"
    );
}

#[test]
fn every_surface_says_an_edited_file_changed_and_draws_none_of_it() {
    let report = reported();
    for (text, drawn) in [(MEASURED, true), (EDITED, false)] {
        let root = holding(text);
        let said = surfaces(root.path(), &report);
        for (surface, said) in &said {
            assert!(
                !said.contains("n > 100 && n < 5"),
                "{surface} never draws an edited line: {said}"
            );
            assert_eq!(
                said.contains("the file has changed since the run read it"),
                !drawn,
                "{surface} says the file changed exactly when it did: {said}"
            );
        }
        let [(_, page), ..] = &said;
        assert_eq!(
            page.contains("n > 0"),
            drawn,
            "the page draws the line the run measured when the file still holds it: {page}"
        );
    }
}

#[test]
fn a_file_gone_since_the_run_is_said_to_be_gone_rather_than_unreadable() {
    let root = tempfile::tempdir().expect("a directory");
    for (surface, said) in surfaces(root.path(), &reported()) {
        assert!(
            said.contains("the file is not there any more"),
            "{surface} says the file the run read has gone, which a new run answers, rather than \
             that it could not be read, which sends a reader to its permissions: {said}"
        );
    }
}

#[test]
fn a_file_that_cannot_be_read_is_said_to_be_unreadable_for_the_reason_reading_it_gave() {
    let root = tempfile::tempdir().expect("a directory");
    let path = root.path().join("src/lib.rs");
    std::fs::create_dir_all(&path).expect("a directory where the file was");
    let kind = std::fs::read(&path)
        .expect_err("a directory is not read as a file")
        .kind();
    let reason = format!("the file could not be read ({kind})");
    for (surface, said) in surfaces(root.path(), &reported()) {
        assert!(
            said.contains(&reason),
            "{surface} says why reading failed, which decides what a reader does about it: {said}"
        );
    }
}

#[test]
fn a_file_holding_the_bytes_the_run_read_that_are_not_text_says_so() {
    let bytes = b"pub fn sign(n: i32) -> bool {\n    n > 0 \xff\n}\n";
    let root = holding(bytes);
    for (surface, said) in surfaces(root.path(), &reading(digest_of(bytes))) {
        assert!(
            said.contains("the file holds the bytes the run read, which are not text"),
            "{surface} does not call a file it read unreadable, nor send a reader to its \
             permissions: {said}"
        );
    }
}

#[test]
fn a_file_checked_out_with_windows_line_endings_is_drawn_without_them() {
    let checked_out = MEASURED.replace('\n', "\r\n");
    let root = holding(&checked_out);
    let sources = Sources::read(root.path(), &reading(digest_of(checked_out.as_bytes())))
        .expect("the sources a report names");
    assert_eq!(
        sources.at("src/lib.rs", 2),
        Excerpt::Read(MeasuredLine::specimen("    n > 0")),
        "a carriage return is the checkout's rather than the line's, and drawn it would return \
         the cursor over whatever the page prints after the line"
    );
}
