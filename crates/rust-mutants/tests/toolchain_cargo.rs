// SPDX-FileCopyrightText: 2026 mjutest contributors
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

use mjutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    CargoErrorKind, Diagnostic, Driver, LocateOptions, Message, Metadata, MetadataOptions,
    Toolchain, parse_messages, resolve_executable, units_of,
};
use rust_mutants::runner::{Cancel, run};
use rust_mutants::trace::Recorder;

fn fixture(name: &str) -> PathBuf {
    mjutest_devkit::paths::fixtures_dir().join(name)
}

fn toolchain(dir: &Path) -> Toolchain {
    let options = LocateOptions {
        cargo: Some(mjutest_devkit::paths::cargo_binary()),
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
#[test]
fn an_explicit_cargo_path_must_exist_and_a_bare_name_is_searched_on_the_given_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = resolve_executable(Path::new("/definitely/not/cargo"), None).unwrap_err();
    assert_eq!(missing.kind(), CargoErrorKind::ToolchainNotFound);
    assert!(missing.to_string().contains("RM1012"), "{missing}");

    let real = mjutest_devkit::paths::cargo_binary();
    assert_eq!(resolve_executable(&real, None).expect("exists"), real);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let fake = bin.join("cargo");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").expect("write");
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut search = OsString::from(temp.path().join("empty"));
        search.push(":");
        search.push(&bin);
        assert_eq!(
            resolve_executable(Path::new("cargo"), Some(search.as_os_str())).expect("found"),
            fake
        );
        let none = resolve_executable(Path::new("cargo"), Some(OsString::from("").as_os_str()))
            .unwrap_err();
        assert_eq!(none.kind(), CargoErrorKind::ToolchainNotFound);
    }
    let bare_without_path = resolve_executable(Path::new("no-such-tool-xyz"), None).unwrap_err();
    assert_eq!(bare_without_path.kind(), CargoErrorKind::ToolchainNotFound);
}
#[test]
fn locating_reads_both_versions_from_inside_the_directory() {
    let dir = fixture("fixture-simple");
    let tc = toolchain(&dir);
    assert_eq!(tc.cargo(), mjutest_devkit::paths::cargo_binary());
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
    )
    .unwrap_err();
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
    assert!(result.ok(), "{}", String::from_utf8_lossy(&result.output));
    let messages = parse_messages(&result.stdout).expect("messages");
    let units = units_of(&messages, &dir).expect("units");
    let mut described: Vec<(String, Vec<String>, bool, Vec<String>)> = units
        .iter()
        .map(|unit| {
            (
                unit.target.name.clone(),
                unit.target.kind.clone(),
                unit.test,
                unit.sources
                    .iter()
                    .map(|p| {
                        p.strip_prefix(&dir)
                            .expect("under the root")
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect(),
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
        assert!(unit.sources.iter().all(|p| p.is_absolute() && p.is_file()));
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
    assert!(result.ok(), "{}", String::from_utf8_lossy(&result.output));
    let units = units_of(&parse_messages(&result.stdout).expect("messages"), &dir).expect("units");
    let core: BTreeSet<String> = units
        .iter()
        .filter(|u| u.target.name == "fixture_core" && !u.test)
        .flat_map(|u| u.sources.iter())
        .map(|p| {
            p.strip_prefix(&dir)
                .expect("under the root")
                .to_string_lossy()
                .into_owned()
        })
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
    assert!(!result.ok());
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
    assert_eq!(
        errors[0].primary_span().expect("primary").file_name,
        "src/lib.rs"
    );
    assert!(matches!(
        messages.last(),
        Some(Message::BuildFinished { success: false })
    ));
}
