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
    let result = run(&spec, &Cancel::new());
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
    let result = run(&spec, &Cancel::new());
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
