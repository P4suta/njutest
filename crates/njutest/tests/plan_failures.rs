// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A plan's failure boundaries and the scratch directory used by its one build.

#![expect(
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::disallowed_methods,
    reason = "test fixtures report setup failures by panicking, and long scenario tables keep one protocol assertion together"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::build::{
    BUILD_OUTPUT_LIMIT, BuildError, BuildOptions, Cargo, Flavour, Selection, build,
};
use njutest::cli::Environment;
use njutest::trace::{Clock, MemorySink, Payload, Recorder, Sink, StartRecord};
use njutest::watch::Watch;
use njutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use njutest_devkit::repo::Repo;
use rust_mutants::cargo::{LocateOptions, Metadata, Toolchain};
use rust_mutants::runner::Cancel;

const CARGO_BANNER: &str = "cargo 1.98.0 (abc 2026-08-05)\nrelease: 1.98.0\ncommit-hash: abc\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\n";
const RUSTC_BANNER: &str = "rustc 1.98.0 (abc 2026-08-05)\nbinary: rustc\nrelease: 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 20.1.0\n";

struct Said {
    code: u8,
    out: String,
    err: String,
}

fn project() -> Repo {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn answer() -> u8 { 42 }\n");
    repo
}

fn environment(
    root: &Path,
    temp: PathBuf,
    path: OsString,
    vars: Vec<(OsString, OsString)>,
) -> Environment {
    let mut vars = vars;
    vars.push((OsString::from("PATH"), path));
    Environment {
        cache_directory: root.join("cache"),
        working_directory: root.to_path_buf(),
        temp_directory: temp,
        program: PathBuf::from(env!("CARGO_BIN_EXE_njutest")),
        vars,
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn fake_environment(repo: &Repo, installed: &Installed) -> Environment {
    environment(
        repo.root(),
        repo.root().join("temp"),
        installed.bin().as_os_str().to_owned(),
        installed.env(),
    )
}

fn asked(environment: &Environment, extra: &[&str]) -> Said {
    let mut args = vec!["njutest", "plan", "--offline", "--locked"];
    args.extend(extra.iter().copied());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        args.into_iter().map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    Said {
        code,
        out: njutest_devkit::process::strict_utf8(&out).into_owned(),
        err: njutest_devkit::process::strict_utf8(&err).into_owned(),
    }
}

/// The identity cargo gives the one package these tests are about, spelled the way cargo spells one.
fn package_id(root: &Path) -> String {
    njutest_devkit::cargo_double::package_id(root, "demo", "0.1.0")
}

/// The identity cargo gives a package of `name` whose manifest sits in `directory`.
fn package_id_of(directory: &Path, name: &str) -> String {
    njutest_devkit::cargo_double::package_id(directory, name, "0.1.0")
}

/// The synthetic root the compiler messages below are about.
fn fixture_root() -> &'static Path {
    Path::new("/fixture")
}

fn metadata(root: &Path) -> String {
    let package_id = package_id(fixture_root());
    serde_json::json!({
        "version": 1,
        "workspace_root": root,
        "target_directory": root.join("target"),
        "workspace_members": [package_id],
        "workspace_default_members": [package_id],
        "packages": [{
            "id": package_id,
            "name": "demo",
            "version": "0.1.0",
            "manifest_path": root.join("Cargo.toml"),
            "edition": "2024",
            "targets": [{
                "name": "demo",
                "kind": ["lib"],
                "crate_types": ["lib"],
                "src_path": root.join("src/lib.rs"),
                "edition": "2024",
                "test": true,
                "doctest": false,
                "harness": true
            }],
            "dependencies": []
        }],
        "resolve": null
    })
    .to_string()
}

fn through_metadata(root: &Path, answer: Invocation) -> Script {
    Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
        .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
        .answering(
            Invocation::new("rustc", &["--print", "sysroot"]).printing("/not-a-real-sysroot\n"),
        )
        .answering(answer)
        .answering(
            Invocation::new("cargo", &["metadata", "--format-version", "1"])
                .printing(&metadata(root)),
        )
}

fn successful_build(executable: Option<&str>) -> String {
    let mut lines = Vec::new();
    if let Some(executable) = executable {
        lines.push(
            serde_json::json!({
                "reason": "compiler-artifact",
                "package_id": package_id(fixture_root()),
                "target": {
                    "name": "demo",
                    "kind": ["lib"],
                    "crate_types": ["lib"],
                    "src_path": "/fixture/src/lib.rs",
                    "edition": "2024",
                    "test": true,
                    "doctest": false,
                    "harness": true
                },
                "profile": { "test": true },
                "filenames": [executable],
                "executable": executable,
                "fresh": false
            })
            .to_string(),
        );
    }
    lines.push(serde_json::json!({ "reason": "build-finished", "success": true }).to_string());
    format!("{}\n", lines.join("\n"))
}

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn located(root: &Path, installed: &Installed) -> Toolchain {
    Toolchain::locate(
        &LocateOptions {
            cargo: Some(installed.cargo()),
            search_path: None,
            env: Some(installed.env()),
        },
        root,
        &Cancel::new(),
    )
    .expect("the scripted toolchain")
}

fn packages(root: &Path) -> Vec<rust_mutants::cargo::Package> {
    njutest_devkit::strictjson::decode_str::<Metadata>(&metadata(root))
        .expect("the test metadata")
        .packages
}

fn options(
    root: &Path,
    target: PathBuf,
    scratch: PathBuf,
    env: Vec<(OsString, OsString)>,
) -> BuildOptions {
    BuildOptions {
        root: root.to_path_buf(),
        selection: Selection::default(),
        flavour: Flavour::Native,
        target_dir: target,
        scratch_build_dir: scratch,
        env,
        cargo: Cargo {
            offline: true,
            locked: true,
        },
        timeout: None,
    }
}

#[test]
fn an_invalid_configuration_is_an_error_and_not_a_panic() {
    let repo = project();
    repo.write(".njutest.toml", "version = 1\nunknown = true\n");
    let empty = repo.root().join("empty-path");
    std::fs::create_dir_all(&empty).expect("an empty path");
    let environment = environment(
        repo.root(),
        repo.root().join("temp"),
        empty.into_os_string(),
        Vec::new(),
    );

    let said = asked(&environment, &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("NJ1002"), "{}", said.err);
}

#[test]
fn a_missing_toolchain_is_an_error_and_not_a_panic() {
    let repo = project();
    let empty = repo.root().join("empty-path");
    std::fs::create_dir_all(&empty).expect("an empty path");
    let environment = environment(
        repo.root(),
        repo.root().join("temp"),
        empty.into_os_string(),
        Vec::new(),
    );

    let said = asked(&environment, &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("cargo"), "{}", said.err);
    assert!(said.err.contains("search path"), "{}", said.err);
}

#[test]
fn metadata_failure_is_an_error_and_not_a_panic() {
    let repo = project();
    let script = Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
        .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
        .answering(
            Invocation::new("rustc", &["--print", "sysroot"]).printing("/not-a-real-sysroot\n"),
        )
        .answering(
            Invocation::new("cargo", &["metadata", "--format-version", "1"])
                .failing(7, "metadata refused\n"),
        );
    let installed = install(&script);

    let said = asked(&fake_environment(&repo, &installed), &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("metadata refused"), "{}", said.err);
}

#[test]
fn unreadable_build_output_is_an_error_and_not_a_panic() {
    let repo = project();
    let build = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing("not a cargo message\n");
    let installed = install(&through_metadata(repo.root(), build));

    let said = asked(&fake_environment(&repo, &installed), &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("NJ3003"), "{}", said.err);
    assert!(said.err.contains("not a cargo message"), "{}", said.err);
}

#[test]
fn an_unusable_scratch_parent_is_an_error() {
    let repo = project();
    let script = through_metadata(repo.root(), Invocation::new("unused", &[]));
    let installed = install(&script);
    let occupied = repo.root().join("not-a-directory");
    std::fs::write(&occupied, "occupied").expect("a file");
    let environment = environment(
        repo.root(),
        occupied,
        installed.bin().as_os_str().to_owned(),
        installed.env(),
    );

    let said = asked(&environment, &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("NJ8001"), "{}", said.err);
}

#[test]
fn a_plan_builds_in_its_dedicated_scratch_target_and_raii_removes_it() {
    let repo = project();
    let marker = repo.root().join("built-target-dir");
    let build = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&successful_build(None))
    .writing(
        marker.to_str().expect("test protocol paths are UTF-8"),
        "{{target_dir}}",
    );
    let installed = install(&through_metadata(repo.root(), build));
    let environment = fake_environment(&repo, &installed);

    let said = asked(&environment, &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ASSURED,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.out.contains("TARGETS\t0"), "{}", said.out);
    let target = PathBuf::from(std::fs::read_to_string(&marker).expect("the target directory"));
    assert_eq!(
        target.file_name().and_then(std::ffi::OsStr::to_str),
        Some("layer")
    );
    let scratch = target.parent().expect("the run scratch");
    assert!(
        scratch
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name.starts_with(njutest::scratch::DIR_PREFIX)),
        "{}",
        scratch.display()
    );
    assert!(
        target.starts_with(&environment.temp_directory),
        "{}",
        target.display()
    );
    assert!(!scratch.exists(), "RAII removes {}", scratch.display());
}

