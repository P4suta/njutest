// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opening a tree: what the engine asks the toolchain, and what it says when the answer is not one it can use.

use std::path::PathBuf;

use njutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants::session::PrepareOptions;
use rust_mutants::workspace::{OpenOptions, Workspace};

const CARGO_BANNER: &str = "cargo 1.98.0 (abc 2026-08-05)\nrelease: 1.98.0\ncommit-hash: abc\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\n";
const RUSTC_BANNER: &str = "rustc 1.98.0 (abc 2026-08-05)\nbinary: rustc\nrelease: 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 20.1.0\n";

/// The three commands every open makes before it reads the metadata.
fn toolchain_answers() -> Script {
    Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing(CARGO_BANNER))
        .answering(Invocation::new("rustc", &["-vV"]).printing(RUSTC_BANNER))
        .answering(
            Invocation::new("rustc", &["--print", "sysroot"]).printing("/nonexistent/sysroot\n"),
        )
}

/// Opens `fixture` against `script`, handing back the programs so the caller keeps them alive: dropping an [`Installed`] takes the fake with it, and a later command would find no cargo.
fn opened(
    fixture: &Fixture,
    script: &Script,
) -> (Result<Workspace, rust_mutants::EngineError>, Installed) {
    let installed = install(script);
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = installed.env();
    env.push((
        std::ffi::OsString::from("PATH"),
        std::ffi::OsString::from(installed.bin()),
    ));
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(std::ffi::OsString::from(installed.bin())),
            temp_directory: fixture.temp().to_path_buf(),
            env,
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    (opened, installed)
}

#[test]
fn opening_reports_what_cargo_metadata_said_when_it_fails() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).failing(
        101,
        "error: failed to load manifest for workspace member `demo`\n",
    ));
    let error = opened(&fixture, &script).0.expect_err("metadata fails");
    assert_eq!(error.code().code, "RM1014", "{error}");
    assert!(
        error.to_string().contains("failed to load manifest"),
        "cargo's own words come back: {error}"
    );
}

#[test]
fn opening_refuses_a_cargo_that_is_not_a_file() {
    let fixture = Fixture::copy("fixture-simple");
    let error = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(PathBuf::from("/nonexistent/cargo")),
            temp_directory: fixture.temp().to_path_buf(),
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect_err("no such cargo");
    assert_eq!(error.code().code, "RM1012", "{error}");
}

#[test]
fn opening_refuses_a_banner_without_a_release_line() {
    let fixture = Fixture::copy("fixture-simple");
    let script = Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing("cargo 1.98.0\nhost: x86_64\n"));
    let error = opened(&fixture, &script)
        .0
        .expect_err("a banner with no release");
    assert_eq!(error.code().code, "RM1013", "{error}");
}

#[test]
fn opening_refuses_metadata_that_is_not_its_document() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(Invocation::new("cargo", &["metadata"]).printing("{\"not\": \"metadata\"}\n"));
    let error = opened(&fixture, &script)
        .0
        .expect_err("metadata that is not one");
    assert_eq!(error.code().code, "RM1015", "{error}");
}

#[test]
fn opening_copies_the_tree_and_asks_the_toolchain_in_the_copy() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers().answering(
        Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
    );
    let installed = install(&script);
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = installed.env();
    env.push((
        std::ffi::OsString::from("PATH"),
        std::ffi::OsString::from(installed.bin()),
    ));
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(std::ffi::OsString::from(installed.bin())),
            temp_directory: fixture.temp().to_path_buf(),
            env,
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("the workspace opens");

    assert_ne!(
        workspace.snapshot_root(),
        fixture.root(),
        "the tree the engine works in is the copy"
    );
    assert!(workspace.snapshot_root().join("src/lib.rs").is_file());
    assert!(
        workspace.snapshot_root().starts_with(fixture.temp()),
        "and it lives under the temporary directory it was given"
    );
    assert_eq!(
        workspace.workspace_digest().len(),
        64,
        "a tree that was copied has an identity"
    );
    assert_eq!(
        installed.answered(),
        vec![0, 1, 2, 3, 3],
        "one banner each, the sysroot, then the metadata of the tree on disk — which is what \
         says whether a copy of it could build at all — and the metadata of the copy"
    );
}

