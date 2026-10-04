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
    open.env = toolchain_env();
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

#[test]
fn an_opaque_loader_graph_always_keeps_its_actual_toolchain_processes() {
    for name in ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
        let fixture = Fixture::copy("fixture-equivalent");
        let trace = rust_mutants::testkit::trace::memory_recorder();
        let cancel = Cancel::new();
        let mut env = toolchain_env();
        env.set("NJUTEST_FIXTURE_BUILD_CACHE", fixture.cache());
        env.set(name, fixture.temp());
        let options = rust_mutants::cargo::LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(env),
            ..rust_mutants::cargo::LocateOptions::default()
        };
        let watch = rust_mutants::runner::Watched::new(&cancel, &trace);
        for _question in 0..2 {
            rust_mutants::cargo::Toolchain::locate(&options, fixture.root(), &watch)
                .expect("the real toolchain still runs under an opaque loader graph");
        }
        assert_eq!(
            trace
                .events()
                .iter()
                .filter(|event| { matches!(event.payload, Payload::Exec { .. }) })
                .count(),
            6,
            "{name} binds no reusable executable or loader graph"
        );
    }
}

#[test]
fn a_changed_observation_command_cannot_certify_its_old_actual_result() {
    let fixture = Fixture::copy("fixture-equivalent");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let cancel = Cancel::new();
    let first = observed_toolchain(&fixture, &cancel, &trace);
    let root = fixture.cache().join("rust-mutants-tool-observations-v1");
    let record = std::fs::read_dir(&root)
        .expect("the bound observation owner")
        .map(|entry| entry.expect("an observation").path().join("located.json"))
        .find(|path| std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()))
        .expect("the actual located record");
    let mut value: serde_json::Value = njutest_devkit::strictjson::decode_slice(
        &std::fs::read(&record).expect("the actual observation"),
    )
    .expect("the complete strict observation");
    *value
        .pointer_mut("/processes/0/exec/argv/0")
        .expect("the actual command identity") =
        serde_json::Value::String("a-command-that-did-not-make-this-observation".to_owned());
    std::fs::write(
        record,
        serde_json::to_vec(&value).expect("retain the changed provenance"),
    )
    .expect("the planted observation corruption");
    let recovered = observed_toolchain(&fixture, &cancel, &trace);
    assert_eq!(recovered.cargo_version(), first.cargo_version());
    assert_eq!(
        trace
            .events()
            .iter()
            .filter(|event| { matches!(event.payload, Payload::Exec { .. }) })
            .count(),
        6,
        "changed causal provenance requires new actual compiler observations"
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
        "an independent witness retains its complete input identity: {request}"
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

fn capture_program_processes(trace: &Recorder) -> usize {
    trace.events().iter().filter(|event| {
        matches!(&event.payload, Payload::Exec { exec } if exec.argv.iter().any(|arg| arg == "--crate-name=capture"))
    }).count()
}

#[test]
fn concurrent_capture_program_requests_have_one_immutable_producer() {
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
    let directory = fixture.temp().join("capture-program");
    let start = std::sync::Barrier::new(3);
    let mut products = Vec::new();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    start.wait();
                    rust_mutants::cargo::build_capture(&driver, &directory)
                })
            })
            .collect();
        for worker in workers {
            products.push(worker.join().expect("the capture preparation owner joins"));
        }
    });
    assert_eq!(
        capture_program_processes(&trace),
        1,
        "one cold capture preparation owns its real compiler"
    );
    let first = products
        .first()
        .expect("a producer result")
        .as_ref()
        .expect("the capture built");
    for product in &products {
        assert_eq!(product.as_ref().expect("every request succeeds"), first);
    }
    let original = std::fs::read(first).expect("the actual capture program");
    std::fs::write(first, b"an altered capture program").expect("the original artifact is altered");
    let recovered =
        rust_mutants::cargo::build_capture(&driver, &directory).expect("verified recovery");
    assert_ne!(
        &recovered, first,
        "recovery cannot overwrite an earlier reader's product path"
    );
    assert_eq!(
        std::fs::read(&recovered).expect("the new compiler program"),
        original
    );
    assert_eq!(
        std::fs::read(first).expect("the altered original stays observable"),
        b"an altered capture program"
    );
    assert_eq!(capture_program_processes(&trace), 2);
    workspace.close().expect("the source owner closes");
}

