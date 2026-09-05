// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation: the compiler decides which mutants are real, one at a time,
//! with its own words attached to every refusal.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rust_mutants::cargo::{
    CompileKind, CompileOptions, Driver, LocateOptions, Message, Metadata, MetadataOptions,
    Toolchain, compile,
};
use rust_mutants::catalog::{Builder, Catalog};
use rust_mutants::instrument::{Placement, instrument_file, plan_file};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::runner::Cancel;
use rust_mutants::syntax::{Selection, discover_file};
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{
    Attempt, Compile, ValidateError, ValidateOptions, Validated, attribute, validate,
};

static REGISTRY: Registry = Registry::canonical();

// --- a scripted compiler ------------------------------------------------------------

/// Instruments one file for real, then decides the outcome from a script:
/// which mutants are poison, and whether their diagnostic points inside the
/// branch (attributable) or somewhere else (not).
struct Scripted {
    path: String,
    source: Vec<u8>,
    placements: Vec<Placement>,
    catalog: Catalog,
    attributable: BTreeSet<u32>,
    unattributable: BTreeSet<u32>,
    attempts: RefCell<Vec<BTreeSet<u32>>>,
}

impl Scripted {
    fn new(source: &str, attributable: &[u32], unattributable: &[u32]) -> Self {
        let selection = Selection::tier(&REGISTRY, Tier::All);
        let discovery =
            discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
        let mut builder = Builder::new();
        for found in &discovery.candidates {
            builder.add(found.candidate.clone()).expect("add");
        }
        let catalog = builder.build().expect("catalog");
        let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
        Self {
            path: "src/lib.rs".to_owned(),
            source: source.as_bytes().to_vec(),
            placements,
            catalog,
            attributable: attributable.iter().copied().collect(),
            unattributable: unattributable.iter().copied().collect(),
            attempts: RefCell::new(Vec::new()),
        }
    }

    fn attempts(&self) -> Vec<BTreeSet<u32>> {
        self.attempts.borrow().clone()
    }
}

impl Compile for Scripted {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        self.attempts.borrow_mut().push(condemned.clone());
        let kept: Vec<Placement> = self
            .placements
            .iter()
            .filter(|placement| !condemned.contains(&placement.index))
            .cloned()
            .collect();
        let file = instrument_file(&self.path, &self.source, &kept, self.catalog.digest())?;
        let live: BTreeSet<u32> = kept.iter().map(|placement| placement.index).collect();
        let mut messages = Vec::new();
        for index in self.attributable.intersection(&live) {
            let branch = file
                .branches
                .iter()
                .find(|branch| branch.index == *index)
                .expect("a branch for a live mutant");
            messages.push(error_at(
                &self.path,
                branch.span.start,
                branch.span.end,
                *index,
            ));
        }
        for index in self.unattributable.intersection(&live) {
            // A diagnostic pointing at the file's very first byte, which no
            // branch covers.
            messages.push(error_at(&self.path, 0, 1, *index));
        }
        let success = messages.is_empty();
        messages.push(Message::BuildFinished { success });
        Ok(Attempt {
            files: vec![file],
            messages,
            success,
        })
    }
}

/// A `compiler-message` whose primary span covers `[start, end)`.
fn error_at(path: &str, start: u32, end: u32, index: u32) -> Message {
    let json = format!(
        r#"{{"reason":"compiler-message","package_id":"p","manifest_path":"/w/Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/src/lib.rs","edition":"2024"}},"message":{{"message":"mutant {index} does not compile","code":{{"code":"E0999","explanation":""}},"level":"error","spans":[{{"file_name":"{path}","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[],"rendered":"error[E0999]: mutant {index} does not compile\n"}}}}"#
    );
    rust_mutants::cargo::parse_messages(json.as_bytes()).expect("json")[0].clone()
}

fn options() -> ValidateOptions {
    ValidateOptions::default()
}

fn run(scripted: &mut Scripted) -> Result<Validated, ValidateError> {
    let catalog = scripted.catalog.clone();
    validate(&catalog, scripted, options(), &Recorder::disabled())
}

const SOURCE: &str = "pub fn f(a: i32, b: i32) -> i32 {\n    let c = a + b;\n    let d = a - b;\n    let e = a * b;\n    c + d + e\n}\n";

// --- attribution ----------------------------------------------------------------------

