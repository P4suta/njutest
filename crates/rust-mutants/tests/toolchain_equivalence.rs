// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What two real builds render, compared byte for byte.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::catalog::Candidate;
use rust_mutants::equivalence::artifacts::Identity;
use rust_mutants::equivalence::{ProveOptions, Prover};
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::opening::opening;
use rust_mutants::trace::{Payload, Recorder};
use rust_mutants::workspace::OpenOptions;

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn prover(fixture: &Fixture, cancel: &Cancel) -> Prover {
    prover_with_env(fixture, cancel, toolchain_env())
}

fn toolchain_env() -> rust_mutants::vars::Variables {
    njutest_devkit::paths::environment_for_a_toolchain_run(&[])
        .into_iter()
        .collect()
}

fn prover_with_env(
    fixture: &Fixture,
    cancel: &Cancel,
    env: rust_mutants::vars::Variables,
) -> Prover {
    prover_with_observation(fixture, cancel, (env, &Recorder::disabled()))
}

fn prover_with_observation(
    fixture: &Fixture,
    cancel: &Cancel,
    (env, trace): (rust_mutants::vars::Variables, &Recorder),
) -> Prover {
    Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(njutest_devkit::paths::cargo_binary()),
                env,
                temp_directory: fixture.temp().to_path_buf(),
                locked: true,
                offline: true,
                ..OpenOptions::default()
            },
            ..ProveOptions::default()
        },
        cancel,
        trace,
    )
    .expect("the tree is copied and built")
}

#[test]
fn equivalence_builds_leave_an_ambient_cargo_target_directory_untouched() {
    let fixture = Fixture::copy("fixture-equivalent");
    let ambient = fixture.temp().join("ambient-cargo-target");
    std::fs::create_dir_all(&ambient).expect("the ambient target directory");
    let marker = ambient.join("untouched");
    std::fs::write(&marker, b"outside the prover").expect("the marker");
    let mut env = toolchain_env();
    env.remove("NJUTEST_FIXTURE_BUILD_CACHE");
    env.set("CARGO_TARGET_DIR", ambient.as_os_str());
    env.set("RUSTC_WRAPPER", "");
    let cancel = Cancel::new();
    let mut prover = prover_with_env(&fixture, &cancel, env);
    let source = std::fs::read(fixture.root().join("src/lib.rs")).expect("the library");
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let candidate = rust_mutants::syntax::discover_file("src/lib.rs", &source, &selection)
        .expect("discover")
        .candidates
        .into_iter()
        .find(|found| found.candidate.rule.name == "mul-to-div")
        .expect("the rendered mutation")
        .candidate;
    prover
        .identical(&candidate, &cancel)
        .expect("the mutated build");
    let own_target = rust_mutants::workspace::target_of(fixture.temp(), fixture.root())
        .join("equivalence")
        .join("debug");
    assert!(
        std::fs::metadata(own_target)
            .expect("the prover's target directory")
            .is_dir(),
        "the prover built inside its own target directory"
    );
    prover.close().expect("the prover closes");
    let entries: Vec<_> = std::fs::read_dir(&ambient)
        .expect("the ambient directory remains")
        .map(|entry| entry.expect("an ambient entry can be read").file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from("untouched")],
        "the ambient target directory holds only its original marker"
    );
    assert_eq!(
        std::fs::read(marker).expect("the marker remains"),
        b"outside the prover"
    );
}

/// A cargo that dates `path` of the tree it runs in back to 2000 and then runs the toolchain's own, which is what a clock that disagrees with the file's time looks like to cargo: it keeps what it built before and says the unit is fresh.
#[cfg(unix)]
fn dating_cargo(dir: &std::path::Path, path: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(dir).expect("the wrapper's directory");
    let wrapper = dir.join("cargo");
    let cargo = njutest_devkit::paths::cargo_binary();
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ntouch -c -t 200001010000 '{path}'\nexec '{}' \"$@\"\n",
            cargo.display()
        ),
    )
    .expect("the wrapper");
    let mut permissions = std::fs::metadata(&wrapper)
        .expect("the wrapper's metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("the wrapper runs");
    std::os::unix::fs::symlink(cargo.with_file_name("rustc"), dir.join("rustc"))
        .expect("the toolchain's rustc beside the wrapper");
    wrapper
}