#[test]
fn target_enumeration_failure_is_an_error_and_not_a_panic() {
    let repo = project();
    let missing = repo.root().join("missing-test-binary");
    let build = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&successful_build(Some(
        missing.to_str().expect("test protocol paths are UTF-8"),
    )));
    let installed = install(&through_metadata(repo.root(), build));

    let said = asked(&fake_environment(&repo, &installed), &[]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ERROR,
        "{}{}",
        said.out,
        said.err
    );
    assert!(said.err.contains("NJ3001"), "{}", said.err);
    assert!(said.err.contains("cannot start"), "{}", said.err);
}

#[test]
fn a_plan_counts_started_and_ignored_tests_separately() {
    let repo = project();
    let suite = format!("{{{{bin_json}}}}/{}", executable("suite"));
    let build = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&successful_build(Some(&suite)));
    let script = through_metadata(repo.root(), build)
        .answering(
            Invocation::new("suite", &["--list", "--format", "terse"])
                .printing("runs::one: test\nruns::two: test\nskips::one: test\n"),
        )
        .answering(
            Invocation::new("suite", &["--list", "--ignored", "--format", "terse"])
                .printing("skips::one: test\n"),
        );
    let installed = install(&script);

    let said = asked(&fake_environment(&repo, &installed), &["--why"]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ASSURED,
        "{}{}",
        said.out,
        said.err
    );
    assert!(
        said.out.contains("3 tests, 1 of them ignored"),
        "{}",
        said.out
    );
    assert!(said.out.contains("TARGETS\t1"), "{}", said.out);
}

