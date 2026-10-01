// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a real cargo answers: the toolchain it names, the metadata it prints, and the dep-info it leaves behind.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    CargoErrorKind, Diagnostic, Driver, LocateOptions, Message, Metadata, MetadataOptions,
    Toolchain, compile_time_inputs, emitted_of, parse_messages, resolve_executable, units_of,
};
use rust_mutants::runner::{Cancel, RunResult, Spec, run};
use rust_mutants::trace::Recorder;

fn fixture(name: &str) -> PathBuf {
    njutest_devkit::paths::fixtures_dir().join(name)
}

fn toolchain(dir: &Path) -> Toolchain {
    let mut env: rust_mutants::vars::Variables = njutest_devkit::paths::environment_for_a_run()
        .into_iter()
        .collect();
    env.set("RUSTC_WRAPPER", "");
    let options = LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
        env: Some(env),
        ..LocateOptions::default()
    };
    Toolchain::locate(&options, dir, &Cancel::new()).expect("locate")
}
fn scratch_target(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("rust-mutants-{name}-"))
        .tempdir()
        .expect("tempdir")
}

fn run_build(spec: &Spec) -> RunResult {
    let result = run(spec, &Cancel::new());
    let messages = parse_messages(&result.stdout).expect("Cargo messages");
    rust_mutants::cargo::record_build(
        spec.env.as_ref(),
        spec.dir.as_deref().expect("build root"),
        result.duration,
        &messages,
    )
    .expect("direct build diagnostics");
    result
}
/// Where `path` is below `root`, spelled the one way a catalog spells a path.
fn under(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("under the root")
        .to_str()
        .expect("fixture paths are exact UTF-8")
        .replace('\\', "/")
}

#[test]
fn an_explicit_cargo_path_must_exist_and_a_bare_name_is_searched_on_the_given_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = resolve_executable(Path::new("/definitely/not/cargo"), None);
    assert!(
        missing.is_err(),
        "the absent absolute path is refused: {missing:?}"
    );
    let Err(missing) = missing else { return };
    assert_eq!(missing.kind(), CargoErrorKind::ToolchainNotFound);
    assert!(missing.to_string().contains("RM1012"), "{missing}");

    let real = njutest_devkit::paths::cargo_binary();
    assert_eq!(resolve_executable(&real, None).expect("exists"), real);

    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let fake = bin.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&fake, "not run by this lookup\n").expect("write");
    let search = std::env::join_paths([temp.path().join("empty"), bin]).expect("a search path");
    assert_eq!(
        resolve_executable(Path::new("cargo"), Some(search.as_os_str())).expect("found"),
        fake
    );
    let none = resolve_executable(Path::new("cargo"), Some(OsString::from("").as_os_str()));
    assert!(
        none.is_err(),
        "an empty search path finds no cargo: {none:?}"
    );
    let Err(none) = none else { return };
    assert_eq!(none.kind(), CargoErrorKind::ToolchainNotFound);
    let bare_without_path = resolve_executable(Path::new("no-such-tool-xyz"), None);
    assert!(
        bare_without_path.is_err(),
        "a missing bare tool is refused: {bare_without_path:?}"
    );
    let Err(bare_without_path) = bare_without_path else {
        return;
    };
    assert_eq!(bare_without_path.kind(), CargoErrorKind::ToolchainNotFound);
}
#[test]
fn variables_added_to_an_environment_the_toolchain_was_never_given_are_refused() {
    let dir = scratch_target("unenvironed");
    let tc = Toolchain::locate(
        &LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        dir.path(),
        &Cancel::new(),
    )
    .expect("locate without an explicit environment");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let mut options = rust_mutants::cargo::CompileOptions::new(rust_mutants::cargo::BuildDir::new(
        dir.path().join("target"),
        Vec::new(),
    ));
    options.env.set("RUST_MUTANTS_ADDED", "1");
    let refused = rust_mutants::cargo::compile(
        &Driver {
            toolchain: &tc,
            dir: dir.path(),
            cancel: &cancel,
            trace: &trace,
        },
        &options,
    )
    .expect_err(
        "a toolchain given no environment inherits this process's, so the variables a build adds \
         have nothing to be added to, and an empty set in its place loses everything inherited",
    );
    assert!(
        refused.to_string().contains("was given none"),
        "the refusal says the toolchain was given no environment: {refused}"
    );
}