#[test]
#[cfg(unix)]
fn a_file_a_member_reads_from_outside_its_directory_is_compiled_again_whatever_its_time() {
    let fixture = Fixture::copy("fixture-shared-path");
    let cancel = Cancel::new();
    let mut prover = Prover::open(
        fixture.root(),
        &ProveOptions {
            open: OpenOptions {
                cargo: Some(dating_cargo(
                    &fixture.temp().join("dating"),
                    "shared/util.rs",
                )),
                env: toolchain_env(),
                temp_directory: fixture.temp().to_path_buf(),
                locked: true,
                offline: true,
                ..OpenOptions::default()
            },
            ..ProveOptions::default()
        },
        &cancel,
        &Recorder::disabled(),
    )
    .expect("the tree is copied and built");
    let source = std::fs::read(fixture.root().join("shared/util.rs")).expect("the shared file");
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let candidate = rust_mutants::syntax::discover_file("shared/util.rs", &source, &selection)
        .expect("discover")
        .candidates
        .into_iter()
        .find(|found| found.candidate.rule.name == "le-to-lt")
        .expect("a mutation the compiler renders at every level")
        .candidate;
    let answer = prover.identical(&candidate, &cancel).expect("an answer");
    prover.close().expect("the tree goes away");
    assert_eq!(
        answer,
        Identity::Differs,
        "`n <= bound` and `n < bound` are two programs, and shared/util.rs lies under no member's \
         directory: the target directory's record keeps every file a member's units read, from \
         the dep-info of the build before, so the splice moves both members and cargo compiles \
         them again however old the file's time says it is"
    );
}

#[test]
fn a_mutation_the_compiler_renders_identically_is_identical_and_one_it_renders_is_not() {
    let fixture = Fixture::copy("fixture-equivalent");
    let cancel = Cancel::new();
    let recorder = rust_mutants::testkit::trace::memory_recorder();
    let mut prover = prover_with_observation(&fixture, &cancel, (toolchain_env(), &recorder));
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let source = std::fs::read(fixture.root().join("src/lib.rs")).expect("the library");
    let discovery =
        rust_mutants::syntax::discover_file("src/lib.rs", &source, &selection).expect("discover");
    let by_rule = |rule: &str| {
        discovery
            .candidates
            .iter()
            .find(|found| found.candidate.rule.name == rule)
            .expect("the requested rule has a candidate")
            .candidate
            .clone()
    };

    let rendered = prover
        .identical(&by_rule("mul-to-div"), &cancel)
        .expect("a comparison");
    assert_eq!(
        prover.withdrawn(),
        !njutest_devkit::reproducible::builds_a_reverted_change_to_the_same_bytes(),
        "what this layer can speak for is what the machine builds the same way twice, and \
         the suites that read it end to end ask that question the same way"
    );
    if prover.withdrawn() {
        assert_eq!(
            rendered,
            Identity::NotEstablished(rust_mutants::equivalence::CONTROL_DRIFTED),
            "a machine that renders one unchanged tree two ways is one this layer says \
             nothing about: reporting a difference it did not cause would be reporting \
             the machine"
        );
        prover.close().expect("the tree goes away");
        return;
    }

    assert_eq!(rendered, Identity::Differs, "`n * 2` and `n / 2` are not");
    let first_witnesses = independent_witnesses(&recorder);
    assert_eq!(
        prover
            .identical(&by_rule("add-to-sub"), &cancel)
            .expect("a comparison"),
        Identity::Identical,
        "`n + 0` and `n - 0` are the same instructions at opt-level 2"
    );
    assert!(
        !prover.withdrawn(),
        "and the original built to the same bytes every time it was asked to"
    );
    assert_eq!(
        first_witnesses, 1,
        "the first question's control is one independent compiler witness: the original and \
         the mutated builds never start one, the engine's verified record answering them"
    );
    assert_eq!(
        independent_witnesses(&recorder),
        first_witnesses,
        "the same complete restored input retains its paired actual control for every answer"
    );
    prover.close().expect("the tree goes away");
}