fn owned_doctest_capture(
    driver: &rust_mutants::cargo::Driver<'_>,
    capture: &rust_mutants::cargo::DoctestCapture<'_>,
) -> (Vec<u8>, std::path::PathBuf) {
    let owned = rust_mutants::cargo::capture_prepared_doctests(driver, capture)
        .expect("the actual owned Cargo capture");
    (owned.report().to_vec(), owned.directory().to_path_buf())
}

#[test]
fn a_complete_doctest_capture_has_one_owned_actual_compiler() {
    let fixture = Fixture::copy("fixture-doctest");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let mut options = compiler_options(&workspace);
    options.build.target = Some(rust_mutants::sealed::TARGET.to_owned());
    let directory = fixture.temp().join("doctests");
    let program = rust_mutants::cargo::build_capture(&driver, &fixture.temp().join("capture"))
        .expect("the real capture program");
    let capture = rust_mutants::cargo::DoctestCapture {
        package: "fixture-doctest",
        capture: (&program, &directory),
        compile: &options,
        baked: rust_mutants::sealed::doctest::Baked::List,
    };
    let (report, first) = owned_doctest_capture(&driver, &capture);
    let held = rust_mutants::sealed::doctest::Held::read(&first).expect("actual captured binaries");
    let listing =
        rust_mutants::sealed::doctest::listing(&report).expect("a complete real rustdoc listing");
    let binaries = rust_mutants::sealed::doctest::merged_binaries(&listing, &held)
        .expect("every listing claim is held");
    assert!(
        !binaries.is_empty(),
        "the positive control compiles and captures real doctests"
    );
    let original: Vec<_> = binaries
        .iter()
        .map(|path| std::fs::read(path).expect("captured original bytes"))
        .collect();
    let again = owned_doctest_capture(&driver, &capture);
    assert_eq!(
        compiler_processes(&trace),
        1,
        "one complete capture starts one actual Cargo producer"
    );
    assert_eq!(again, (report.clone(), first.clone()));
    let source = workspace.snapshot_root().join("src/lib.rs");
    let pristine = std::fs::read_to_string(&source).expect("the real source");
    std::fs::write(&source, pristine.replace("n * 2", "n * 3"))
        .expect("a different complete graph");
    let changed = owned_doctest_capture(&driver, &capture);
    assert_ne!(changed.1, first);
    for (path, bytes) in binaries.iter().zip(original) {
        assert_eq!(
            std::fs::read(path).expect("an earlier reader keeps its product"),
            bytes
        );
    }
    std::fs::write(&source, pristine).expect("input A returns");
    assert_eq!(owned_doctest_capture(&driver, &capture), (report, first));
    assert_eq!(
        compiler_processes(&trace),
        2,
        "A-B-A reuses the original immutable capture"
    );
    workspace.close().expect("the source owner closes");
}

struct NativeCaptureInvocation {
    cwd: std::path::PathBuf,
    argv: [std::ffi::OsString; 3],
    environment: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
}

const fn native_invocation_take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, rest) = bytes
        .split_at_checked(length)
        .expect("the actual invocation has every framed byte");
    *bytes = rest;
    value
}

fn native_invocation_number(bytes: &mut &[u8]) -> usize {
    let value = u64::from_le_bytes(
        native_invocation_take(bytes, 8)
            .try_into()
            .expect("an exact count"),
    );
    usize::try_from(value).expect("the count fits this actual host")
}

fn native_invocation_string(bytes: &mut &[u8]) -> std::ffi::OsString {
    let length = native_invocation_number(bytes);
    let value = native_invocation_take(bytes, length);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        std::ffi::OsString::from_vec(value.to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;
        let mut chunks = value.chunks_exact(2);
        let wide: Vec<_> = chunks
            .by_ref()
            .map(|pair| u16::from_le_bytes(pair.try_into().expect("two actual Windows bytes")))
            .collect();
        assert!(chunks.remainder().is_empty());
        std::ffi::OsString::from_wide(&wide)
    }
}