#[test]
fn locating_reads_both_versions_from_inside_the_directory() {
    let dir = fixture("fixture-simple");
    let tc = toolchain(&dir);
    assert_eq!(tc.cargo(), njutest_devkit::paths::cargo_binary());
    assert!(!tc.cargo_version().release.is_empty());
    assert!(!tc.rustc_version().release.is_empty());
    assert_eq!(tc.host(), tc.rustc_version().host);
    assert!(tc.host().contains('-'), "{}", tc.host());
    let spec = tc.command(&dir, ["metadata", "--format-version", "1"]);
    assert_eq!(spec.argv[0], tc.cargo().as_os_str());
    assert_eq!(spec.argv[1], "metadata");
    assert_eq!(spec.dir.as_deref(), Some(dir.as_path()));
    let described = tc.to_string();
    assert!(
        described.contains("cargo ") && described.contains("rustc "),
        "{described}"
    );
}

fn build_trace() -> Recorder {
    Recorder::wall(
        rust_mutants::trace::Sink::Memory(rust_mutants::trace::MemorySink::unbounded()),
        rust_mutants::trace::TraceContext::Standalone {
            run_id: rust_mutants::id::RunId::try_from("build-cache-test".to_owned())
                .expect("run identity"),
            build_selection: rust_mutants::cargo::BuildConfig::default()
                .selection()
                .digest()
                .clone(),
        },
    )
}

fn cargo_builds(trace: &Recorder) -> usize {
    trace
        .events()
        .iter()
        .filter(|event| {
            matches!(&event.payload,
                rust_mutants::trace::Payload::Note { note } if note.kind == "fixture-cargo-build"
            )
        })
        .count()
}