#[test]
fn opening_a_member_directory_names_the_workspace_root_and_the_flag() {
    let fixture = Fixture::copy("fixture-simple");
    let elsewhere = fixture.temp().join("the-workspace");
    let document = metadata_document(fixture.root()).replace(
        &format!(
            "\"workspace_root\":\"{}\"",
            njutest_devkit::paths::in_json(fixture.root())
        ),
        &format!(
            "\"workspace_root\":\"{}\"",
            njutest_devkit::paths::in_json(&elsewhere)
        ),
    );
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let (opened, _installed) = opened(&fixture, &script);
    let error = opened.expect_err("a member is not a workspace");
    let said = error.to_string();
    assert!(said.contains("RM1018"), "{said}");
    assert!(
        said.contains(&elsewhere.display().to_string()),
        "the refusal names the workspace the member belongs to: {said}"
    );
    assert!(
        said.contains("--root"),
        "and the flag that answers it: {said}"
    );
}

#[test]
fn a_path_dependency_outside_the_root_is_named_before_any_copy() {
    let fixture = Fixture::copy("fixture-simple");
    let document = metadata_document(fixture.root()).replace(
        "\"targets\":[",
        "\"dependencies\":[{\"name\":\"outside\",\"kind\":null,\"path\":\"../../../elsewhere\"}],\"targets\":[",
    );
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let (opened, _installed) = opened(&fixture, &script);
    let error = opened.expect_err("a tree that reads from outside itself");
    let said = error.to_string();
    assert!(said.contains("RM1017"), "{said}");
    assert!(said.contains("outside"), "{said}");
    assert!(
        said.contains("--allow-outside"),
        "the refusal says what allows it: {said}"
    );
}

#[test]
fn an_allowed_directory_outside_the_root_is_read_rather_than_refused() {
    let fixture = Fixture::copy("fixture-simple");
    let allowed = fixture.temp().join("elsewhere");
    std::fs::create_dir_all(allowed.join("src")).expect("the directory beside the tree");
    std::fs::write(
        allowed.join("Cargo.toml"),
        "[package]\nname = \"outside\"\n",
    )
    .expect("its manifest");
    std::fs::write(allowed.join("src/lib.rs"), "pub fn f() {}\n").expect("its source");
    let document = metadata_document(fixture.root()).replace(
        "\"targets\":[",
        &format!(
            "\"dependencies\":[{{\"name\":\"outside\",\"kind\":null,\"path\":\"{}\"}}],\"targets\":[",
            njutest_devkit::paths::in_json(&allowed)
        ),
    );
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let installed = install(&script);
    let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = installed.env();
    env.push((
        std::ffi::OsString::from("PATH"),
        std::ffi::OsString::from(installed.bin()),
    ));
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(std::ffi::OsString::from(installed.bin())),
            temp_directory: fixture.temp().to_path_buf(),
            env,
            locked: true,
            offline: true,
            allow_outside: vec![allowed],
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    let workspace = opened.expect("a directory somebody named is one the run may read");
    let beside = workspace
        .snapshot_root()
        .parent()
        .expect("the snapshot directory")
        .join("elsewhere");
    assert!(
        beside.is_dir(),
        "and one the run copies beside the tree, so the same relative path resolves in the \
         copy: {}",
        beside.display()
    );
}

/// A `cargo metadata` document for a one-package workspace at `root`.
fn metadata_document(root: &std::path::Path) -> String {
    let manifest = root.join("Cargo.toml");
    let source = root.join("src/lib.rs");
    format!(
        r#"{{"packages":[{{"name":"demo","version":"0.1.0","id":"demo 0.1.0","manifest_path":"{manifest}","targets":[{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"{source}","edition":"2024","test":true,"doctest":true,"harness":true}}]}}],"workspace_members":["demo 0.1.0"],"workspace_root":"{root}","target_directory":"{target}","version":1,"resolve":null}}"#,
        manifest = njutest_devkit::paths::in_json(&manifest),
        source = njutest_devkit::paths::in_json(&source),
        root = njutest_devkit::paths::in_json(root),
        target = njutest_devkit::paths::in_json(&root.join("target")),
    )
}

#[test]
fn preparing_refuses_a_tree_that_does_not_compile_before_anything_is_instrumented() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(
            Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
        )
        .answering(
            Invocation::new("cargo", &["check"])
                .printing(&format!(
                    "{}\n{}\n",
                    compiler_message(fixture.root(), "expected one of `;`"),
                    r#"{"reason":"build-finished","success":false}"#
                ))
                .failing(101, ""),
        );
    let (opened, _installed) = opened(&fixture, &script);
    let workspace = opened.expect("the workspace opens");
    let error = workspace
        .prepare(&PrepareOptions::default(), &Cancel::new())
        .expect_err("a tree that does not compile");
    assert_eq!(error.code().code, "RM5001", "{error}");
    assert!(
        error.to_string().contains("expected one of"),
        "the compiler's first error is what a reader gets: {error}"
    );
}