#[test]
fn plan_why_names_packages_that_came_from_the_configuration() {
    let repo = project();
    repo.write(
        ".njutest.toml",
        "version = 1\n\n[project]\npackages = [\"demo\"]\n",
    );
    let build = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--package",
            "demo",
            "--target-dir",
        ],
    )
    .printing(&successful_build(None));
    let installed = install(&through_metadata(repo.root(), build));

    let said = asked(&fake_environment(&repo, &installed), &["--why"]);

    assert_eq!(
        said.code,
        njutest::cli::EXIT_ASSURED,
        "{}{}",
        said.out,
        said.err
    );
    assert!(
        said.out
            .contains("SCOPE\tthe packages the configuration names: demo"),
        "{}",
        said.out
    );
}

#[test]
fn build_defaults_command_environment_limit_and_trace_are_exact() {
    assert_eq!(
        Selection::default(),
        Selection {
            packages: Vec::new(),
            features: Vec::new(),
            all_features: false,
            default_features: true,
            skip_targets: Vec::new(),
        }
    );
    assert_eq!(BUILD_OUTPUT_LIMIT, 67_108_864);

    let repo = project();
    let target = repo.root().join("target-layer");
    let scratch = repo.root().join("scratch-layer");
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--locked",
            "--offline",
            "--package",
            "alpha",
            "--package",
            "beta",
            "--all-features",
            "--no-default-features",
            "--features",
            "one,two",
            "--target-dir",
        ],
    )
    .when(
        "CARGO_TARGET_DIR",
        scratch.to_str().expect("test protocol paths are UTF-8"),
    )
    .when_set(&["CARGO_ENCODED_RUSTFLAGS", "LLVM_PROFILE_FILE"])
    .printing(&successful_build(None));
    let installed = install(&through_metadata(repo.root(), invocation));
    let toolchain = located(repo.root(), &installed);
    let mut env = installed.env();
    env.extend([
        (OsString::from("CARGO_TARGET_DIR"), OsString::from("old")),
        (OsString::from("RUSTFLAGS"), OsString::from("-Dwarnings")),
        (OsString::from("KEEP"), OsString::from("yes")),
    ]);
    let mut build_options = options(repo.root(), target.clone(), scratch.clone(), env);
    build_options.selection = Selection {
        packages: vec!["alpha".to_owned(), "beta".to_owned()],
        features: vec!["one".to_owned(), "two".to_owned()],
        all_features: true,
        default_features: false,
        skip_targets: Vec::new(),
    };
    build_options.flavour = Flavour::Coverage;
    build_options.timeout = Some(std::time::Duration::from_millis(4321));
    let trace = Recorder::new(
        Sink::Memory(MemorySink::unbounded()),
        Clock::Wall,
        StartRecord::of(
            "run",
            njutest::report::RunKind::Full,
            njutest::config::Contract::StandardV1,
        ),
    );

    let built = build(
        &toolchain,
        &packages(repo.root()),
        &build_options,
        Watch::new(&Cancel::new(), &trace),
    )
    .expect("the exact invocation matched");

    assert_eq!(installed.answered(), [0, 1, 2, 3]);
    let events = trace.events();
    let exec = events
        .iter()
        .find_map(|event| njutest::testkit::payload::of(&event.payload).exec())
        .expect("the build execution is recorded");
    assert_eq!(
        exec.argv,
        [
            installed
                .cargo()
                .to_str()
                .expect("test protocol paths are UTF-8")
                .to_owned(),
            "test".to_owned(),
            "--no-run".to_owned(),
            "--message-format=json".to_owned(),
            "--target".to_owned(),
            "x86_64-unknown-linux-gnu".to_owned(),
            "--locked".to_owned(),
            "--offline".to_owned(),
            "--package".to_owned(),
            "alpha".to_owned(),
            "--package".to_owned(),
            "beta".to_owned(),
            "--all-features".to_owned(),
            "--no-default-features".to_owned(),
            "--features".to_owned(),
            "one,two".to_owned(),
            "--target-dir".to_owned(),
            target
                .to_str()
                .expect("test protocol paths are UTF-8")
                .to_owned(),
        ]
    );
    assert_eq!(
        exec.dir.as_deref(),
        Some(repo.root().to_str().expect("test protocol paths are UTF-8"))
    );
    assert_eq!(exec.timeout_ms, Some(4321));
    let value = |name: &str| {
        built
            .env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| {
                value
                    .to_str()
                    .expect("test protocol paths are UTF-8")
                    .to_owned()
            })
    };
    assert_eq!(
        value("CARGO_TARGET_DIR"),
        Some(scratch.display().to_string())
    );
    assert_eq!(value("KEEP").as_deref(), Some("yes"));
    assert!(value("RUSTFLAGS").is_none());
    assert!(
        value("CARGO_ENCODED_RUSTFLAGS").is_some_and(
            |flags| flags.contains("-Dwarnings") && flags.contains("instrument-coverage")
        )
    );
    assert_eq!(
        value("LLVM_PROFILE_FILE"),
        Some(
            scratch
                .join(njutest::build::BUILD_PROFILES)
                .join("%p-%m.profraw")
                .display()
                .to_string()
        )
    );
}

