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
    Toolchain, UnitInputs, parse_messages, resolve_executable, unit_inputs_of, units_of,
};
use rust_mutants::runner::{Cancel, run};
use rust_mutants::trace::Recorder;

fn fixture(name: &str) -> PathBuf {
    njutest_devkit::paths::fixtures_dir().join(name)
}

fn toolchain(dir: &Path) -> Toolchain {
    let options = LocateOptions {
        cargo: Some(njutest_devkit::paths::cargo_binary()),
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
    let result = run(&spec, &Cancel::new());
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
    let result = run(&spec, &Cancel::new());
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
    let result = run(&spec, &Cancel::new());
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
        Some(Message::BuildFinished { success: false })
    ));
}

/// Every unit of a checked fixture and what each read, with the directories its paths are under.
fn checked_units(name: &str) -> (PathBuf, tempfile::TempDir, Vec<UnitInputs>) {
    let dir = fixture(name);
    let tc = toolchain(&dir);
    let target = scratch_target(name);
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
    let result = run(&spec, &Cancel::new());
    assert!(
        result.succeeded(),
        "{}",
        std::str::from_utf8(&result.output).expect("the fixture writes exact UTF-8")
    );
    let messages = parse_messages(&result.stdout).expect("messages");
    let units = unit_inputs_of(&messages, &dir).expect("the units and what each read");
    (dir, target, units)
}

/// A file a unit read, relative to the fixture, or by name under the target directory.
fn read_as(dir: &Path, target: &Path, path: &Path) -> String {
    match (path.strip_prefix(dir), path.strip_prefix(target)) {
        (Ok(relative), _) => relative.to_str().expect("exact UTF-8").replace('\\', "/"),
        (Err(_outside), Ok(generated)) => format!(
            "$target/{}",
            generated
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .expect("a generated file has a UTF-8 name")
        ),
        (Err(_outside), Err(_elsewhere)) => format!("elsewhere:{}", path.display()),
    }
}

/// One unit as a reader checks it: which target, whether it is the test build, and what it read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Read {
    target: String,
    kind: Vec<String>,
    test: bool,
    files: Vec<String>,
    env: Vec<String>,
}

impl Read {
    fn of((target, kind, test): (&str, &str, bool), files: &[&str], env: &[&str]) -> Self {
        Self {
            target: target.to_owned(),
            kind: vec![kind.to_owned()],
            test,
            files: files.iter().map(|file| (*file).to_owned()).collect(),
            env: env.iter().map(|name| (*name).to_owned()).collect(),
        }
    }
}

#[test]
fn every_unit_names_each_file_it_read_whatever_its_kind_and_each_variable_it_asked_for() {
    let (dir, target, units) = checked_units("fixture-carry");
    let mut described: Vec<Read> = units
        .iter()
        .map(|unit| {
            let mut files: Vec<String> = unit
                .inputs
                .files
                .iter()
                .map(|path| read_as(&dir, target.path(), path))
                .collect();
            files.sort();
            Read {
                target: unit.target.name.clone(),
                kind: unit.target.kind.clone(),
                test: unit.test,
                files,
                env: unit
                    .inputs
                    .env
                    .iter()
                    .map(|read| read.name.clone())
                    .collect(),
            }
        })
        .collect();
    described.sort();
    let library = ["$target/limit.rs", "src/answer.txt", "src/lib.rs"];
    assert_eq!(
        described,
        [
            Read::of(
                ("build-script-build", "custom-build", false),
                &["build.rs"],
                &[]
            ),
            Read::of(("fixture_carry", "lib", false), &library, &["OUT_DIR"]),
            Read::of(("fixture_carry", "lib", true), &library, &["OUT_DIR"]),
        ],
        "a unit is keyed on everything its own compilation read: the build script is a \
         unit of its own, and a library reads a text file and a generated file as surely \
         as it reads its Rust"
    );
    for unit in &units {
        let told = unit.inputs.emitted.len();
        let expected = usize::from(!unit.target.is_custom_build());
        assert_eq!(
            told, expected,
            "{}: every unit of the package is compiled with what its build script emitted, \
             and the build script itself with none of it",
            unit.target.name
        );
    }
}