#[test]
fn a_mutation_whose_tree_does_not_build_is_not_established_for_that_reason() {
    let fixture = Fixture::copy("fixture-equivalent");
    let cancel = Cancel::new();
    let mut prover = Prover::open(
        fixture.root(),
        &ProveOptions {
            open: opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
            ..ProveOptions::default()
        },
        &cancel,
        &Recorder::disabled(),
    )
    .expect("the prover opens");

    let refused = Candidate {
        path: "src/lib.rs".to_owned(),
        rule: rust_mutants::rule::Registry::canonical()
            .lookup("return-default")
            .expect("a rule"),
        span: rust_mutants::span::Span::new(0, 2).expect("a span"),
        original: b"//".to_vec(),
        replacement: b"}{".to_vec(),
        source_digest: "0".repeat(64),
    };
    let answer = prover.identical(&refused, &cancel).expect("an answer");
    assert_eq!(
        answer,
        Identity::NotEstablished(rust_mutants::equivalence::DOES_NOT_BUILD),
        "a mutation the compiler refuses is not one it renders identically: the question is \
         about two programs and there is only one"
    );
    prover.close().expect("close");
}

#[test]
fn an_original_that_does_not_build_cannot_establish_a_mutation() {
    let fixture = Fixture::copy("fixture-equivalent");
    let path = fixture.root().join("src/lib.rs");
    let mut source = std::fs::read(&path).expect("the original source");
    let refusal = b"\ncompile_error!(\"the original does not build\");\n";
    let start = u32::try_from(source.len()).expect("the source length fits a span");
    let end = start
        .checked_add(u32::try_from(refusal.len()).expect("the refusal length fits a span"))
        .expect("the span fits u32");
    source.extend_from_slice(refusal);
    std::fs::write(&path, &source).expect("the original refuses to build");
    let candidate = Candidate {
        path: "src/lib.rs".to_owned(),
        rule: REGISTRY.lookup("return-default").expect("a rule"),
        span: rust_mutants::span::Span::new(start, end).expect("the refusal span"),
        original: refusal.to_vec(),
        replacement: Vec::new(),
        source_digest: "0".repeat(64),
    };
    let cancel = Cancel::new();
    let mut prover = prover(&fixture, &cancel);
    assert_eq!(
        prover.identical(&candidate, &cancel).expect("an answer"),
        Identity::NotEstablished(rust_mutants::equivalence::DID_NOT_BUILD),
        "a mutation that removes the compile failure cannot be compared with an original that never built"
    );
    prover.close().expect("close");
}

fn compiler_workspace(fixture: &Fixture, trace: &Recorder) -> rust_mutants::workspace::Workspace {
    let mut open = opening(&njutest_devkit::paths::cargo_binary(), fixture.temp());
    open.trace = trace.clone();
    open.env.remove("NJUTEST_FIXTURE_BUILD_CACHE");
    rust_mutants::workspace::Workspace::open(fixture.root(), open, &Cancel::new())
        .expect("the compiler input graph opens")
}

fn compiler_options(
    workspace: &rust_mutants::workspace::Workspace,
) -> rust_mutants::cargo::CompileOptions {
    rust_mutants::cargo::CompileOptions {
        kind: rust_mutants::cargo::CompileKind::Tests,
        locked: true,
        offline: true,
        ..rust_mutants::cargo::CompileOptions::new(workspace.build_dir())
    }
}

fn compiled_executable(compiled: &rust_mutants::cargo::Compiled) -> &std::path::Path {
    compiled
        .messages
        .iter()
        .find_map(|message| {
            if let rust_mutants::cargo::Message::CompilerArtifact(artifact) = message {
                artifact.executable.as_deref()
            } else {
                None
            }
        })
        .expect("a real test executable")
}

fn compiler_processes(trace: &Recorder) -> usize {
    trace
        .events()
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                Payload::Note { note } if note.kind == "fixture-build-process"
            )
        })
        .count()
}