#[test]
fn a_native_build_preserves_plain_flags_and_uses_the_workspace_default_selection() {
    let repo = project();
    let target = repo.root().join("native-target");
    let scratch = repo.root().join("native-scratch");
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .when(
        "CARGO_TARGET_DIR",
        scratch.to_str().expect("test protocol paths are UTF-8"),
    )
    .when("RUSTFLAGS", "--cfg native")
    .printing(&successful_build(None));
    let installed = install(&through_metadata(repo.root(), invocation));
    let toolchain = located(repo.root(), &installed);
    let mut env = installed.env();
    env.push((OsString::from("RUSTFLAGS"), OsString::from("--cfg native")));
    let built = build(
        &toolchain,
        &packages(repo.root()),
        &options(repo.root(), target, scratch, env),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("native build");
    assert!(built.limitations.is_empty());
    assert!(built.env.iter().any(|(key, value)| {
        key == "RUSTFLAGS" && value == std::ffi::OsStr::new("--cfg native")
    }));
    assert!(built.env.iter().all(|(key, _)| key != "LLVM_PROFILE_FILE"));
}

#[test]
fn a_build_timeout_is_a_not_run_error_and_is_recorded() {
    let repo = project();
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .taking(250)
    .printing(&successful_build(None));
    let installed = install(&through_metadata(repo.root(), invocation));
    let toolchain = located(repo.root(), &installed);
    let mut build_options = options(
        repo.root(),
        repo.root().join("target"),
        repo.root().join("scratch"),
        installed.env(),
    );
    build_options.timeout = Some(std::time::Duration::from_millis(10));
    let trace = Recorder::new(
        Sink::Memory(MemorySink::unbounded()),
        Clock::Wall,
        StartRecord::of(
            "run",
            njutest::report::RunKind::Full,
            njutest::config::Contract::StandardV1,
        ),
    );
    let error = build(
        &toolchain,
        &packages(repo.root()),
        &build_options,
        Watch::new(&Cancel::new(), &trace),
    )
    .expect_err("the bounded command did not finish");
    assert!(matches!(error, BuildError::NotRun { .. }), "{error}");
    assert!(
        trace.events().iter().any(|event| matches!(
            &event.payload,
            Payload::Exec { exec }
                if matches!(exec.stopped, rust_mutants::execute::Stopped::TimedOut { .. })
        )),
        "that this machine stopped waiting is durable trace evidence, and the record says \
         which of the ways a process can end it was rather than a status beside a flag"
    );
}

fn target_document(
    root: &Path,
    name: &str,
    kind: &[&str],
    crate_types: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "kind": kind,
        "crate_types": crate_types,
        "src_path": root.join("src").join(format!("{name}.rs")),
        "edition": "2024",
        "test": true,
        "doctest": false,
        "harness": true,
    })
}

