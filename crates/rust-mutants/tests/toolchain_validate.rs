// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a real compiler refuses, mutant by mutant, in its own words.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use njutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    CompileKind, CompileOptions, Driver, LocateOptions, Metadata, MetadataOptions, Toolchain,
    compile,
};
use rust_mutants::catalog::Catalog;
use rust_mutants::instrument::{Instrumenting, Placement, instrument_file, plan_file};
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::runner::Watched;
use rust_mutants::syntax::Selection;
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{
    Attempt, Compile, Condemnation, Constness, ValidateError, ValidateOptions, validate,
};

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
    fn attempt(
        &mut self,
        condemned: &BTreeSet<u32>,
        constness: &Constness,
    ) -> Result<Attempt, ValidateError> {
        let mut files = Vec::new();
        let kept: BTreeMap<&String, Vec<Placement>> = self
            .placements
            .iter()
            .map(|(path, placements)| {
                let kept = placements
                    .iter()
                    .filter(|placement| !condemned.contains(&placement.index))
                    .cloned()
                    .collect();
                (path, kept)
            })
            .collect();
        let carriers = constness.carriers_of(
            kept.iter()
                .map(|(path, placements)| (path.as_str(), placements.as_slice())),
        );
        for (path, kept) in &kept {
            let source = &self.sources[*path];
            let file = instrument_file(&Instrumenting {
                path,
                source,
                placements: kept,
                carriers: carriers.get(*path).map_or(&[][..], Vec::as_slice),
                markers: &[],
                comparable: &BTreeSet::default(),
                probed: &BTreeMap::default(),
                catalog_digest: self.catalog.digest(),
                first_item: 0,
                watched: "/watched",
            })
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
                kind: CompileKind::Tests,
                packages: Vec::new(),
                target_dir: rust_mutants::cargo::BuildDir::new(self.target.clone(), Vec::new()),
                locked: true,
                offline: true,
                timeout: None,
                env: rust_mutants::vars::Variables::empty(),
                build: rust_mutants::cargo::BuildConfig::default(),
            },
        )
        .map_err(ValidateError::from)?;
        let written =
            u32::try_from(files.len()).map_err(|_overflow| ValidateError::AccountingOverflow {
                what: "attempt written file count",
            })?;
        Ok(Attempt {
            files,
            completion: checked.completion(),
            messages: checked.messages,
            written,
        })
    }
}

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn options() -> ValidateOptions {
    ValidateOptions::default()
}

fn prepare_fixture(name: &str) -> CargoScripted {
    prepare_fixture_with(name, |_| {})
}

fn prepare_fixture_with(name: &str, arrange: impl FnOnce(&std::path::Path)) -> CargoScripted {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-validate-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_tree(&njutest_devkit::paths::fixtures_dir().join(name), &root);
    arrange(&root);
    let target = tempfile::Builder::new()
        .prefix("rust-mutants-validate-target-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(
                njutest_devkit::paths::environment_for_a_run()
                    .into_iter()
                    .collect(),
            ),
            ..LocateOptions::default()
        },
        &root,
        &Watched::new(&cancel, &Recorder::disabled()),
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
            packages: Vec::new(),
            target_dir: rust_mutants::cargo::BuildDir::new(target.path().to_path_buf(), Vec::new()),
            locked: true,
            offline: true,
            timeout: None,
            env: rust_mutants::vars::Variables::empty(),
            build: rust_mutants::cargo::BuildConfig::default(),
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
            narrowing: Vec::new(),
            packages: Vec::new(),
            skips: Vec::new(),
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

#[test]
fn generated_guards_preserve_user_lints_coercions_temporaries_and_control_flow() {
    let source = r#"#![deny(warnings)]

trait Named {
    fn name(&self) -> &'static str;
}
struct A;
struct B;
impl Named for A {
    fn name(&self) -> &'static str { "a" }
}
impl Named for B {
    fn name(&self) -> &'static str { "b" }
}

pub fn positive(value: u32) -> bool {
    value > 0
}

pub fn signed(value: i32) -> bool {
    value > 0
}

fn coerced(flag: bool) -> &'static dyn Named {
    if flag { &A } else { &B }
}

pub fn temporary() -> usize {
    let held: &str = &(String::from("a") + "b");
    held.len()
}

pub fn questioned(value: Option<i32>) -> Option<i32> {
    Some(value? + 1)
}

pub fn broken(value: i32) -> i32 {
    loop {
        break value + 1;
    }
}