#[test]
fn an_error_inside_a_branch_belongs_to_that_mutant_and_one_outside_belongs_to_nobody() {
    let scripted = Scripted::new(SOURCE, &[], &[]);
    let file = instrument_file(
        "src/lib.rs",
        &scripted.source,
        &scripted.placements,
        scripted.catalog.digest(),
    )
    .expect("instrument");
    let branch = file.branches[2];
    let messages = vec![
        error_at(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        ),
        error_at("src/lib.rs", 0, 1, 999),
        error_at("src/other.rs", branch.span.start, branch.span.end, 7),
    ];
    let attributed = attribute(&[file], &messages);
    assert_eq!(attributed.condemned, BTreeSet::from([branch.index]));
    assert_eq!(
        attributed.unattributed.len(),
        2,
        "{:?}",
        attributed.unattributed
    );
    assert!(
        attributed.diagnostics[&branch.index].contains("does not compile"),
        "the compiler's own words are kept"
    );
}

#[test]
fn a_warning_is_not_a_rejection() {
    let scripted = Scripted::new(SOURCE, &[], &[]);
    let file = instrument_file(
        "src/lib.rs",
        &scripted.source,
        &scripted.placements,
        scripted.catalog.digest(),
    )
    .expect("instrument");
    let branch = file.branches[0];
    let warning = error_at(
        "src/lib.rs",
        branch.span.start,
        branch.span.end,
        branch.index,
    );
    let Message::CompilerMessage(mut message) = warning else {
        panic!("a compiler message");
    };
    message.message.level = "warning".to_owned();
    let attributed = attribute(&[file], &[Message::CompilerMessage(message)]);
    assert!(attributed.condemned.is_empty());
    assert!(attributed.unattributed.is_empty());
}

// --- the round loop -------------------------------------------------------------------

#[test]
fn a_tree_that_compiles_is_accepted_whole_in_one_round() {
    let mut scripted = Scripted::new(SOURCE, &[], &[]);
    let validated = run(&mut scripted).expect("validate");
    assert_eq!(validated.rounds, 1);
    assert!(validated.rejections.is_empty());
    assert_eq!(
        validated.accepted.len(),
        scripted.catalog.len(),
        "every mutant is accepted"
    );
    assert_eq!(scripted.attempts(), [BTreeSet::new()]);
}

#[test]
fn an_attributable_error_condemns_one_mutant_and_costs_one_more_round() {
    let mut scripted = Scripted::new(SOURCE, &[1, 4], &[]);
    let validated = run(&mut scripted).expect("validate");
    assert_eq!(validated.rounds, 2, "both are attributed in the same round");
    let rejected: Vec<u32> = validated
        .rejections
        .iter()
        .map(|rejection| rejection.index)
        .collect();
    assert_eq!(rejected, [1, 4]);
    assert!(!validated.accepted.contains(&1) && !validated.accepted.contains(&4));
    assert_eq!(
        validated.accepted.len(),
        scripted.catalog.len() - 2,
        "a refusal never costs a sibling"
    );
    let rejection = &validated.rejections[0];
    assert_eq!(rejection.code.as_deref(), Some("E0999"));
    assert!(rejection.diagnostic.contains("mutant 1 does not compile"));
    assert_eq!(rejection.path, "src/lib.rs");
    assert_eq!(
        rejection.id,
        scripted.catalog.by_index(1).expect("mutant").id
    );
    assert_eq!(
        scripted.attempts(),
        [BTreeSet::new(), BTreeSet::from([1, 4])]
    );
}

#[test]
fn an_unattributable_error_is_isolated_by_bisection() {
    let mut scripted = Scripted::new(SOURCE, &[], &[3]);
    let validated = run(&mut scripted).expect("validate");
    let rejected: Vec<u32> = validated
        .rejections
        .iter()
        .map(|rejection| rejection.index)
        .collect();
    assert_eq!(rejected, [3]);
    assert!(validated.bisections > 0, "isolation was needed");
    assert!(
        scripted.attempts().len() > 2,
        "bisection costs compilations: {:?}",
        scripted.attempts()
    );
    assert!(!validated.accepted.contains(&3));
    assert_eq!(validated.accepted.len(), scripted.catalog.len() - 1);
}