fn artifact(
    root: &Path,
    package_id: &str,
    name: &str,
    kind: &[&str],
    crate_types: &[&str],
    file: &Path,
) -> serde_json::Value {
    serde_json::json!({
        "reason": "compiler-artifact",
        "package_id": package_id,
        "target": target_document(root, name, kind, crate_types),
        "profile": { "test": true },
        "filenames": [file],
        "executable": file,
        "fresh": false,
    })
}

fn stream(messages: &[serde_json::Value]) -> String {
    let mut text = messages
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

#[test]
fn compiler_failures_prefer_each_rendered_error_and_have_a_nonempty_fallback() {
    let repo = project();
    let target = target_document(repo.root(), "demo", &["lib"], &["lib"]);
    let messages = stream(&[
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": package_id(fixture_root()),
            "target": target,
            "message": {
                "message": "first fallback",
                "level": "error",
                "rendered": "  rendered first\n",
            },
        }),
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": package_id(fixture_root()),
            "target": target_document(repo.root(), "demo", &["lib"], &["lib"]),
            "message": {
                "message": "second fallback",
                "level": "error",
                "rendered": "   ",
            },
        }),
        serde_json::json!({
            "reason": "compiler-message",
            "package_id": package_id(fixture_root()),
            "target": target_document(repo.root(), "demo", &["lib"], &["lib"]),
            "message": { "message": "not an error", "level": "warning" },
        }),
        serde_json::json!({ "reason": "build-finished", "success": false }),
    ]);
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&messages)
    .failing(101, "");
    let installed = install(&through_metadata(repo.root(), invocation));
    let built = build(
        &located(repo.root(), &installed),
        &packages(repo.root()),
        &options(
            repo.root(),
            repo.root().join("target"),
            repo.root().join("scratch"),
            installed.env(),
        ),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("an unsuccessful compilation is a result, not a runner failure");
    assert_eq!(
        built.failure.as_deref(),
        Some("rendered first\nsecond fallback")
    );

    let no_diagnostic = stream(&[serde_json::json!({
        "reason": "build-finished",
        "success": false
    })]);
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&no_diagnostic)
    .failing(101, "");
    let installed = install(&through_metadata(repo.root(), invocation));
    let built = build(
        &located(repo.root(), &installed),
        &packages(repo.root()),
        &options(
            repo.root(),
            repo.root().join("target-two"),
            repo.root().join("scratch-two"),
            installed.env(),
        ),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("an unsuccessful compilation is still readable");
    assert!(
        built
            .failure
            .as_deref()
            .is_some_and(|failure| !failure.trim().is_empty()),
        "a build failure never becomes an empty diagnostic"
    );
}