#[test]
fn a_bound_compilation_keeps_its_products_when_another_source_compiles() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    let first = rust_mutants::cargo::compile(&driver, &options).expect("the original compiles");
    let original = rust_mutants::id::HexDigest::of(
        &std::fs::read(compiled_executable(&first)).expect("the original product"),
    );
    let source = driver.dir.join("src/lib.rs");
    let changed = std::fs::read_to_string(&source)
        .expect("the source")
        .replace("n * 2", "n / 2");
    std::fs::write(&source, changed).expect("a different program");
    let second =
        rust_mutants::cargo::compile(&driver, &options).expect("the changed tree compiles");
    assert_ne!(
        rust_mutants::id::HexDigest::of(
            &std::fs::read(compiled_executable(&second)).expect("the changed product")
        ),
        original
    );
    assert_eq!(
        rust_mutants::id::HexDigest::of(
            &std::fs::read(compiled_executable(&first))
                .expect("the first owned product still exists")
        ),
        original,
        "the first result owns immutable compiler products after another input uses the target"
    );
    workspace.close().expect("the source owner closes");
}

#[test]
fn a_restored_complete_input_reuses_its_original_compilation_after_a_different_build() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    let source = driver.dir.join("src/lib.rs");
    let original = std::fs::read_to_string(&source).expect("the source");
    let first = rust_mutants::cargo::compile(&driver, &options).expect("the original compiles");
    std::fs::write(&source, original.replace("n * 2", "n / 2")).expect("the changed tree");
    rust_mutants::cargo::compile(&driver, &options).expect("the changed tree compiles");
    std::fs::write(&source, original).expect("the exact original returns");
    let restored =
        rust_mutants::cargo::compile(&driver, &options).expect("the original is recovered");
    assert_eq!(
        compiler_processes(&trace),
        2,
        "A-B-A costs only the two distinct compiler inputs"
    );
    assert_eq!(
        restored.provenance,
        rust_mutants::cargo::Provenance::VerifiedReuse
    );
    assert_eq!(compiled_executable(&restored), compiled_executable(&first));
    workspace.close().expect("the source owner closes");
}

#[test]
fn concurrent_cold_requests_have_one_owned_compiler_preparation() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    let start = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _request in 0..3 {
            workers.push(njutest_devkit::thread::ScopedThread::launch(scope, || {
                start.wait();
                rust_mutants::cargo::compile(&driver, &options).expect("one complete result")
            }));
        }
        for worker in workers {
            let compiled = worker.join().expect("the preparation owner joins");
            assert_eq!(
                compiled.completion(),
                rust_mutants::cargo::Completion::Built
            );
        }
    });
    assert_eq!(
        compiler_processes(&trace),
        1,
        "all concurrent requests share the one cold producer"
    );
    workspace.close().expect("the source owner closes");
}

#[test]
fn a_cold_producer_publishes_its_refusal_and_a_changed_input_recovers() {
    let fixture = Fixture::copy("fixture-equivalent");
    let original = fixture.read("src/lib.rs");
    fixture.write("src/lib.rs", b"pub fn refused( {\n");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    let start = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_request| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    start.wait();
                    rust_mutants::cargo::compile(&driver, &options)
                        .expect("the actual compiler refusal")
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(
                worker.join().expect("the producer is joined").completion(),
                rust_mutants::cargo::Completion::Refused
            );
        }
    });
    assert_eq!(
        compiler_processes(&trace),
        1,
        "waiting requests receive the one producer refusal"
    );
    std::fs::write(driver.dir.join("src/lib.rs"), original).expect("the valid input returns");
    assert_eq!(
        rust_mutants::cargo::compile(&driver, &options)
            .expect("a new complete input recovers")
            .completion(),
        rust_mutants::cargo::Completion::Built
    );
    assert_eq!(
        compiler_processes(&trace),
        2,
        "failure publication does not poison a later input"
    );
    workspace.close().expect("the source owner closes");
}