#[test]
fn a_pristine_tree_that_does_not_compile_is_not_the_mutants_fault() {
    struct Broken;
    impl Compile for Broken {
        fn attempt(&mut self, _condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
            Ok(Attempt {
                files: Vec::new(),
                messages: vec![
                    error_at("src/lib.rs", 0, 1, 0),
                    Message::BuildFinished { success: false },
                ],
                success: false,
            })
        }
    }
    let scripted = Scripted::new(SOURCE, &[], &[]);
    let error = validate(
        &scripted.catalog,
        &mut Broken,
        options(),
        &Recorder::disabled(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ValidateError::NotMutantInduced { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("RM4001"), "{error}");
}

// --- the real compiler ------------------------------------------------------------------

/// Instruments a copy of a fixture and compiles it with a real cargo.
struct CargoScripted {
    root: PathBuf,
    sources: BTreeMap<String, Vec<u8>>,
    placements: BTreeMap<String, Vec<Placement>>,
    catalog: Catalog,
    toolchain: Toolchain,
    target: PathBuf,
    _dir: tempfile::TempDir,
    _target: tempfile::TempDir,
}

impl Compile for CargoScripted {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        let mut files = Vec::new();
        for (path, placements) in &self.placements {
            let kept: Vec<Placement> = placements
                .iter()
                .filter(|placement| !condemned.contains(&placement.index))
                .cloned()
                .collect();
            let source = &self.sources[path];
            let file = instrument_file(path, source, &kept, self.catalog.digest())
                .map_err(ValidateError::from)?;
            std::fs::write(self.root.join(path), &file.text).map_err(|error| {
                ValidateError::AttemptFailed {
                    message: format!("cannot write {path}: {error}"),
                }
            })?;
            files.push(file);
        }
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        let checked = compile(
            &Driver {
                toolchain: &self.toolchain,
                dir: &self.root,
                cancel: &cancel,
                trace: &trace,
            },
            &CompileOptions {
                // The build validation ends with is the build the run
                // executes: some refusals only happen once code is generated.
                kind: CompileKind::Tests,
                target_dir: Some(self.target.clone()),
                locked: true,
                offline: true,
                timeout: None,
            },
        )
        .map_err(ValidateError::from)?;
        Ok(Attempt {
            files,
            messages: checked.messages,
            success: checked.success,
        })
    }
}

fn prepare_fixture(name: &str) -> CargoScripted {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-validate-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_dir(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let target = tempfile::Builder::new()
        .prefix("rust-mutants-validate-target-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        &root,
        &cancel,
    )
    .expect("locate");
    let driver = Driver {
        toolchain: &toolchain,
        dir: &root,
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
    let checked = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Check,
            target_dir: Some(target.path().to_path_buf()),
            locked: true,
            offline: true,
            timeout: None,
        },
    )
    .expect("check");
    let discovery = rust_mutants::discover::discover(
        &rust_mutants::discover::Input {
            root: &root,
            metadata: &metadata,
            units: &checked.units,
        },
        &rust_mutants::discover::DiscoverOptions {
            selection: Selection::tier(&REGISTRY, Tier::All),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
        },
        &trace,
    )
    .expect("discover");
    let found: Vec<rust_mutants::syntax::Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut sources = BTreeMap::new();
    let mut placements = BTreeMap::new();
    for file in &discovery.files {
        if file.candidates == 0 {
            continue;
        }
        sources.insert(
            file.path.clone(),
            std::fs::read(root.join(&file.path)).expect("read"),
        );
        placements.insert(
            file.path.clone(),
            plan_file(&discovery.catalog, &file.path, &found).expect("plan"),
        );
    }
    CargoScripted {
        root,
        sources,
        placements,
        catalog: discovery.catalog,
        toolchain,
        target: target.path().to_path_buf(),
        _dir: dir,
        _target: target,
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

#[test]
fn the_compiler_decides_which_mutants_are_real_and_says_why_for_each() {
    let mut fixture = prepare_fixture("fixture-rejectable");
    let catalog = fixture.catalog.clone();
    let validated =
        validate(&catalog, &mut fixture, options(), &Recorder::disabled()).expect("validate");

    let mut rejected: Vec<(&str, &str)> = validated
        .rejections
        .iter()
        .map(|rejection| {
            (
                catalog
                    .by_index(rejection.index)
                    .expect("mutant")
                    .candidate
                    .rule
                    .name,
                rejection.code.as_deref().unwrap_or(""),
            )
        })
        .collect();
    rejected.sort_unstable();
    assert_eq!(
        rejected,
        [
            ("add-to-sub", "E0369"),
            ("mul-to-div", "unconditional_panic"),
            ("range-to-inclusive", "E0308"),
            ("return-default", "E0277"),
        ],
        "{:?}",
        validated
            .rejections
            .iter()
            .map(|r| r.diagnostic.lines().next().unwrap_or(""))
            .collect::<Vec<_>>()
    );
    assert!(
        validated
            .rejections
            .iter()
            .any(|rejection| rejection.diagnostic.contains("cannot subtract")),
        "the compiler's own words are attached"
    );
    assert_eq!(
        validated.accepted.len() + validated.rejections.len(),
        catalog.len(),
        "every mutant is accounted for"
    );
    assert!(validated.rounds >= 2);

    // The tree left behind holds exactly the accepted mutants and compiles.
    let final_attempt = fixture
        .attempt(&validated.rejections.iter().map(|r| r.index).collect())
        .expect("attempt");
    assert!(final_attempt.success, "the accepted tree compiles");
}