#[test]
fn library_sources_are_only_workspace_library_inputs_and_dep_info_failure_is_an_error() {
    let repo = project();
    repo.write("src/z.rs", "pub fn z() {}\n");
    repo.write("src/main.rs", "fn main() {}\n");
    repo.write("src/proc.rs", "pub fn proc_item() {}\n");
    repo.write("src/foreign.rs", "pub fn foreign() {}\n");
    repo.write("README.md", "not Rust\n");
    let deps = repo.root().join("target/deps");
    let library = deps.join("libdemo.rlib");
    let binary = deps.join("demo-bin");
    let proc_macro = deps.join("libdemo_proc.so");
    let foreign = deps.join("libforeign.rlib");
    let outside = repo
        .root()
        .parent()
        .expect("repository parent")
        .join("outside.rs");
    let messages = stream(&[
        artifact(
            repo.root(),
            &package_id(fixture_root()),
            "demo",
            &["lib"],
            &["lib"],
            &library,
        ),
        artifact(
            repo.root(),
            &package_id(fixture_root()),
            "demo-bin",
            &["bin"],
            &["bin"],
            &binary,
        ),
        artifact(
            repo.root(),
            &package_id(fixture_root()),
            "demo-proc",
            &["proc-macro"],
            &["proc-macro"],
            &proc_macro,
        ),
        artifact(
            repo.root(),
            &package_id_of(Path::new("/foreign"), "foreign"),
            "foreign",
            &["lib"],
            &["lib"],
            &foreign,
        ),
        serde_json::json!({ "reason": "build-finished", "success": true }),
    ]);
    let dep_line = format!(
        "{}: {} {} {} {} {}\n",
        library.display(),
        repo.root().join("src/z.rs").display(),
        repo.root().join("src/lib.rs").display(),
        repo.root().join("src/z.rs").display(),
        outside.display(),
        repo.root().join("README.md").display(),
    );
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .writing(
        deps.join("demo.d")
            .to_str()
            .expect("test protocol paths are UTF-8"),
        &dep_line,
    )
    .writing(
        deps.join("demo-bin.d")
            .to_str()
            .expect("test protocol paths are UTF-8"),
        &format!(
            "{}: {}\n",
            binary.display(),
            repo.root().join("src/main.rs").display()
        ),
    )
    .writing(
        deps.join("demo_proc.d")
            .to_str()
            .expect("test protocol paths are UTF-8"),
        &format!(
            "{}: {}\n",
            proc_macro.display(),
            repo.root().join("src/proc.rs").display()
        ),
    )
    .writing(
        deps.join("foreign.d")
            .to_str()
            .expect("test protocol paths are UTF-8"),
        &format!(
            "{}: {}\n",
            foreign.display(),
            repo.root().join("src/foreign.rs").display()
        ),
    )
    .printing(&messages);
    let installed = install(&through_metadata(repo.root(), invocation));
    let built = build(
        &located(repo.root(), &installed),
        &packages(repo.root()),
        &options(
            repo.root(),
            repo.root().join("target-layer"),
            repo.root().join("scratch"),
            installed.env(),
        ),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("every artifact has readable dep-info");
    assert_eq!(
        built.library_sources.get("demo"),
        Some(&vec![
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/z.rs")
        ])
    );
    assert_eq!(built.library_sources.len(), 1);

    let missing = deps.join("libmissing.rlib");
    let messages = stream(&[
        artifact(
            repo.root(),
            &package_id(fixture_root()),
            "missing",
            &["lib"],
            &["lib"],
            &missing,
        ),
        serde_json::json!({ "reason": "build-finished", "success": true }),
    ]);
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .printing(&messages);
    let installed = install(&through_metadata(repo.root(), invocation));
    let error = build(
        &located(repo.root(), &installed),
        &packages(repo.root()),
        &options(
            repo.root(),
            repo.root().join("target-missing"),
            repo.root().join("scratch-missing"),
            installed.env(),
        ),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect_err("missing dep-info makes the build evidence unreadable");
    assert!(matches!(error, BuildError::Unreadable { .. }), "{error}");
    assert!(error.to_string().contains("dep-info"), "{error}");
}

#[test]
fn each_unit_gets_cargos_environment_over_the_parent_environment() {
    let repo = project();
    let deps = repo.root().join("target/deps");
    let library = deps.join("libdemo.rlib");
    let out_dir = repo.root().join("generated");
    let messages = stream(&[
        serde_json::json!({
            "reason": "build-script-executed",
            "package_id": package_id(fixture_root()),
            "out_dir": out_dir,
            "env": [["DUPLICATE", "child"], ["ONLY_CHILD", "yes"]],
        }),
        artifact(
            repo.root(),
            &package_id(fixture_root()),
            "demo",
            &["lib"],
            &["lib"],
            &library,
        ),
        serde_json::json!({ "reason": "build-finished", "success": true }),
    ]);
    let invocation = Invocation::new(
        "cargo",
        &[
            "test",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--workspace",
            "--target-dir",
        ],
    )
    .writing(
        deps.join("demo.d")
            .to_str()
            .expect("test protocol paths are UTF-8"),
        &format!(
            "{}: {}\n",
            library.display(),
            repo.root().join("src/lib.rs").display()
        ),
    )
    .printing(&messages);
    let installed = install(&through_metadata(repo.root(), invocation));
    let toolchain = located(repo.root(), &installed);
    let scratch = repo.root().join("scratch");
    let mut env = installed.env();
    env.extend([
        (OsString::from("DUPLICATE"), OsString::from("parent")),
        (OsString::from("ONLY_PARENT"), OsString::from("yes")),
        (OsString::from("CARGO"), OsString::from("wrong-cargo")),
    ]);
    let built = build(
        &toolchain,
        &packages(repo.root()),
        &options(
            repo.root(),
            repo.root().join("target-layer"),
            scratch.clone(),
            env,
        ),
        Watch::new(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("built");
    let unit = built.units.first().expect("the library test unit");
    let values = |name: &str| {
        unit.env
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| {
                value
                    .to_str()
                    .expect("test protocol paths are UTF-8")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(values("DUPLICATE"), ["child"]);
    assert_eq!(values("ONLY_CHILD"), ["yes"]);
    assert_eq!(values("ONLY_PARENT"), ["yes"]);
    assert_eq!(values("OUT_DIR"), [out_dir.display().to_string()]);
    assert_eq!(values("CARGO"), [installed.cargo().display().to_string()]);
    assert_eq!(values("CARGO_TARGET_DIR"), [scratch.display().to_string()]);
}