#[test]
fn an_identical_build_verifies_artifacts_without_starting_cargo() {
    let directory = scratch_target("content-build");
    let root = directory.path().join("source");
    copy_tree(&fixture("fixture-simple"), &root);
    let tc = toolchain(&root);
    let cancel = Cancel::new();
    let trace = build_trace();
    let mut options = rust_mutants::cargo::CompileOptions::new(
        rust_mutants::cargo::BuildDir::new(directory.path().join("target"), Vec::new())
            .rooted(root.clone()),
    );
    options.kind = rust_mutants::cargo::CompileKind::Tests;
    options.locked = true;
    options.offline = true;
    let driver = Driver {
        toolchain: &tc,
        dir: &root,
        cancel: &cancel,
        trace: &trace,
    };
    let first = rust_mutants::cargo::compile(&driver, &options).expect("cold compilation");
    assert_eq!(cargo_builds(&trace), 1);
    let second =
        rust_mutants::cargo::compile(&driver, &options).expect("verified cached compilation");
    assert_eq!(
        cargo_builds(&trace),
        1,
        "a content-addressed hit starts no Cargo process: {:#?}",
        trace
            .events()
            .iter()
            .filter_map(|event| {
                if let rust_mutants::trace::Payload::Note { note } = &event.payload {
                    Some(note)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    );
    let mut reusable = first.units.clone();
    for unit in &mut reusable {
        unit.fresh = true;
    }
    assert_eq!(reusable, second.units);
    assert!(trace.events().iter().any(|event| matches!(&event.payload,
        rust_mutants::trace::Payload::Note { note } if note.kind == "build-cache-hit" && note.detail.len() == 64
    )), "every hit names its input digest");
    let artifact = first
        .messages
        .iter()
        .find_map(|message| {
            if let Message::CompilerArtifact(artifact) = message {
                artifact.executable.as_ref()
            } else {
                None
            }
        })
        .expect("a test executable");
    std::fs::write(artifact, b"corrupt").expect("damage a cached artifact");
    rust_mutants::cargo::compile(&driver, &options).expect("damaged artifacts rebuild");
    assert_eq!(
        cargo_builds(&trace),
        2,
        "artifact digests are verified before reuse"
    );
}

#[test]
fn every_changed_build_input_misses_and_then_reuses_only_its_verified_result() {
    let directory = scratch_target("changed-build");
    let root = directory.path().join("source");
    copy_tree(&fixture("fixture-simple"), &root);
    let tc = toolchain(&root);
    let cancel = Cancel::new();
    let trace = build_trace();
    let mut options = rust_mutants::cargo::CompileOptions::new(
        rust_mutants::cargo::BuildDir::new(directory.path().join("target"), Vec::new())
            .rooted(root.clone()),
    );
    options.kind = rust_mutants::cargo::CompileKind::Tests;
    options.locked = true;
    options.offline = true;
    let driver = Driver {
        toolchain: &tc,
        dir: &root,
        cancel: &cancel,
        trace: &trace,
    };
    let compile_pair = |options: &rust_mutants::cargo::CompileOptions| {
        let before = cargo_builds(&trace);
        rust_mutants::cargo::compile(&driver, options).expect("the changed input compiles");
        assert_eq!(
            cargo_builds(&trace),
            before + 1,
            "a changed source, manifest, lockfile, configuration, selection, flag or environment misses"
        );
        rust_mutants::cargo::compile(&driver, options).expect("its verified result reuses");
        assert_eq!(
            cargo_builds(&trace),
            before + 1,
            "a verified hit starts no Cargo process"
        );
    };
    compile_pair(&options);
    let source = root.join("src/cache_probe.rs");
    std::fs::write(source, "pub const PROBE: u32 = 1;\n").expect("a new source");
    compile_pair(&options);
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest");
    std::fs::write(&manifest, format!("{text}\n[features]\nprobe = []\n"))
        .expect("a changed manifest");
    compile_pair(&options);
    let lock = root.join("Cargo.lock");
    let text = std::fs::read_to_string(&lock).expect("the lockfile");
    std::fs::write(lock, format!("{text}\n")).expect("a changed lockfile");
    compile_pair(&options);
    options.build.features.push("probe".to_owned());
    compile_pair(&options);
    options.build.profile = Some("release".to_owned());
    compile_pair(&options);
    options.build.target = Some(tc.host().to_owned());
    compile_pair(&options);
    options
        .env
        .set("CARGO_ENCODED_RUSTFLAGS", "--cfg\u{1f}cache_probe");
    compile_pair(&options);
    options.env.set("BUILD_PROBE", "changed");
    compile_pair(&options);
    std::fs::create_dir_all(root.join(".cargo")).expect("configuration directory");
    std::fs::write(
        root.join(".cargo/config.toml"),
        "[profile.release]\nopt-level = 1\n",
    )
    .expect("a changed configuration");
    compile_pair(&options);
    let keys: BTreeSet<String> = trace
        .events()
        .iter()
        .filter_map(|event| {
            if let rust_mutants::trace::Payload::Note { note } = &event.payload {
                (note.kind == "build-cache-hit").then(|| note.detail.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(keys.len(), 10, "each complete input set has its own key");
}

#[test]
fn missing_or_malformed_cache_records_cannot_replace_a_verified_compilation() {
    let directory = scratch_target("damaged-record");
    let root = directory.path().join("source");
    copy_tree(&fixture("fixture-simple"), &root);
    let tc = toolchain(&root);
    let cancel = Cancel::new();
    let trace = build_trace();
    let mut options = rust_mutants::cargo::CompileOptions::new(
        rust_mutants::cargo::BuildDir::new(directory.path().join("target"), Vec::new())
            .rooted(root.clone()),
    );
    options.locked = true;
    options.offline = true;
    let driver = Driver {
        toolchain: &tc,
        dir: &root,
        cancel: &cancel,
        trace: &trace,
    };
    rust_mutants::cargo::compile(&driver, &options).expect("initial compilation");
    let record = std::fs::read_dir(options.target_dir.path().join("rust-mutants-compilations"))
        .expect("cache records")
        .next()
        .expect("one record")
        .expect("the record entry")
        .path();
    let bytes = std::fs::read(&record).expect("the complete record");
    let mut partial: serde_json::Value =
        njutest_devkit::strictjson::decode_slice(&bytes).expect("record JSON");
    partial["files"] = serde_json::json!({});
    let malformed = serde_json::to_vec(&partial).expect("incomplete record JSON");
    for payload in [None, Some(b"{".as_slice()), Some(malformed.as_slice())] {
        match payload {
            None => std::fs::remove_file(&record).expect("a missing cache record"),
            Some(payload) => std::fs::write(&record, payload).expect("a malformed cache record"),
        }
        let before = cargo_builds(&trace);
        rust_mutants::cargo::compile(&driver, &options).expect("doubt returns to Cargo");
        assert_eq!(cargo_builds(&trace), before + 1);
        rust_mutants::cargo::compile(&driver, &options).expect("the repaired record verifies");
        assert_eq!(cargo_builds(&trace), before + 1);
    }
}

#[test]
fn an_opaque_graph_or_unlocked_build_always_returns_to_cargo() {
    for name in ["fixture-simple", "fixture-carry"] {
        let directory = scratch_target("unbound-build");
        let root = directory.path().join("source");
        copy_tree(&fixture(name), &root);
        let tc = toolchain(&root);
        let cancel = Cancel::new();
        let trace = build_trace();
        let mut options = rust_mutants::cargo::CompileOptions::new(
            rust_mutants::cargo::BuildDir::new(directory.path().join("target"), Vec::new())
                .rooted(root.clone()),
        );
        options.locked = name != "fixture-simple";
        options.offline = true;
        let driver = Driver {
            toolchain: &tc,
            dir: &root,
            cancel: &cancel,
            trace: &trace,
        };
        for _ in 0..2 {
            rust_mutants::cargo::compile(&driver, &options).expect("conservative Cargo fallback");
        }
        assert_eq!(
            cargo_builds(&trace),
            2,
            "opaque inputs and an unlocked graph cannot claim a hit"
        );
    }
}

#[test]
fn metadata_is_loaded_from_a_workspace_with_the_locked_offline_flags() {
    let dir = fixture("fixture-workspace");
    let tc = toolchain(&dir);
    let options = MetadataOptions {
        locked: true,
        offline: true,
    };
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let driver = Driver {
        toolchain: &tc,
        dir: &dir,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(&driver, options).expect("metadata");
    assert_eq!(metadata.workspace_root, dir);
    let mut members: Vec<&str> = metadata.members().map(|p| p.name.as_str()).collect();
    members.sort_unstable();
    assert_eq!(members, ["fixture-app", "fixture-core"]);
    let app = metadata
        .members()
        .find(|p| p.name == "fixture-app")
        .expect("app");
    assert_eq!(app.manifest_dir(), dir.join("crates/app"));
    let mut targets: Vec<(String, Vec<String>)> = app
        .targets
        .iter()
        .map(|t| (t.name.clone(), t.kind.clone()))
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        [
            ("cli".to_owned(), vec!["test".to_owned()]),
            ("fixture-app".to_owned(), vec!["bin".to_owned()]),
        ]
    );
    for target in metadata.members().flat_map(|p| p.targets.iter()) {
        assert!(
            target.src_path.is_absolute(),
            "{}",
            target.src_path.display()
        );
    }

    let not_a_workspace = tempfile::tempdir().expect("tempdir");
    let error = Metadata::load(
        &Driver {
            toolchain: &tc,
            dir: not_a_workspace.path(),
            cancel: &cancel,
            trace: &trace,
        },
        options,
    );
    assert!(error.is_err(), "a non-workspace has no metadata: {error:?}");
    let Err(error) = error else { return };
    assert_eq!(error.kind(), CargoErrorKind::CommandFailed);
    assert!(error.to_string().contains("RM1014"), "{error}");
    assert!(
        error.to_string().contains("could not find"),
        "the command's own words are kept: {error}"
    );
}
#[test]
fn a_unit_names_every_file_and_variable_the_compiler_read_for_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"reads\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("manifest");
    std::fs::write(root.join("src/greeting.txt"), "hello\n").expect("data");
    std::fs::write(
        root.join("src/lib.rs"),
        "pub const GREETING: &str = include_str!(\"greeting.txt\");\n\
         pub const WHO: Option<&str> = option_env!(\"RUST_MUTANTS_PROBE_UNSET\");\n",
    )
    .expect("source");
    let tc = toolchain(root);
    let target = scratch_target("reads");
    let mut spec = tc.command(
        root,
        ["check", "--lib", "--message-format=json", "--offline"],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the tree writes exact UTF-8")
    );
    let messages = parse_messages(&result.stdout).expect("messages");
    let units = units_of(&messages, root).expect("units");
    let [unit] = units.as_slice() else {
        panic!("one library unit: {units:?}");
    };
    let root = root
        .canonicalize()
        .expect("the tree has a physical spelling");
    let inputs: Vec<String> = unit
        .inputs
        .iter()
        .map(|path| under(&root, &path.canonicalize().expect("an input is a file")))
        .collect();
    assert_eq!(
        inputs,
        ["src/greeting.txt", "src/lib.rs"],
        "an included file is read by the compiler as surely as a compiled one"
    );
    assert_eq!(
        unit.sources
            .iter()
            .map(|path| under(&root, &path.canonicalize().expect("a source")))
            .collect::<Vec<_>>(),
        ["src/lib.rs"],
        "what is compiled stays what is compiled"
    );
    assert_eq!(
        unit.env.get("RUST_MUTANTS_PROBE_UNSET"),
        Some(&None),
        "an unset variable the compiler read is a fact about the build: {:?}",
        unit.env
    );
}

#[test]
fn every_unit_is_read_from_the_dep_info_rustc_wrote_whatever_its_name_begins_with() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for directory in ["src", "tests"] {
        std::fs::create_dir_all(root.join(directory)).expect("mkdir");
    }
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"library\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [[bin]]\nname = \"libretto\"\npath = \"src/main.rs\"\n\n\
         [[test]]\nname = \"libtest-x\"\npath = \"tests/x.rs\"\n",
    )
    .expect("manifest");
    for (file, source) in [
        ("src/lib.rs", "pub fn one() -> u32 { 1 }\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("tests/x.rs", "#[test]\nfn one() {}\n"),
    ] {
        std::fs::write(root.join(file), source).expect("source");
    }
    let tc = toolchain(root);
    let target = scratch_target("library");
    let mut spec = tc.command(
        root,
        [
            "test",
            "--all-targets",
            "--no-run",
            "--message-format=json",
            "--offline",
        ],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the tree writes exact UTF-8")
    );
    let messages = parse_messages(&result.stdout).expect("messages");
    let units = units_of(&messages, root).unwrap_or_else(|error| {
        panic!(
            "a library named `library`, a binary named `libretto` and a test named `libtest-x` \
             are each read from the dep-info rustc wrote for it: {error}"
        )
    });
    let root = root
        .canonicalize()
        .expect("the tree has a physical spelling");
    let read: BTreeSet<(String, bool, Vec<String>)> = units
        .iter()
        .map(|unit| {
            (
                unit.target.name.clone(),
                unit.test,
                unit.sources
                    .iter()
                    .map(|path| under(&root, &path.canonicalize().expect("a source")))
                    .collect(),
            )
        })
        .collect();
    for (name, test, source) in [
        ("library", false, "src/lib.rs"),
        ("library", true, "src/lib.rs"),
        ("libretto", true, "src/main.rs"),
        ("libtest-x", true, "tests/x.rs"),
    ] {
        assert!(
            read.contains(&(name.to_owned(), test, vec![source.to_owned()])),
            "{name} (test: {test}) compiled {source}: {read:?}"
        );
    }
}

#[test]
fn the_test_binaries_are_the_ones_cargo_test_runs_each_read_as_its_manifest_declares_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for directory in ["src", "tests", "examples"] {
        std::fs::create_dir_all(root.join(directory)).expect("mkdir");
    }
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"own-harness\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [lib]\nharness = false\n\n\
         [[bin]]\nname = \"tool\"\npath = \"src/main.rs\"\ntest = false\n\n\
         [[test]]\nname = \"kept\"\npath = \"tests/kept.rs\"\n\n\
         [[example]]\nname = \"demo\"\npath = \"examples/demo.rs\"\n",
    )
    .expect("manifest");
    for (file, source) in [
        (
            "src/lib.rs",
            "pub fn above(n: u32) -> bool { n > 10 }\npub fn main() { assert!(above(11)); }\n",
        ),
        ("src/main.rs", "fn main() {}\n"),
        (
            "tests/kept.rs",
            "#[test]\nfn eleven() { assert!(own_harness::above(11)); }\n",
        ),
        ("examples/demo.rs", "fn main() {}\n"),
    ] {
        std::fs::write(root.join(file), source).expect("source");
    }
    let tc = toolchain(root);
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let driver = Driver {
        toolchain: &tc,
        dir: root,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: false,
            offline: true,
        },
    )
    .expect("metadata");
    let target = scratch_target("own-harness");
    let built = rust_mutants::execute::build(
        &driver,
        &metadata.packages,
        &rust_mutants::execute::BuildOptions {
            target_dir: rust_mutants::cargo::BuildDir::new(target.path().to_path_buf(), Vec::new()),
            locked: false,
            offline: true,
            packages: Vec::new(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("the test binaries build");
    let read: Vec<(&str, bool)> = built
        .iter()
        .map(|target| (target.id(), target.harness))
        .collect();
    assert_eq!(
        read,
        [
            ("own-harness/lib/own_harness", false),
            ("own-harness/test/kept", true)
        ],
        "`cargo test --all-targets` builds the binary and the example that `test = false` leaves \
         out of `cargo test` as test binaries too, and the library's unnamed `[lib]` table says it \
         is its own program rather than libtest"
    );
}

#[test]
fn units_from_a_check_name_exactly_the_files_each_unit_compiled() {
    let dir = fixture("fixture-simple");
    let tc = toolchain(&dir);
    let target = scratch_target("simple");
    let mut spec = tc.command(
        &dir,
        [
            "check",
            "--workspace",
            "--all-targets",
            "--message-format=json",
            "--offline",
            "--locked",
        ],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the fixture writes exact UTF-8")
    );
    let messages = parse_messages(&result.stdout).expect("messages");
    let units = units_of(&messages, &dir).expect("units");
    let mut described: Vec<(String, Vec<String>, bool, Vec<String>)> = units
        .iter()
        .map(|unit| {
            (
                unit.target.name.clone(),
                unit.target.kind.clone(),
                unit.test,
                unit.sources.iter().map(|p| under(&dir, p)).collect(),
            )
        })
        .collect();
    described.sort();
    assert_eq!(
        described,
        [
            (
                "fixture_simple".to_owned(),
                vec!["lib".to_owned()],
                false,
                vec!["src/lib.rs".to_owned()]
            ),
            (
                "fixture_simple".to_owned(),
                vec!["lib".to_owned()],
                true,
                vec!["src/lib.rs".to_owned(), "src/testutil.rs".to_owned()]
            ),
            (
                "parity".to_owned(),
                vec!["test".to_owned()],
                true,
                vec!["tests/parity.rs".to_owned()]
            ),
        ]
    );
    for unit in &units {
        assert!(unit.sources.iter().all(|path| {
            path.is_absolute()
                && matches!(std::fs::metadata(path), Ok(metadata) if metadata.is_file())
        }));
    }
}
#[test]
fn units_of_a_nested_member_resolve_against_the_workspace_root() {
    let dir = fixture("fixture-workspace");
    let tc = toolchain(&dir);
    let target = scratch_target("workspace");
    let mut spec = tc.command(
        &dir,
        [
            "check",
            "--workspace",
            "--all-targets",
            "--message-format=json",
            "--offline",
            "--locked",
        ],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the fixture writes exact UTF-8")
    );
    let units = units_of(&parse_messages(&result.stdout).expect("messages"), &dir).expect("units");
    let core: BTreeSet<String> = units
        .iter()
        .filter(|u| u.target.name == "fixture_core" && !u.test)
        .flat_map(|u| u.sources.iter())
        .map(|p| under(&dir, p))
        .collect();
    assert_eq!(
        core,
        ["crates/core/src/lib.rs", "crates/core/src/util.rs"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let app_test = units
        .iter()
        .find(|u| u.target.name == "cli")
        .expect("integration test unit");
    assert_eq!(app_test.sources, [dir.join("crates/app/tests/cli.rs")]);
    assert!(app_test.target.is_test());
}
#[test]
fn a_check_that_fails_to_compile_still_yields_its_messages() {
    let dir = fixture("fixture-simple");
    let tc = toolchain(&dir);
    let target = scratch_target("broken");
    let snapshot = tempfile::tempdir().expect("tempdir");
    let copy = snapshot.path().join("fixture-simple");
    copy_tree(&dir, &copy);
    std::fs::write(
        copy.join("src/lib.rs"),
        "pub fn f() -> i32 { let s = String::new(); s - \"x\" }\n",
    )
    .expect("break");
    let mut spec = tc.command(
        &copy,
        ["check", "--message-format=json", "--offline", "--locked"],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.path().into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(!result.succeeded());
    let messages = parse_messages(&result.stdout).expect("messages");
    let errors: Vec<&Diagnostic> = messages
        .iter()
        .filter_map(|m| match m {
            Message::CompilerMessage(m) if m.message.is_error() => Some(&m.message),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 1, "{messages:?}");
    assert_eq!(errors[0].code.as_deref(), Some("E0369"));
    assert!(
        rust_mutants::cargo::names_file(
            &errors[0].primary_span().expect("primary").file_name,
            "src/lib.rs"
        ),
        "the compiler names the file it was given with its own separator, and what the \
         engine matches a catalog against is the rule that reads either spelling: {:?}",
        errors[0].primary_span().expect("primary").file_name
    );
    assert!(matches!(
        messages.last(),
        Some(Message::BuildFinished(finished))
            if *finished == rust_mutants::cargo::Finished::new(false)
    ));
}

/// What fixture-carry's build script emitted when checked in `copy`, by package.
fn emitted_in(copy: &Path, target: &Path) -> Vec<Vec<rust_mutants::cargo::Emitted>> {
    let tc = toolchain(copy);
    let mut spec = tc.command(
        copy,
        [
            "check",
            "--lib",
            "--message-format=json",
            "--offline",
            "--locked",
        ],
    );
    spec.argv.push("--target-dir".into());
    spec.argv.push(target.into());
    spec.structured_stdout = Some(64 << 20);
    let result = run_build(&spec);
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the fixture writes exact UTF-8")
    );
    let messages = parse_messages(&result.stdout).expect("messages");
    let compile_time = compile_time_inputs(&messages, copy).expect("compile-time inputs");
    let copy = copy
        .canonicalize()
        .expect("the copy has a physical spelling");
    assert!(
        compile_time.iter().any(|path| path
            .canonicalize()
            .is_ok_and(|path| path == copy.join("build.rs"))),
        "the build script's own source is read for the build: {compile_time:?}"
    );
    emitted_of(&messages).into_values().collect()
}

#[test]
fn what_a_build_script_emitted_is_kept_for_the_package_it_builds_for() {
    let snapshot = tempfile::tempdir().expect("tempdir");
    let copy = snapshot.path().join("fixture-carry");
    copy_tree(&fixture("fixture-carry"), &copy);
    let target = scratch_target("emitted");
    let before = emitted_in(&copy, target.path());
    let [told] = before.as_slice() else {
        panic!("one package ran a build script: {before:?}");
    };
    let [told] = told.as_slice() else {
        panic!("it ran once: {told:?}");
    };
    assert!(
        told.out_dir.is_some() && told.cfgs.is_empty(),
        "the directory it wrote into, and no configuration yet: {told:?}"
    );
    std::fs::write(copy.join("waive"), "").expect("waive the limit");
    let after = emitted_in(&copy, target.path());
    assert_eq!(
        after
            .concat()
            .into_iter()
            .map(|told| told.cfgs)
            .collect::<Vec<_>>(),
        [vec!["waived".to_owned()]],
        "a configuration the build script sets is in no dep-info, and changes what compiles"
    );
}