#[test]
fn a_message_stream_line_that_is_not_a_message_is_refused_by_line_number() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(
            Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
        )
        .answering(
            Invocation::new("cargo", &["check"]).printing(
                "{\"reason\":\"build-finished\",\"success\":true}\nthis is not a message\n",
            ),
        );
    let (opened, _installed) = opened(&fixture, &script);
    let workspace = opened.expect("the workspace opens");
    let error = workspace
        .prepare(&PrepareOptions::default(), &Cancel::new())
        .expect_err("a stream that is not messages");
    assert_eq!(error.code().code, "RM1016", "{error}");
    assert!(
        error.to_string().contains("line 2"),
        "the line a reader would go to: {error}"
    );
}

#[test]
fn a_check_that_takes_longer_than_the_build_timeout_says_it_timed_out() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(
            Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
        )
        .answering(Invocation::new("cargo", &["check"]).taking(5_000));
    let (opened, _installed) = opened(&fixture, &script);
    let workspace = opened.expect("the workspace opens");
    let options = PrepareOptions {
        build_timeout: Some(std::time::Duration::from_millis(200)),
        ..PrepareOptions::default()
    };
    let started = std::time::Instant::now();
    let error = workspace
        .prepare(&options, &Cancel::new())
        .expect_err("a build that runs long");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the timeout is what ended it, not the command"
    );
    assert_eq!(error.code().code, "RM1014", "{error}");
    assert!(error.to_string().contains("timed out"), "{error}");
}

/// A `compiler-message` a check would print for a tree that does not parse.
fn compiler_message(root: &std::path::Path, said: &str) -> String {
    let source = root.join("src/lib.rs");
    format!(
        r#"{{"reason":"compiler-message","package_id":"demo 0.1.0","manifest_path":"{manifest}","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"{source}","edition":"2024"}},"message":{{"message":"{said}","code":null,"level":"error","spans":[],"children":[],"rendered":"error: {said}\n"}}}}"#,
        manifest = njutest_devkit::paths::in_json(&root.join("Cargo.toml")),
        source = njutest_devkit::paths::in_json(&source),
    )
}

#[test]
fn a_compile_stopped_by_cancellation_is_an_error_not_a_failed_build() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(
            Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
        )
        .answering(Invocation::new("cargo", &["check"]).taking(30_000));
    let (opened, _installed) = opened(&fixture, &script);
    let workspace = opened.expect("open");

    let cancel = Cancel::new();
    let waiting = cancel.clone();
    let stopping = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        waiting.cancel();
    });
    let error = rust_mutants::cargo::compile(
        &rust_mutants::testkit::workspace::driver(&workspace, &cancel),
        &rust_mutants::cargo::CompileOptions {
            kind: rust_mutants::cargo::CompileKind::Check,
            locked: true,
            offline: true,
            ..rust_mutants::cargo::CompileOptions::default()
        },
    )
    .expect_err("a cancelled compilation does not answer");
    stopping.join().expect("the canceller");

    assert_eq!(
        error.kind(),
        rust_mutants::cargo::CargoErrorKind::Cancelled,
        "a build nobody waited for printed nothing about the tree, and reading its silence as \
         a failed build condemns mutants the compiler never saw"
    );
    assert_eq!(error.kind().code().code, "RM0001");
    workspace.close().expect("close");
}

/// A test process binding a Unix socket under the directory it runs in pays for every byte of that directory's path.
#[test]
fn what_a_run_adds_leaves_a_test_room_to_bind_a_socket() {
    const SUN_PATH: usize = 104;
    let parent = PathBuf::from("/var/folders/q9/8kq0lqv91bd3z5wz_0000gn/T");
    let scratch = rust_mutants::workspace::scratch_of(&parent, 7);
    let socket = scratch.join("7").join(".tmpAbCdEf").join("service.sock");
    assert!(
        socket.as_os_str().len() <= SUN_PATH,
        "a run leaves a test {} bytes of the {SUN_PATH} a Unix socket has: {}",
        SUN_PATH.saturating_sub(scratch.as_os_str().len()),
        socket.display()
    );
}
