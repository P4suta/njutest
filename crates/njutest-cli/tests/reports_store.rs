// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run keeps of itself, and what a later run may take away.

#![expect(
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

#[derive(Debug, thiserror::Error)]
enum PlatformReportError {
    #[error(transparent)]
    RunId(#[from] rust_mutants::id::RunIdError),
    #[error(transparent)]
    Measurements(#[from] njutest_cli::report::across::BuildMeasurementsError),
    #[error(transparent)]
    Configured(#[from] njutest_cli::report::across::ConfiguredError),
    #[error(transparent)]
    Completion(#[from] njutest_cli::report::CompletionError),
    #[error("the whole-catalog fixture produced a shard report")]
    UnexpectedShard,
}

fn platform_report() -> Result<njutest_cli::report::Report, PlatformReportError> {
    let mut source = njutest_cli::report::BuildReport::new(
        "platform-evidence",
        njutest_cli::report::RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
    );
    "demo".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "a".repeat(64);
    source.repository.configuration_digest = "b".repeat(64);
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest_cli::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.started);
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.finished);
    source
        .limitations
        .push(njutest_cli::report::Limitation::new(
            "git-metadata-unavailable",
            "the platform-contract fixture is not a git repository",
        ));
    let run_id = rust_mutants::id::RunId::try_from("platform-contract")?;
    let measurements = njutest_cli::report::across::BuildMeasurements::checked(vec![(
        njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])?;
    let latticed = njutest_cli::report::across::configured(&run_id, &measurements)?;
    let njutest_cli::report::LatticedDocument::Complete(latticed) = latticed else {
        return Err(PlatformReportError::UnexpectedShard);
    };
    Ok(latticed.complete_without_models()?)
}

#[test]
fn the_authority_backend_contract_is_explicit_on_every_platform() {
    let directory = tempfile::tempdir().expect("an isolated report store");
    let report = platform_report().expect("the checked platform-contract report");

    #[cfg(unix)]
    {
        let kept = njutest_cli::app::reports::keep(directory.path(), &report)
            .expect("Unix has the capability-rooted authority backend");
        assert!(
            kept.indexes.permits_retention(),
            "a supported backend completes canonical and derived publication"
        );
    }

    #[cfg(not(unix))]
    {
        use njutest_cli::app::reports::{Index, StoreError, keep, pointed_at, retain};

        assert!(
            matches!(
                keep(directory.path(), &report),
                Err(StoreError::UnsupportedCapability)
            ),
            "an unsupported host cannot publish through a path fallback"
        );
        assert!(
            matches!(
                pointed_at(directory.path(), Index::Any),
                Err(StoreError::UnsupportedCapability)
            ),
            "an unsupported host cannot treat a pathname as index authority"
        );
        assert!(
            matches!(
                retain(directory.path(), 1),
                Err(StoreError::UnsupportedCapability)
            ),
            "an unsupported host cannot delete through a pathname inventory"
        );
    }
}

#[cfg(unix)]
mod unix {

    use std::path::{Path, PathBuf};

    use njutest_cli::app::reports::{
        DOCUMENT_NAME, HTML_NAME, Index, JUNIT_NAME, SARIF_NAME, SCHEMA_NAME, StoreError, keep,
        pointed_at, retain,
    };
    use njutest_cli::report::across::{BuildMeasurements, BuildMeasurementsError, ConfiguredError};
    use njutest_cli::report::{BuildReport, CompletionError, LatticedDocument, Report, RunKind};
    use rust_mutants::cargo::BuildConfig;
    use rust_mutants::id::{RunId, RunIdError, StoredRunId};

    fn stored_run_id(value: &str) -> StoredRunId {
        StoredRunId::try_from(value).expect("a path-safe stored run id")
    }

    /// The default fixture report root. Production deliberately keeps this
    /// spelling behind a held [`njutest_cli::app::reports::Store`]; tests that
    /// arrange hostile filesystem entries spell the documented default locally
    /// instead of reopening a production authority as a raw path.
    fn report_root(root: &Path) -> PathBuf {
        root.join(njutest_cli::config::DEFAULT_REPORTS_DIRECTORY)
    }

    fn runs_root(root: &Path) -> PathBuf {
        report_root(root).join("runs")
    }

    fn run_path(root: &Path, run: &StoredRunId) -> PathBuf {
        runs_root(root).join(run.as_str())
    }

    fn index_path(root: &Path, index: Index) -> PathBuf {
        report_root(root).join(index.file())
    }

    fn named_run(run: &StoredRunId) -> String {
        format!(
            "{}/runs/{}",
            njutest_cli::config::DEFAULT_REPORTS_DIRECTORY,
            run.as_str()
        )
    }

    /// A workspace with one directory per run named, and an index pointing where asked.
    fn filled(runs: &[&str], indexes: &[(Index, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        for run in runs {
            std::fs::create_dir_all(run_path(&root, &stored_run_id(run))).expect("a run directory");
        }
        for (index, run) in indexes {
            let path = index_path(&root, *index);
            std::fs::create_dir_all(path.parent().unwrap_or(&root)).expect("the index's directory");
            std::fs::write(
                &path,
                serde_json::json!({
                    "schema": njutest_cli::report::SCHEMA,
                    "run_id": run,
                    "directory": named_run(&stored_run_id(run)),
                })
                .to_string(),
            )
            .expect("the index");
        }
        (dir, root)
    }

    /// The names of the run directories still there.
    fn left(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(runs_root(root))
            .expect("the runs directory")
            .map(|entry| entry.expect("every run-directory entry is readable"))
            .map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .expect("test protocol paths are UTF-8")
                    .to_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_collection_keeps_the_newest_and_says_what_it_took() {
        let runs = [
            "20260101T000000Z-aaaaaa",
            "20260102T000000Z-bbbbbb",
            "20260103T000000Z-cccccc",
            "20260104T000000Z-dddddd",
        ];
        let (directory_owner, root) = filled(&runs, &[]);

        let removed = retain(&root, 2).expect("the runs directory is readable");
        assert_eq!(
            left(&root),
            vec![runs[2].to_owned(), runs[3].to_owned()],
            "a run identity starts with the time it began, so the newest two are the last \
         two in order: keeping the first two would keep the ones nobody is looking at"
        );
        assert_eq!(
            removed.len(),
            2,
            "and the collection says what it took, because a person watching a directory \
         shrink wants to know it was this and not something else: {removed:?}"
        );
        assert!(
            removed.iter().all(|path| path
                .file_name()
                .is_some_and(|name| name == runs[0] || name == runs[1])),
            "naming each one: {removed:?}"
        );
        drop(directory_owner);
    }

    #[test]
    fn a_collection_never_takes_a_run_an_index_still_names() {
        let runs = [
            "20260101T000000Z-aaaaaa",
            "20260102T000000Z-bbbbbb",
            "20260103T000000Z-cccccc",
        ];
        let (directory_owner, root) =
            filled(&runs, &[(Index::Any, runs[2]), (Index::Full, runs[0])]);

        let removed = retain(&root, 1).expect("the runs directory is readable");
        assert_eq!(
            removed.len(),
            1,
            "only the unindexed middle run is collected"
        );
        let kept = left(&root);
        assert!(
            kept.contains(&runs[0].to_owned()),
            "the oldest is what an index names, and an index pointing at a directory a \
         collection took is a reader sent to nothing: `njutest report` would answer \
         about a run whose report is gone. It kept {kept:?}"
        );
        assert!(
            kept.contains(&runs[2].to_owned()),
            "the newest is kept because it is the newest, and also because the other index \
         names it: {kept:?}"
        );
        assert!(
            !kept.contains(&runs[1].to_owned()),
            "and the one in the middle, which nothing points at and nothing is the newest \
         of, is the one a collection is for: {kept:?}"
        );
        drop(directory_owner);
    }

    #[test]
    fn a_collection_asked_to_keep_everything_takes_nothing() {
        let runs = ["20260101T000000Z-aaaaaa", "20260102T000000Z-bbbbbb"];
        let (directory_owner, root) = filled(&runs, &[]);
        assert!(
            retain(&root, u32::MAX)
                .expect("the runs directory is readable")
                .is_empty()
                && left(&root).len() == 2,
            "a store nobody bounded is one a person is keeping on purpose: taking anything \
         from it would be this program deciding how much history somebody may have"
        );
        assert!(
            retain(&root, 0)
                .expect("the runs directory is readable")
                .len()
                == 2
                && left(&root).is_empty(),
            "while one bounded at nothing keeps nothing, and says so rather than quietly \
         treating zero as one"
        );
        drop(directory_owner);
    }

    #[test]
    fn an_index_that_names_nothing_is_read_as_naming_nothing() {
        let (directory_owner, root) = filled(&["20260101T000000Z-aaaaaa"], &[]);
        assert_eq!(
            pointed_at(&root, Index::Any).expect("an absent index is readable"),
            None,
            "a directory where no run has finished has no latest run, and answering with one \
         would send a reader to a report nobody wrote"
        );

        std::fs::write(index_path(&root, Index::Any), "not an index\n")
            .expect("a file that is not one");
        assert!(
            pointed_at(&root, Index::Any).is_err(),
            "an index this release cannot read is corrupt, not absent: treating it as no \
         index would silently select a different run"
        );

        let (named_directory_owner, named) = filled(
            &["20260101T000000Z-aaaaaa"],
            &[(Index::Any, "20260101T000000Z-aaaaaa")],
        );
        assert_eq!(
            pointed_at(&named, Index::Any)
                .expect("the index is exact")
                .as_ref()
                .map(StoredRunId::as_str),
            Some("20260101T000000Z-aaaaaa"),
            "while one that names a run answers with it"
        );
        drop(named_directory_owner);
        drop(directory_owner);
    }

    #[test]
    fn an_index_run_is_a_typed_component_and_its_other_fields_are_exact() {
        let (directory_owner, root) = filled(&["20260101T000000Z-aaaaaa"], &[]);
        let path = index_path(&root, Index::Any);
        for document in [
            serde_json::json!({
                "schema": njutest_cli::report::SCHEMA,
                "run_id": "../../outside",
                "directory": "../../outside",
            }),
            serde_json::json!({
                "schema": njutest_cli::report::SCHEMA,
                "run_id": "20260101T000000Z-aaaaaa",
                "directory": "../../outside",
            }),
            serde_json::json!({
                "schema": njutest_cli::report::SCHEMA,
                "run_id": "20260101T000000Z-aaaaaa",
                "directory": named_run(&stored_run_id("20260101T000000Z-aaaaaa")),
                "extra": true,
            }),
        ] {
            std::fs::write(
                &path,
                serde_json::to_vec(&document).expect("an adversarial index"),
            )
            .expect("the index fixture");
            assert!(
                pointed_at(&root, Index::Any).is_err(),
                "a traversal, mismatched path, or unknown field is corruption: {document}"
            );
        }
        drop(directory_owner);
    }

    #[test]
    fn a_report_run_id_is_validated_before_any_path_is_made() {
        let dir = tempfile::tempdir().expect("tempdir");
        for invalid in ["../../outside", "20260101T000000Z-ABCDEF"] {
            let refused = complete(invalid, report_draft(RunKind::Full));
            assert!(
                matches!(refused, Err(FixtureReportError::RunId(_))),
                "an unsafe or non-canonical identity {invalid:?} cannot acquire report or path \
             capability: {refused:?}"
            );
        }
        assert!(
            !dir.path()
                .join("outside")
                .try_exists()
                .expect("inspectable"),
            "validation happens before an escaping path can be created"
        );
    }

    #[test]
    fn a_new_run_cannot_alias_a_historical_spelling_by_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(runs_root(dir.path()).join("RUN-A"))
            .expect("a historical run directory");

        let refused = keep(dir.path(), &keepable("run-a", RunKind::Full))
            .expect_err("a case-fold alias is not a writable run");
        assert!(
            matches!(
                &refused,
                StoreError::NotKept { source, .. }
                    if source.kind() == std::io::ErrorKind::AlreadyExists
            ),
            "the complete store inventory rejects the alias before writing: {refused}"
        );
        assert!(
            !runs_root(dir.path())
                .join("RUN-A")
                .join(DOCUMENT_NAME)
                .try_exists()
                .expect("the historical directory is inspectable"),
            "the historical directory was not overwritten through a case-insensitive alias"
        );
    }

    #[test]
    fn an_exact_canonical_run_never_replaces_published_evidence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let report = keepable("same-run", RunKind::Full);
        let first = keep(dir.path(), &report).expect("the first write");
        let document = first.directory.join(DOCUMENT_NAME);
        let original = std::fs::read(&document).expect("the first document");
        let second = keep(dir.path(), &report).expect_err("a published run is immutable");
        assert!(
            matches!(
                &second,
                StoreError::NotKept { source, .. }
                    if source.kind() == std::io::ErrorKind::AlreadyExists
            ),
            "the exclusive capability claim refuses an existing authority: {second}"
        );
        assert_eq!(
            std::fs::read(&document).expect("the unchanged first document"),
            original,
            "a repeated identity cannot replace evidence that readers may already hold"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_stand_in_for_a_run_directory_or_index() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(runs_root(root)).expect("the runs root");
        let outside = tempfile::tempdir().expect("outside");
        symlink(outside.path(), run_path(root, &stored_run_id(BLOCKED_RUN)))
            .expect("a run symlink");
        assert!(
            matches!(
                keep(root, &keepable(BLOCKED_RUN, RunKind::Full)),
                Err(StoreError::NotKept { .. })
            ),
            "a typed run id does not make a symlink eligible for an exclusive publication claim"
        );

        let index = index_path(root, Index::Any);
        let target = outside.path().join("index.json");
        std::fs::write(&target, "{}").expect("a target");
        symlink(&target, &index).expect("an index symlink");
        assert!(
            matches!(
                pointed_at(root, Index::Any),
                Err(StoreError::Index { path, .. }) if path == index
            ),
            "an owned index is a no-follow regular file, never an external path through a link"
        );
    }

    /// Why a mutable fixture draft could not take the checked transition to a durable report.
    #[derive(Debug, thiserror::Error)]
    enum FixtureReportError {
        /// The final namespace was not writable on every supported filesystem.
        #[error(transparent)]
        RunId(#[from] RunIdError),
        /// The fixture did not describe exactly one primary build.
        #[error(transparent)]
        Measurements(#[from] BuildMeasurementsError),
        /// The source evidence did not form one sound build lattice.
        #[error(transparent)]
        Configured(#[from] ConfiguredError),
        /// The checked lattice could not take the non-model completion transition.
        #[error(transparent)]
        Completion(#[from] CompletionError),
        /// A whole-catalog fixture unexpectedly became a partial report.
        #[error("a whole-catalog fixture produced a shard report")]
        UnexpectedShard,
    }

    /// Mutable source evidence for one report-store fixture.
    fn report_draft(kind: RunKind) -> BuildReport {
        let mut report = BuildReport::new(
            "fixture-evidence",
            kind,
            njutest_cli::config::Contract::StandardV1,
        );
        "demo".clone_into(&mut report.repository.root_name);
        report.repository.workspace_digest = "a".repeat(64);
        report.repository.configuration_digest = "b".repeat(64);
        "rustc 1.98.0".clone_into(&mut report.toolchain.rustc);
        report.scope.configured_builds =
            vec![njutest_cli::config::DEFAULT_CONFIGURATION.to_owned()];
        "2026-01-01T00:00:00Z".clone_into(&mut report.timing.started);
        "2026-01-01T00:00:00Z".clone_into(&mut report.timing.finished);
        report
            .limitations
            .push(njutest_cli::report::Limitation::new(
                "git-metadata-unavailable",
                "the tree a test builds is not a git repository",
            ));
        report
    }

    /// Consumes mutable source evidence through the same checked lattice and
    /// contract transition as production before a store can observe it.
    fn complete(run: &str, report: BuildReport) -> Result<Report, FixtureReportError> {
        let final_run = RunId::try_from(run)?;
        let measurements = BuildMeasurements::checked(vec![(
            njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
            BuildConfig::default().selection(),
            report,
        )])?;
        let latticed = njutest_cli::report::across::configured(&final_run, &measurements)?;
        let LatticedDocument::Complete(latticed) = latticed else {
            return Err(FixtureReportError::UnexpectedShard);
        };
        Ok(latticed.complete_without_models()?)
    }

    /// A report a run may keep: one that passed every constructor and audit.
    fn keepable(run: &str, kind: RunKind) -> Report {
        complete(run, report_draft(kind)).expect("the fixture evidence forms one durable report")
    }

    /// Both latest-run pointers, decoded through the production reader.
    fn latest(root: &Path) -> (Option<StoredRunId>, Option<StoredRunId>) {
        (
            pointed_at(root, Index::Any).expect("the any index"),
            pointed_at(root, Index::Full).expect("the full index"),
        )
    }

    #[test]
    fn a_run_keeps_every_projection_beside_its_document_and_points_both_indexes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let run = "20260101t000000z-aaaaaa";
        let kept = keep(root, &keepable(run, RunKind::Full)).expect("a report a run may keep");

        for name in [
            DOCUMENT_NAME,
            SCHEMA_NAME,
            HTML_NAME,
            SARIF_NAME,
            JUNIT_NAME,
            njutest_cli::report::lines::FILE_NAME,
        ] {
            assert!(
                kept.directory
                    .join(name)
                    .try_exists()
                    .expect("the published projection path is inspectable"),
                "the surface a team already reads is the one it will read this on, and a \
             projection that has to be generated later is one nobody generates: {name}"
            );
        }
        assert_eq!(
            std::fs::read(kept.directory.join(SCHEMA_NAME)).expect("the copied schema"),
            include_bytes!("../../../schema/njutest-assurance-report-v1.json"),
            "the schema beside a report is the exact schema this release publishes"
        );
        assert_eq!(
            std::fs::read_to_string(index_path(root, Index::Any),).expect("the latest index"),
            format!(
                "{{\n  \"directory\": \"{}\",\n  \"run_id\": \"{run}\",\n  \
             \"schema\": \"njutest-assurance-report-v1\"\n}}\n",
                named_run(&stored_run_id(run))
            ),
            "an index is a stable newline-terminated interface, not merely JSON that happens to parse"
        );
        assert_eq!(
            latest(root),
            (Some(stored_run_id(run)), Some(stored_run_id(run))),
            "a run over the whole project is the latest of any kind and the latest full one"
        );

        let narrowed = "20260102t000000z-bbbbbb";
        let kept = keep(root, &keepable(narrowed, RunKind::Changed)).expect("a report");
        assert!(
            std::fs::metadata(kept.directory.join(DOCUMENT_NAME))
                .expect("the canonical document is readable")
                .is_file(),
            "keeping a run publishes its canonical report"
        );
        assert_eq!(
            latest(root),
            (Some(stored_run_id(narrowed)), Some(stored_run_id(run))),
            "while a run that looked at only what changed is the latest of any kind and not \
         the latest full one: a reader asking what the whole project last established \
         would otherwise be handed an answer about a handful of files"
        );
    }

    #[test]
    fn a_report_that_fails_its_own_audit_is_not_written_at_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let run = "20260101t000000z-aaaaaa";
        let mut wrong = report_draft(RunKind::Full);
        wrong.accounting.targets.selected = 1;
        wrong.accounting.targets.passed = 1;

        let refused = complete(run, wrong)
            .expect_err("target accounting cannot claim a row the ledger does not carry");
        assert!(
            !runs_root(root)
                .join(run)
                .try_exists()
                .expect("the refused run path is inspectable")
                && pointed_at(root, Index::Any)
                    .expect("no index was written")
                    .is_none(),
            "accounting with no corresponding target row is evidence nobody may be handed — \
         and half of one on disk with no index naming it is worse than none: a later \
         collection reads the directory as a run: {refused}"
        );
    }

    /// The run the blocked-path cases are about.
    const BLOCKED_RUN: &str = "20260101t000000z-aaaaaa";

    /// One path a run has to write, asked of the type that owns the layout.
    type Wanted = fn(&Path) -> PathBuf;

    #[test]
    fn every_failed_projection_and_index_names_the_path_that_was_not_kept() {
        let blocked: [(Wanted, bool, Option<Index>); 9] = [
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)),
                true,
                None,
            ),
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)).join(DOCUMENT_NAME),
                false,
                None,
            ),
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)).join(SCHEMA_NAME),
                false,
                None,
            ),
            (
                |root| {
                    run_path(root, &stored_run_id(BLOCKED_RUN))
                        .join(njutest_cli::report::lines::FILE_NAME)
                },
                false,
                None,
            ),
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)).join(HTML_NAME),
                false,
                None,
            ),
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)).join(SARIF_NAME),
                false,
                None,
            ),
            (
                |root| run_path(root, &stored_run_id(BLOCKED_RUN)).join(JUNIT_NAME),
                false,
                None,
            ),
            (|root| index_path(root, Index::Any), false, Some(Index::Any)),
            (
                |root| index_path(root, Index::Full),
                false,
                Some(Index::Full),
            ),
        ];

        for (of, file_in_place_of_directory, derived_index) in blocked {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = of(dir.path());
            std::fs::create_dir_all(path.parent().unwrap_or_else(|| dir.path()))
                .expect("the parent");
            if file_in_place_of_directory {
                std::fs::write(&path, "occupied").expect("a file where a directory is needed");
            } else {
                std::fs::create_dir_all(&path).expect("a directory where a file is needed");
            }

            if let Some(expected) = derived_index {
                let kept = keep(dir.path(), &keepable(BLOCKED_RUN, RunKind::Full))
                    .expect("the canonical report is authoritative even when an index is blocked");
                assert!(
                    matches!(
                        kept.indexes,
                        njutest_cli::app::reports::IndexPublication::Incomplete { index, .. }
                            if index == expected
                    ),
                    "the derived index failure is a typed successful publication"
                );
                std::fs::read(kept.directory.join(DOCUMENT_NAME))
                    .expect("an index failure cannot roll back the canonical publication");
                continue;
            }
            let error = keep(dir.path(), &keepable(BLOCKED_RUN, RunKind::Full))
                .expect_err("one authoritative output path refused the report");
            assert!(
                matches!(&error, StoreError::NotKept { .. }),
                "{} failed as {error}. A caller cannot acquire the private staging capability \
             for a run whose public authority is already occupied",
                path.display()
            );
            let obstacle = std::fs::symlink_metadata(&path)
                .expect("a refused publication cannot replace the obstacle");
            assert_eq!(
                obstacle.is_file(),
                file_in_place_of_directory,
                "the pre-existing authority remains exactly the caller-owned entry"
            );
        }
    }
}
