// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opening a tree: what the engine asks the toolchain, and what it says when the answer is not one it can use.
//!
//! Every command here is scripted, so what fails is the engine's reading of a
//! toolchain rather than a toolchain.

use std::path::PathBuf;

use mjutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use mjutest_devkit::fixture::Fixture;
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
        vec![0, 1, 2, 3],
        "one banner each, the sysroot, and the metadata, in that order"
    );
}

/// A `cargo metadata` document for a one-package workspace at `root`.
fn metadata_document(root: &std::path::Path) -> String {
    let manifest = root.join("Cargo.toml");
    let source = root.join("src/lib.rs");
    format!(
        r#"{{"packages":[{{"name":"demo","version":"0.1.0","id":"demo 0.1.0","manifest_path":"{manifest}","targets":[{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"{source}","edition":"2024","test":true,"doctest":true,"harness":true}}]}}],"workspace_members":["demo 0.1.0"],"workspace_root":"{root}","target_directory":"{root}/target","version":1,"resolve":null}}"#,
        manifest = manifest.display(),
        source = source.display(),
        root = root.display(),
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
        manifest = root.join("Cargo.toml").display(),
        source = source.display(),
    )
}