#[test]
fn one_immutable_source_graph_feeds_six_separate_mutable_workspaces() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let mut workspaces = Vec::new();
    for _request in 0..6 {
        let mut options = opening(&njutest_devkit::paths::cargo_binary(), fixture.temp());
        options
            .env
            .set("NJUTEST_FIXTURE_BUILD_CACHE", fixture.cache());
        options.trace = trace.clone();
        workspaces.push(
            rust_mutants::workspace::Workspace::open(fixture.root(), options, &Cancel::new())
                .expect("each mutable workspace opens"),
        );
    }
    let snapshots = trace
        .events()
        .iter()
        .filter(|event| matches!(event.payload, Payload::Snapshot { .. }))
        .count();
    assert_eq!(
        snapshots, 7,
        "one immutable graph is published and six editable copies are made"
    );
    for (index, workspace) in workspaces.iter().enumerate() {
        let original = std::fs::read(workspace.snapshot_root().join("src/lib.rs"))
            .expect("the original copied bytes");
        std::fs::write(
            workspace.snapshot_root().join("src/lib.rs"),
            format!("pub const COPY: usize = {index};\n"),
        )
        .expect("only this owned mutable copy changes");
        for other in workspaces
            .iter()
            .skip(index.checked_add(1).expect("the next copy index"))
        {
            assert_eq!(
                std::fs::read(other.snapshot_root().join("src/lib.rs"))
                    .expect("another mutable copy"),
                original
            );
        }
    }
    for workspace in workspaces {
        workspace.close().expect("each source lease closes");
    }
}

#[test]
fn a_bound_toolchain_observation_answers_twice_without_another_process() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let cancel = Cancel::new();
    let mut env = toolchain_env();
    env.set("NJUTEST_FIXTURE_BUILD_CACHE", fixture.cache());
    let options = rust_mutants::cargo::LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        env: Some(env),
        ..rust_mutants::cargo::LocateOptions::default()
    };
    let watch = rust_mutants::runner::Watched::new(&cancel, &trace);
    let first = rust_mutants::cargo::Toolchain::locate(&options, fixture.root(), &watch)
        .expect("actual toolchain banners");
    let observed = trace
        .events()
        .iter()
        .filter(|event| matches!(event.payload, Payload::Exec { .. }))
        .count();
    assert!(
        observed > 0,
        "the first observation comes from real processes"
    );
    let again = rust_mutants::cargo::Toolchain::locate(&options, fixture.root(), &watch)
        .expect("the owned bound observation");
    assert_eq!(again.cargo_version(), first.cargo_version());
    assert_eq!(again.rustc_version(), first.rustc_version());
    assert_eq!(
        trace
            .events()
            .iter()
            .filter(|event| matches!(event.payload, Payload::Exec { .. }))
            .count(),
        observed,
        "an unchanged bound observation launches no probe"
    );
}

fn metadata_processes(trace: &Recorder) -> usize {
    trace
        .events()
        .iter()
        .filter(|event| matches!(&event.payload, Payload::Exec { exec } if exec.argv.get(1).is_some_and(|arg| arg == "metadata")))
        .count()
}

#[test]
fn a_bound_metadata_observation_reuses_only_its_unchanged_complete_graph() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let cancel = Cancel::new();
    let toolchain = observed_toolchain(&fixture, &cancel, &trace);
    let driver = rust_mutants::cargo::Driver {
        toolchain: &toolchain,
        dir: fixture.root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = rust_mutants::cargo::MetadataOptions {
        locked: true,
        offline: true,
    };
    let first = rust_mutants::cargo::Metadata::load(&driver, options).expect("actual metadata");
    let again = rust_mutants::cargo::Metadata::load(&driver, options).expect("owned metadata");
    assert_eq!(again.packages, first.packages);
    assert_eq!(
        metadata_processes(&trace),
        1,
        "unchanged metadata has one producer"
    );
    let original = fixture.read("src/lib.rs");
    let mut changed = original.clone();
    changed.extend_from_slice(b"\npub const OBSERVATION: u8 = 1;\n");
    fixture.write("src/lib.rs", &changed);
    rust_mutants::cargo::Metadata::load(&driver, options).expect("a new source graph");
    assert_eq!(
        metadata_processes(&trace),
        2,
        "changed source cannot reuse an old observation"
    );
    fixture.write("src/lib.rs", &original);
    rust_mutants::cargo::Metadata::load(&driver, options).expect("the original graph returns");
    assert_eq!(
        metadata_processes(&trace),
        2,
        "A-B-A recovers the retained original observation"
    );
    let unbound = rust_mutants::cargo::MetadataOptions {
        locked: false,
        offline: true,
    };
    rust_mutants::cargo::Metadata::load(&driver, unbound).expect("unlocked real metadata");
    rust_mutants::cargo::Metadata::load(&driver, unbound).expect("unlocked actual fallback");
    assert_eq!(
        metadata_processes(&trace),
        4,
        "an unbound graph always retains actual work"
    );
}

