// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A file edited after the run is never drawn as the code the run measured, on any surface.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;

use njutest::config::Contract;
use njutest::presentation::{Excerpt, Missing, Sources};
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
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let run = rust_mutants::id::RunId::try_from("one").expect("a canonical run id");
    let njutest::report::LatticedDocument::Complete(latticed) =
        njutest::report::across::configured(&run, &measurements).expect("one checked lattice")
    else {
        panic!("a whole-catalog fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

/// A workspace holding `text` at `src/lib.rs`.
fn holding(text: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::create_dir_all(root.path().join("src")).expect("src");
    std::fs::write(root.path().join("src/lib.rs"), text).expect("the file");
    root
}

fn excerpt(root: &Path) -> Excerpt {
    Sources::read(root, &reported())
        .expect("the sources a report names")
        .at("src/lib.rs", 2, ">")
}

#[test]
fn a_file_edited_after_the_run_is_not_drawn_as_the_code_the_run_measured() {
    let measured = holding(MEASURED);
    assert_eq!(
        excerpt(measured.path()),
        Excerpt::Read("    n > 0".to_owned()),
        "the file the run measured is drawn"
    );
    let edited = holding(EDITED);
    assert_eq!(
        excerpt(edited.path()),
        Excerpt::Instead(Missing::Moved),
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