fn native_capture_invocation(bytes: &[u8]) -> NativeCaptureInvocation {
    assert!(bytes.starts_with(b"rust-mutants-native-invocation-v1\0"));
    let mut frames = bytes
        .strip_prefix(b"rust-mutants-native-invocation-v1\0")
        .expect("the exact native invocation schema");
    assert_eq!(
        native_invocation_take(&mut frames, 1),
        [u8::from(cfg!(windows))]
    );
    let cwd = std::path::PathBuf::from(native_invocation_string(&mut frames));
    let arguments: Vec<_> = (0..native_invocation_number(&mut frames))
        .map(|_| native_invocation_string(&mut frames))
        .collect();
    assert_eq!(arguments.len(), 3);
    let argv = arguments
        .try_into()
        .expect("the complete original native argv");
    let mut environment = std::collections::BTreeMap::new();
    for _ in 0..native_invocation_number(&mut frames) {
        assert!(
            environment
                .insert(
                    native_invocation_string(&mut frames),
                    native_invocation_string(&mut frames)
                )
                .is_none()
        );
    }
    assert!(frames.is_empty());
    NativeCaptureInvocation {
        cwd,
        argv,
        environment,
    }
}

fn native_capture_markers(options: &mut rust_mutants::cargo::CompileOptions) -> std::ffi::OsString {
    options.env.set(
        "NJUTEST_CAPTURE_INVOCATION_CONTROL",
        "actual-native-capture\noriginal-environment\t",
    );
    #[cfg(unix)]
    let raw_marker = {
        use std::os::unix::ffi::OsStringExt as _;
        std::ffi::OsString::from_vec(vec![0xff, b'\n', b'\t'])
    };
    #[cfg(windows)]
    let raw_marker = {
        use std::os::windows::ffi::OsStringExt as _;
        std::ffi::OsString::from_wide(&[0xd800, 10, 9])
    };
    options
        .env
        .set("NJUTEST_CAPTURE_RAW_OS_CONTROL", &raw_marker);
    raw_marker
}