#[test]
fn an_independent_compiler_witness_retains_its_complete_input_identity() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    rust_mutants::cargo::compile_with(&driver, &options, rust_mutants::cargo::Witness::Compiler)
        .expect("an actual independent compiler process");
    let requests: Vec<_> = trace
        .events()
        .into_iter()
        .filter_map(|event| {
            if let Payload::Note { note } = event.payload
                && note.kind == "fixture-build-request"
            {
                Some(note.detail)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(requests.len(), 1);
    let request = requests.first().expect("the one actual witness request");
    assert_eq!(
        request.len(),
        64,
        "an independent witness has the same complete input identity as ordinary compilation"
    );
    assert!(request.bytes().all(|byte| byte.is_ascii_hexdigit()));
    workspace.close().expect("the compiler owner closes");
}

fn observed_toolchain(
    fixture: &Fixture,
    cancel: &Cancel,
    trace: &Recorder,
) -> rust_mutants::cargo::Toolchain {
    let mut env = toolchain_env();
    env.set("NJUTEST_FIXTURE_BUILD_CACHE", fixture.cache());
    let located = rust_mutants::cargo::LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        env: Some(env),
        ..rust_mutants::cargo::LocateOptions::default()
    };
    rust_mutants::cargo::Toolchain::locate(
        &located,
        fixture.root(),
        &rust_mutants::runner::Watched::new(cancel, trace),
    )
    .expect("the actual toolchain")
}

fn independent_witnesses(recorder: &Recorder) -> usize {
    recorder.events().iter().filter(|event| {
        matches!(&event.payload, Payload::Note { note } if note.kind == "compiler-witness")
    }).count()
}

#[test]
fn a_complete_reproducibility_pair_answers_repeated_identical_questions_once() {
    let fixture = Fixture::copy("fixture-equivalent");
    let cancel = Cancel::new();
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let mut prover = prover_with_observation(&fixture, &cancel, (toolchain_env(), &trace));
    let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, rust_mutants::rule::Tier::All);
    let discovery =
        rust_mutants::syntax::discover_file("src/lib.rs", &fixture.read("src/lib.rs"), &selection)
            .expect("the actual source catalog");
    let candidate = &discovery
        .candidates
        .iter()
        .find(|found| found.candidate.rule.name == "add-to-sub")
        .expect("the unchanged equivalent mutation")
        .candidate;
    for _question in 0..2 {
        assert_eq!(
            prover
                .identical(candidate, &cancel)
                .expect("the actual comparison"),
            Identity::Identical,
            "each identical answer retains independent compiler reproducibility evidence"
        );
    }
    assert_eq!(
        independent_witnesses(&trace),
        1,
        "the same complete input shares its paired actual compiler witness"
    );
    prover.close().expect("the source owner closes");
}

#[test]
fn a_failed_preparation_is_published_to_its_waiting_requests_and_can_recover() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let options = compiler_options(&workspace);
    std::fs::create_dir_all(options.target_dir.path()).expect("the owned target");
    let ledger = options
        .target_dir
        .path()
        .join(rust_mutants::cargo::LEDGER_NAME);
    std::fs::write(&ledger, b"not a compiler input ledger").expect("an unreadable input ledger");
    let start = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_request| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    start.wait();
                    rust_mutants::cargo::compile(&driver, &options)
                        .expect_err("preparation refuses the actual malformed ledger")
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(
                worker.join().expect("the owner joins").kind(),
                rust_mutants::cargo::CargoErrorKind::BuildLedger
            );
        }
    });
    assert_eq!(trace.events().iter().filter(|event| {
        matches!(&event.payload, Payload::Note { note } if note.kind == "compiler-preparation-failed")
    }).count(), 1, "one failed owner publishes the actual preparation refusal");
    assert_eq!(
        compiler_processes(&trace),
        0,
        "preparation failure starts no compiler"
    );
    std::fs::remove_file(ledger).expect("the repaired ledger input");
    assert_eq!(
        rust_mutants::cargo::compile(&driver, &options)
            .expect("a later request recovers")
            .completion(),
        rust_mutants::cargo::Completion::Built
    );
    workspace.close().expect("the source owner closes");
}