pub async fn awaited(value: i32) -> i32 {
    async { value + 1 }.await + 1
}

pub fn name(flag: bool) -> &'static str {
    coerced(flag).name()
}
"#;
    let mut fixture = prepare_fixture_with("fixture-rejectable", |root| {
        std::fs::write(root.join("src/lib.rs"), source).expect("write strict source");
    });
    let catalog = fixture.catalog.clone();
    let positive_at =
        u32::try_from(source.find("value > 0").expect("positive comparison") + "value ".len())
            .expect("small source");
    let signed_at =
        u32::try_from(source.rfind("value > 0").expect("signed comparison") + "value ".len())
            .expect("small source");
    let positive = catalog
        .mutants()
        .iter()
        .find(|mutant| {
            mutant.candidate.rule.name == "gt-to-ge" && mutant.candidate.span.start == positive_at
        })
        .expect("the unsigned comparison mutation");
    let signed = catalog
        .mutants()
        .iter()
        .find(|mutant| {
            mutant.candidate.rule.name == "gt-to-ge" && mutant.candidate.span.start == signed_at
        })
        .expect("the signed comparison mutation");

    let validated = validate(
        &catalog,
        &mut fixture,
        &rust_mutants::validate::Validating {
            options: options(),
            cancel: &Cancel::new(),
            trace: &Recorder::disabled(),
        },
    )
    .expect("validate strict source");
    let rejection = validated
        .rejections
        .iter()
        .find(|rejection| rejection.index == positive.index)
        .expect("u32 >= 0 is refused by the crate's warning policy");
    assert_eq!(rejection.code.as_deref(), Some("unused_comparisons"));
    assert!(rejection.isolated, "{rejection:?}");
    assert!(
        validated.accepted.contains(&signed.index),
        "the same mutation over a signed value remains legal: {validated:?}"
    );

    let standalone = source.replacen("value > 0", "value >= 0", 1);
    std::fs::write(fixture.root.join("src/lib.rs"), standalone)
        .expect("write the standalone mutant");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let standalone = compile(
        &Driver {
            toolchain: &fixture.toolchain,
            dir: &fixture.root,
            cancel: &cancel,
            trace: &trace,
        },
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: Vec::new(),
            target_dir: rust_mutants::cargo::BuildDir::new(fixture.target.clone(), Vec::new()),
            locked: true,
            offline: true,
            timeout: None,
            env: rust_mutants::vars::Variables::empty(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("compile the standalone mutant");
    assert_eq!(
        standalone.completion(),
        rust_mutants::cargo::Completion::Refused,
        "{standalone:?}"
    );
    let standalone_code = standalone
        .messages
        .iter()
        .find_map(|message| match message {
            rust_mutants::cargo::Message::CompilerMessage(message)
                if message.message.is_error() =>
            {
                message.message.code.as_deref()
            }
            _ => None,
        });
    assert_eq!(
        standalone_code,
        rejection.code.as_deref(),
        "the instrumented branch is refused exactly as the standalone edit is"
    );

    let final_attempt = fixture
        .attempt(
            &validated.rejections.iter().map(|one| one.index).collect(),
            &Constness::default(),
        )
        .expect("final strict attempt");
    assert!(
        final_attempt.completion == rust_mutants::cargo::Completion::Built,
        "the identity macro preserves coercions, temporary extension, ?, break, and async: {:?}",
        final_attempt.messages
    );
}

#[test]
fn the_compiler_decides_which_mutants_are_real_and_says_why_for_each() {
    let mut fixture = prepare_fixture("fixture-rejectable");
    let catalog = fixture.catalog.clone();
    let validated = validate(
        &catalog,
        &mut fixture,
        &rust_mutants::validate::Validating {
            options: options(),
            cancel: &Cancel::new(),
            trace: &Recorder::disabled(),
        },
    )
    .expect("validate");

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

    let final_attempt = fixture
        .attempt(
            &validated.rejections.iter().map(|r| r.index).collect(),
            &Constness::default(),
        )
        .expect("attempt");
    assert_eq!(
        final_attempt.completion,
        rust_mutants::cargo::Completion::Built,
        "the accepted tree compiles"
    );
}

/// A const item that calls a chain of two `const fn`s, and a `const fn` whose only guard the compiler refuses, which calls a third that only the program calls.
const EVALUATED: &str = "pub const X: u32 = outer(1);

pub const fn outer(n: u32) -> u32 {
    inner(n) + 1
}

pub const fn inner(n: u32) -> u32 {
    n * 2
}

pub struct Wrapped(pub u32);

pub const fn relay(n: u32) -> Wrapped {
    Wrapped(lone(n))
}

pub const fn lone(n: u32) -> u32 {
    n - 1
}
";

#[test]
fn a_chain_the_compiler_evaluates_gives_back_one_const_a_round_and_a_caller_only_the_program_calls_carries_the_guard()
 {
    let mut fixture = prepare_fixture_with("fixture-rejectable", |root| {
        std::fs::write(root.join("src/lib.rs"), EVALUATED).expect("write the chain");
    });
    let catalog = fixture.catalog.clone();
    let of = |name: &str| -> BTreeSet<u32> {
        let placements: Vec<Placement> = fixture
            .placements
            .values()
            .flat_map(|file| file.iter().cloned())
            .collect();
        placements
            .iter()
            .filter(|placement| {
                placement
                    .hint
                    .const_fn
                    .as_ref()
                    .is_some_and(|function| function.name == name)
            })
            .map(|placement| placement.index)
            .collect()
    };
    let (outer, inner, relay, lone) = (of("outer"), of("inner"), of("relay"), of("lone"));
    assert!(
        [&outer, &inner, &relay, &lone]
            .iter()
            .all(|held| !held.is_empty()),
        "every function holds a guard going in"
    );
    let validated = validate(
        &catalog,
        &mut fixture,
        &rust_mutants::validate::Validating {
            options: options(),
            cancel: &Cancel::new(),
            trace: &Recorder::disabled(),
        },
    )
    .expect("validation settles");
    let left = |reason: Condemnation| -> BTreeSet<u32> {
        validated
            .rejections
            .iter()
            .filter(|rejection| rejection.reason == reason)
            .map(|rejection| rejection.index)
            .collect()
    };
    let evaluated: BTreeSet<u32> = outer.union(&inner).copied().collect();
    assert_eq!(
        left(Condemnation::EvaluatedBeforeRun),
        evaluated,
        "the const item evaluates outer, and outer, const again, evaluates inner: every mutant of \
         both is left out for that, and none of any other function is: {:?}",
        validated.rejections
    );
    assert!(
        validated
            .rejections
            .iter()
            .filter(|rejection| rejection.reason == Condemnation::EvaluatedBeforeRun)
            .all(|rejection| rejection.code.as_deref() == Some("E0015")),
        "in the compiler's own words, which name the call it would have to make before the \
         program runs: {:?}",
        validated.rejections
    );
    assert!(
        relay.is_subset(&left(Condemnation::CompilerRefused)),
        "relay's only guard is a return the compiler refuses, as it would anywhere: {:?}",
        validated.rejections
    );
    assert!(
        lone.iter().all(|index| validated.accepted.contains(index)),
        "relay keeps its const only for want of a guard, and it calls lone, which nothing \
         evaluates before the program runs: relay goes without its const too, so lone's \
         mutants are mutants: {validated:?}"
    );
    assert_eq!(
        (validated.rounds, validated.bisections),
        (3, 0),
        "outer in the first round, inner and relay's call in the second, and a tree that builds \
         in the third: each round learns something about a function, so the rounds end without \
         a bisection, and the build is never refused"
    );
}

/// A `const fn` holding no guard whose own `const` calls a function that holds one, and another whose body calls it, each kept `const` by a marker.
const NESTED: &str = "pub struct Held;

impl Held {
    pub const fn make(n: u32) -> u32 {
        n * 2
    }
}

// rust-mutants: skip the constant inside is what the compiler evaluates
pub const fn nests(n: u32) -> u32 {
    const INNER: u32 = Held::make(1);
    n + INNER
}

// rust-mutants: skip only the program calls this one
pub const fn keeps(n: u32) -> u32 {
    Held::make(n)
}
";

#[test]
fn the_pinned_compiler_s_own_spans_tell_a_constant_inside_a_const_fn_from_its_body() {
    let mut fixture = prepare_fixture_with("fixture-rejectable", |root| {
        std::fs::write(root.join("src/lib.rs"), NESTED).expect("write the nesting");
    });
    let catalog = fixture.catalog.clone();
    let placements: Vec<Placement> = fixture
        .placements
        .values()
        .flat_map(|file| file.iter().cloned())
        .collect();
    let make: BTreeSet<u32> = placements
        .iter()
        .filter(|placement| {
            placement
                .hint
                .const_fn
                .as_ref()
                .is_some_and(|function| function.name == "make")
        })
        .map(|placement| placement.index)
        .collect();
    assert!(
        !make.is_empty() && make.len() == placements.len(),
        "make holds every guard, and the markers keep nests and keeps without one: {placements:?}"
    );
    let validated = validate(
        &catalog,
        &mut fixture,
        &rust_mutants::validate::Validating {
            options: options(),
            cancel: &Cancel::new(),
            trace: &Recorder::disabled(),
        },
    )
    .expect("validation settles");
    let evaluated: BTreeSet<u32> = validated
        .rejections
        .iter()
        .filter(|rejection| rejection.reason == Condemnation::EvaluatedBeforeRun)
        .map(|rejection| rejection.index)
        .collect();
    assert_eq!(
        evaluated, make,
        "the call inside INNER is inside nests's body, and the compiler evaluates INNER on its \
         own: where it points is enough to say make is evaluated before the program runs, so \
         every mutant of make is left out for that: {:?}",
        validated.rejections
    );
    assert_eq!(
        (validated.rounds, validated.bisections),
        (2, 0),
        "the first round pins make for INNER and learns keeps's call, and the second builds: \
         keeps carries nothing once make keeps its const"
    );
}

/// A session over `fixture`, with `RUSTFLAGS` set to `flags` or left alone.
fn prepared(
    fixture: &njutest_devkit::fixture::Fixture,
    flags: Option<&str>,
) -> rust_mutants::session::Session {
    let cancel = Cancel::new();
    let mut options = rust_mutants::testkit::opening::opening(
        &njutest_devkit::paths::cargo_binary(),
        fixture.temp(),
    );
    if let Some(flags) = flags {
        options.env.set("RUSTFLAGS", flags);
    }
    rust_mutants::workspace::Workspace::open(fixture.root(), options, &cancel)
        .expect("the workspace opens")
        .prepare(
            &rust_mutants::session::PrepareOptions {
                tier: Tier::All,
                ..rust_mutants::session::PrepareOptions::new(Tier::Balanced)
            },
            &cancel,
        )
        .expect("the session prepares")
}

/// Whether `candidate`, written into a fresh copy of `name` by hand, compiles with every warning denied.
fn compiles_by_hand(name: &str, candidate: &rust_mutants::catalog::Candidate) -> bool {
    let fixture = njutest_devkit::fixture::Fixture::copy(name);
    let path = fixture.root().join(&candidate.path);
    let mut source = std::fs::read(&path).expect("the mutated file");
    let start = usize::try_from(candidate.span.start).expect("a small offset");
    let end = usize::try_from(candidate.span.end).expect("a small offset");
    source.splice(start..end, candidate.replacement.iter().copied());
    std::fs::write(&path, source).expect("the edit by hand");
    let mut command = std::process::Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args(["check", "--all-targets", "--offline", "--locked", "--quiet"])
        .current_dir(fixture.root())
        .env("RUSTFLAGS", "-D warnings")
        .env("CARGO_TARGET_DIR", fixture.temp().join("by-hand"));
    njutest_devkit::cost::cargo(command, "a hand-written validation build")
        .expect("cargo runs")
        .status
        .success()
}

#[test]
fn denying_warnings_refuses_no_mutant_the_generated_code_alone_would_warn_about() {
    for name in [
        "fixture-probeable",
        "fixture-simple",
        "fixture-families",
        "fixture-modern",
    ] {
        let fixture = njutest_devkit::fixture::Fixture::copy(name);
        let lenient: BTreeSet<String> = prepared(&fixture, None)
            .rejections()
            .iter()
            .map(|rejection| rejection.id.clone())
            .collect();
        let strict = prepared(&fixture, Some("-D warnings"));
        let added: Vec<&str> = strict
            .rejections()
            .iter()
            .filter(|rejection| !lenient.contains(&rejection.id))
            .filter(|rejection| {
                let mutant = strict
                    .resolve(&rejection.id)
                    .expect("a refused mutant resolves");
                compiles_by_hand(name, &mutant.candidate)
            })
            .map(|rejection| rejection.diagnostic.as_str())
            .collect();
        assert!(
            added.is_empty(),
            "{name}: denying warnings refused mutants that compile cleanly when written by hand, \
             so the lint came from code the engine wrote around them rather than from the \
             mutation: {added:#?}"
        );
    }
}