fn assert_native_capture_sidecar(
    binary: &std::path::Path,
    (cwd, program, directory): (&std::path::Path, &std::path::Path, &std::path::Path),
    raw_marker: &std::ffi::OsString,
) {
    let sidecar = binary.with_extension("invocation");
    let bytes = std::fs::read(&sidecar)
        .expect("each actual rustdoc invocation must retain its original argv/cwd/environment");
    let marker = "actual-native-capture\noriginal-environment\t";
    #[cfg(unix)]
    let encoded_marker = marker.as_bytes().to_vec();
    #[cfg(windows)]
    let encoded_marker: Vec<u8> = marker.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert!(
        bytes
            .windows(encoded_marker.len())
            .any(|part| part == encoded_marker)
    );
    let invocation = native_capture_invocation(&bytes);
    assert_eq!(invocation.cwd, cwd);
    let [helper, claimed_directory, original] = invocation.argv;
    assert_eq!(std::path::Path::new(&helper), program);
    assert_eq!(
        std::path::Path::new(&claimed_directory).parent(),
        Some(directory)
    );
    assert!(std::path::Path::new(&original).is_absolute());
    assert_ne!(std::path::Path::new(&original), binary);
    assert_eq!(
        invocation
            .environment
            .get(std::ffi::OsStr::new("NJUTEST_CAPTURE_INVOCATION_CONTROL")),
        Some(&std::ffi::OsString::from(marker)),
    );
    assert_eq!(
        invocation
            .environment
            .get(std::ffi::OsStr::new("NJUTEST_CAPTURE_RAW_OS_CONTROL")),
        Some(raw_marker),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&sidecar)
                .expect("private original invocation")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

fn assert_native_sidecar_corruption_recompiles(
    driver: &rust_mutants::cargo::Driver<'_>,
    capture: &rust_mutants::cargo::DoctestCapture<'_>,
    first: &rust_mutants::cargo::PreparedDoctests,
    binary: &std::path::Path,
) {
    std::fs::write(
        binary.with_extension("invocation"),
        b"changed original invocation",
    )
    .expect("change only this control's owned sidecar");
    let repaired = rust_mutants::cargo::capture_prepared_doctests(driver, capture)
        .expect("corrupted invocation custody needs a new actual producer");
    assert_ne!(repaired.directory(), first.directory());
    assert_eq!(compiler_processes(driver.trace), 2);
    assert_eq!(capture_program_processes(driver.trace), 1);
}

fn assert_native_sidecar_publication_refuses(
    driver: &rust_mutants::cargo::Driver<'_>,
    program: &std::path::Path,
    binary: &std::path::Path,
    directory: &std::path::Path,
) {
    std::fs::create_dir_all(directory).expect("owned failed-publication directory");
    let original_sidecar = directory.join("0.invocation");
    std::fs::write(&original_sidecar, b"original private record")
        .expect("a pre-existing invocation must never be overwritten");
    let mut spec = rust_mutants::runner::Spec::new(
        [
            program.as_os_str().to_owned(),
            directory.as_os_str().to_owned(),
            binary.as_os_str().to_owned(),
        ],
        rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
    );
    spec.env = driver.toolchain.env().cloned();
    spec.dir = Some(driver.dir.to_path_buf());
    let result = rust_mutants::runner::run(&spec, driver.cancel);
    driver
        .trace
        .exec_result(rust_mutants::trace::ExecRecord::of(&spec, &result));
    assert_eq!(result.termination.exit_code(), Some(3));
    assert!(result.stdout.is_empty());
    assert_eq!(
        std::fs::metadata(directory.join("0.wasm"))
            .expect_err("failed invocation publication cannot publish a program")
            .kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(
        std::fs::read(original_sidecar).expect("the original private record remains"),
        b"original private record"
    );
}

#[test]
fn a_real_native_doctest_capture_retains_every_actual_invocation() {
    let fixture = Fixture::copy("fixture-doctest");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let mut options = compiler_options(&workspace);
    options.build.target = Some(workspace.toolchain().host().to_owned());
    let raw_marker = native_capture_markers(&mut options);
    let program = rust_mutants::cargo::build_capture(&driver, &fixture.temp().join("capture"))
        .expect("the single actual shared host helper");
    let directory = fixture.temp().join("native-doctests");
    let capture = rust_mutants::cargo::DoctestCapture {
        package: "fixture-doctest",
        capture: (&program, &directory),
        compile: &options,
        baked: rust_mutants::sealed::doctest::Baked::Run,
    };
    let first = rust_mutants::cargo::capture_prepared_doctests(&driver, &capture)
        .expect("the actual native Cargo/rustdoc capture");
    let held = rust_mutants::sealed::doctest::Held::read(first.directory())
        .expect("the original compiler's captured claims");
    assert!(
        !held.binaries.is_empty(),
        "real native doctests were compiled"
    );
    for binary in held.binaries.values() {
        assert_native_capture_sidecar(
            binary,
            (workspace.snapshot_root(), &program, &directory),
            &raw_marker,
        );
    }
    let reused = rust_mutants::cargo::capture_prepared_doctests(&driver, &capture)
        .expect("the same complete capture is verified again");
    assert_eq!(reused.directory(), first.directory());
    assert_eq!(reused.report(), first.report());
    assert_eq!(
        compiler_processes(&trace),
        1,
        "verified invocation reuse starts no additional Cargo"
    );
    assert_eq!(
        capture_program_processes(&trace),
        1,
        "native capture uses one actual host helper"
    );
    let binary = held
        .binaries
        .values()
        .next()
        .expect("an actual original native program");
    assert_native_sidecar_corruption_recompiles(&driver, &capture, &first, binary);
    assert_native_sidecar_publication_refuses(
        &driver,
        &program,
        binary,
        &fixture.temp().join("refused-invocation"),
    );
    workspace.close().expect("the owned source closes");
}

#[cfg(unix)]
fn execute_original_native_products(
    driver: &rust_mutants::cargo::Driver<'_>,
    products: &rust_mutants::cargo::NativeDoctestProducts,
) {
    for program in products.programs() {
        products
            .verify()
            .expect("original runtime inputs before execution");
        let mut spec = rust_mutants::runner::Spec::new(
            std::iter::once(program.executable().as_os_str().to_owned())
                .chain(program.arguments().iter().cloned()),
            rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
        );
        spec.dir = Some(program.cwd().to_path_buf());
        spec.env = Some(program.environment().clone());
        let result = rust_mutants::runner::run(&spec, driver.cancel);
        driver
            .trace
            .exec_result(rust_mutants::trace::ExecRecord::of(&spec, &result));
        assert!(result.succeeded(), "actual native execution: {result:?}");
        products
            .verify()
            .expect("original runtime inputs after execution");
    }
}

#[cfg(unix)]
fn native_program_processes(
    trace: &Recorder,
    products: &rust_mutants::cargo::NativeDoctestProducts,
) -> usize {
    trace.events().iter().filter(|event| {
        matches!(&event.payload, Payload::Exec { exec } if products.programs().iter().any(|program| {
            program.executable().to_str().is_some_and(|path| exec.argv.first().is_some_and(|arg| arg == path))
        }))
    }).count()
}

#[cfg(unix)]
fn native_library_change_refuses_originals(
    (first, reused): (
        &rust_mutants::cargo::NativeDoctestProducts,
        &rust_mutants::cargo::NativeDoctestProducts,
    ),
    library: &std::path::Path,
) {
    let added = library.join("added-native-library");
    std::fs::write(&added, b"changed original native search namespace")
        .expect("the actual original search namespace changes");
    for products in [first, reused] {
        assert_eq!(
            products
                .verify()
                .expect_err("a retained original namespace cannot attest a new library")
                .kind(),
            rust_mutants::cargo::CargoErrorKind::BuildLedger
        );
    }
    std::fs::remove_file(added).expect("the original native namespace is restored");
    first
        .verify()
        .expect("original namespace after restoration");
    reused
        .verify()
        .expect("reused original namespace after restoration");
}

#[cfg(unix)]
fn assert_original_native_reuse(
    driver: &rust_mutants::cargo::Driver<'_>,
    first: &rust_mutants::cargo::NativeDoctestProducts,
    (package, options): (
        &rust_mutants::cargo::Package,
        &rust_mutants::cargo::CompileOptions,
    ),
    harness_args: &[String],
) -> rust_mutants::cargo::NativeDoctestProducts {
    let reused =
        rust_mutants::cargo::prepare_native_doctests(driver, package, options, harness_args)
            .expect("reuse restores the original serialized runtime attestations");
    assert_eq!(first.observation().id(), reused.observation().id());
    assert_eq!(first.capture_program(), reused.capture_program());
    assert_eq!(reused.harness_args(), harness_args);
    assert_eq!(
        compiler_processes(driver.trace),
        1,
        "unchanged native reuse starts no Cargo"
    );
    assert_eq!(
        capture_program_processes(driver.trace),
        1,
        "native reuse has one actual helper compiler"
    );
    execute_original_native_products(driver, &reused);
    assert_eq!(
        native_program_processes(driver.trace, first),
        first
            .programs()
            .len()
            .checked_mul(2)
            .expect("paired actual native execution count")
    );
    reused
}

#[cfg(unix)]
#[test]
fn actual_native_products_retain_original_runtime_inputs_and_fresh_executions() {
    let fixture = Fixture::copy("fixture-doctest");
    let trace = rust_mutants::testkit::trace::memory_recorder();
    let workspace = compiler_workspace(&fixture, &trace);
    let cancel = Cancel::new();
    let driver = rust_mutants::cargo::Driver {
        toolchain: workspace.toolchain(),
        dir: workspace.snapshot_root(),
        cancel: &cancel,
        trace: &trace,
    };
    let package = workspace
        .metadata()
        .packages
        .iter()
        .find(|package| package.name == "fixture-doctest")
        .expect("the original documented package");
    let library = fixture.temp().join("original-native-libraries");
    std::fs::create_dir_all(&library).expect("the original native library namespace");
    let mut options = compiler_options(&workspace);
    options.env.set(
        if cfg!(target_os = "macos") {
            "DYLD_LIBRARY_PATH"
        } else {
            "LD_LIBRARY_PATH"
        },
        library.as_os_str(),
    );
    let harness_args = ["--test-threads=1".to_owned()];
    let first =
        rust_mutants::cargo::prepare_native_doctests(&driver, package, &options, &harness_args)
            .expect("actual original native compiler and invocation publication");
    assert!(
        !first.programs().is_empty(),
        "actual native programs were captured"
    );
    assert_eq!(
        first.compiler_only().len(),
        1,
        "the original compile-fail attestation remains compiler-only"
    );
    execute_original_native_products(&driver, &first);
    let reused = assert_original_native_reuse(&driver, &first, (package, &options), &harness_args);
    native_library_change_refuses_originals((&first, &reused), &library);
    assert_eq!(
        compiler_processes(&trace),
        1,
        "namespace verification starts no phantom Cargo"
    );
    workspace
        .close()
        .expect("the original native source owner closes");
}
