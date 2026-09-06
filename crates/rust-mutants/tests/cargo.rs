// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cargo boundary: locating the toolchain, reading `cargo metadata`, parsing `--message-format=json`, and reading dep-info to learn which files a unit really compiled. The end-to-end tests drive the cargo that built this test binary against the fixtures, offline.

#![expect(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use rust_mutants::cargo::{
    CargoError, CargoErrorKind, Diagnostic, LocateOptions, Message, Metadata, Toolchain,
    dep_info_path, parse_dep_info, parse_messages, parse_version,
};
use rust_mutants::runner::Cancel;

#[test]
fn verbose_version_output_is_parsed_into_its_fields() {
    let cargo = "cargo 1.98.1 (797e8a9bc 2026-08-05)\nrelease: 1.98.1\ncommit-hash: 797e8a9bca276c1c9f9f738d2a20f484fa4eea9d\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\nlibgit2: 1.9.4 (sys:0.21.0 vendored)\nos: Linux Mint 22.3.0 (zena) [64-bit]\n";
    let parsed = parse_version(cargo).expect("cargo");
    assert_eq!(parsed.release, "1.98.1");
    assert_eq!(
        parsed.commit_hash.as_deref(),
        Some("797e8a9bca276c1c9f9f738d2a20f484fa4eea9d")
    );
    assert_eq!(parsed.commit_date.as_deref(), Some("2026-08-05"));
    assert_eq!(parsed.host, "x86_64-unknown-linux-gnu");
    assert_eq!(parsed.llvm_version, None);
    assert_eq!(parsed.summary, "cargo 1.98.1 (797e8a9bc 2026-08-05)");

    let rustc = "rustc 1.98.1 (48a229cea 2026-09-01)\nbinary: rustc\ncommit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\ncommit-date: 2026-09-01\nhost: x86_64-unknown-linux-gnu\nrelease: 1.98.1\nLLVM version: 22.1.8\n";
    let parsed = parse_version(rustc).expect("rustc");
    assert_eq!(parsed.release, "1.98.1");
    assert_eq!(parsed.llvm_version.as_deref(), Some("22.1.8"));

    let nightly = "rustc 1.100.0-nightly (abcdef012 2026-09-01)\nbinary: rustc\ncommit-hash: unknown\ncommit-date: unknown\nhost: aarch64-apple-darwin\nrelease: 1.100.0-nightly\nLLVM version: 23.0.0\n";
    let parsed = parse_version(nightly).expect("nightly");
    assert_eq!(parsed.commit_hash, None, "unknown is absent, not a hash");
    assert!(parsed.is_nightly());
    assert_eq!(parsed.host, "aarch64-apple-darwin");
}

#[test]
fn version_output_without_release_or_host_is_refused() {
    for bad in [
        "",
        "cargo 1.98.1\n",
        "release: 1.98.1\n",
        "host: x\n",
        "garbage\nrelease: 1\n",
    ] {
        let error = parse_version(bad).unwrap_err();
        assert_eq!(error.kind(), CargoErrorKind::VersionUnreadable, "{bad:?}");
    }
}

#[test]
fn a_cargo_that_cannot_run_is_a_typed_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let broken = temp.path().join("cargo");
    std::fs::write(&broken, "not a program").expect("write");
    let options = LocateOptions {
        cargo: Some(broken),
        ..LocateOptions::default()
    };
    let error = Toolchain::locate(&options, temp.path(), &Cancel::new()).unwrap_err();
    assert!(
        matches!(
            error.kind(),
            CargoErrorKind::ToolchainNotFound | CargoErrorKind::CommandFailed
        ),
        "{error}"
    );
}

#[test]
fn metadata_json_is_parsed_into_packages_and_targets() {
    let json = r#"{
      "packages": [{
        "name": "demo", "version": "0.1.0", "id": "path+file:///w/demo#0.1.0",
        "manifest_path": "/w/demo/Cargo.toml", "edition": "2024",
        "targets": [
          {"kind": ["lib"], "crate_types": ["lib"], "name": "demo", "src_path": "/w/demo/src/lib.rs", "edition": "2024", "doc": true, "doctest": true, "test": true},
          {"kind": ["proc-macro"], "crate_types": ["proc-macro"], "name": "demo_macros", "src_path": "/w/demo/macros.rs", "edition": "2021", "doctest": false, "test": false, "harness": false},
          {"kind": ["custom-build"], "crate_types": ["bin"], "name": "build-script-build", "src_path": "/w/demo/build.rs", "edition": "2024", "doctest": false, "test": false}
        ],
        "features": {}, "dependencies": [], "extra": "ignored"
      }],
      "workspace_members": ["path+file:///w/demo#0.1.0"],
      "workspace_default_members": ["path+file:///w/demo#0.1.0"],
      "resolve": null,
      "target_directory": "/w/target",
      "version": 1,
      "workspace_root": "/w",
      "metadata": null
    }"#;
    let metadata = Metadata::parse(json.as_bytes()).expect("parse");
    assert_eq!(metadata.workspace_root, Path::new("/w"));
    assert_eq!(metadata.target_directory, Path::new("/w/target"));
    let members: Vec<&str> = metadata.members().map(|p| p.name.as_str()).collect();
    assert_eq!(members, ["demo"]);
    let demo = &metadata.packages[0];
    assert_eq!(demo.manifest_dir(), Path::new("/w/demo"));
    assert_eq!(demo.edition, "2024");
    let kinds: Vec<(&str, bool, bool, bool)> = demo
        .targets
        .iter()
        .map(|t| {
            (
                t.name.as_str(),
                t.is_proc_macro(),
                t.is_custom_build(),
                t.harness,
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("demo", false, false, true),
            ("demo_macros", true, false, false),
            ("build-script-build", false, true, true),
        ]
    );
    assert!(demo.targets[0].is_lib());
    assert!(demo.targets[0].test);

    let error = Metadata::parse(b"{\"version\": 1}").unwrap_err();
    assert_eq!(error.kind(), CargoErrorKind::MetadataUnparsable);
    let error = Metadata::parse(b"not json").unwrap_err();
    assert_eq!(error.kind(), CargoErrorKind::MetadataUnparsable);
}

const fn sample_stream() -> &'static str {
    concat!(
        r#"{"reason":"compiler-artifact","package_id":"path+file:///w/demo#0.1.0","manifest_path":"/w/demo/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/demo/src/lib.rs","edition":"2024","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":true},"features":[],"filenames":["/w/target/debug/deps/libdemo-abc.rmeta"],"executable":null,"fresh":false}"#,
        "\n",
        r#"{"reason":"compiler-message","package_id":"path+file:///w/demo#0.1.0","manifest_path":"/w/demo/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/demo/src/lib.rs","edition":"2024","doc":true,"doctest":true,"test":true},"message":{"$message_type":"diagnostic","message":"cannot subtract `&str` from `String`","code":{"code":"E0369","explanation":"..."},"level":"error","spans":[{"file_name":"src/lib.rs","byte_start":67,"byte_end":68,"line_start":3,"line_end":3,"column_start":14,"column_end":15,"is_primary":false,"text":[],"label":"String","suggested_replacement":null,"suggestion_applicability":null,"expansion":null},{"file_name":"src/lib.rs","byte_start":69,"byte_end":70,"line_start":3,"line_end":3,"column_start":16,"column_end":17,"is_primary":true,"text":[],"label":null,"suggested_replacement":null,"suggestion_applicability":null,"expansion":null}],"children":[{"message":"`String` does not implement `Sub<&str>`","code":null,"level":"note","spans":[],"children":[],"rendered":null}],"rendered":"error[E0369]: cannot subtract `&str` from `String`\n"}}"#,
        "\n",
        "\n",
        r#"{"reason":"build-script-executed","package_id":"x","linked_libs":[],"linked_paths":[],"cfgs":[],"env":[],"out_dir":"/o"}"#,
        "\n",
        r#"{"reason":"something-new","payload":1}"#,
        "\n",
        r#"{"reason":"build-finished","success":false}"#,
        "\n",
    )
}

#[test]
fn message_lines_are_typed_and_a_line_that_is_not_one_is_refused() {
    let messages = parse_messages(sample_stream().as_bytes()).expect("parse");
    assert_eq!(messages.len(), 5);
    let artifact = artifact_of(&messages[0]);
    assert_eq!(artifact.target.name, "demo");
    assert!(artifact.profile.test);
    assert_eq!(
        artifact.filenames,
        [PathBuf::from("/w/target/debug/deps/libdemo-abc.rmeta")]
    );
    assert_eq!(artifact.executable, None);
    let diagnostic = diagnostic_of(&messages[1]);
    assert_eq!(diagnostic.level, "error");
    assert_eq!(diagnostic.code.as_deref(), Some("E0369"));
    assert!(diagnostic.is_error());
    let primary = diagnostic.primary_span().expect("primary");
    assert_eq!(
        (
            primary.file_name.as_str(),
            primary.byte_start,
            primary.byte_end
        ),
        ("src/lib.rs", 69, 70)
    );
    assert_eq!(diagnostic.children[0].level, "note");
    assert!(
        diagnostic
            .rendered
            .as_deref()
            .unwrap()
            .starts_with("error[E0369]")
    );
}

#[test]
fn other_message_kinds_are_typed_and_a_line_that_is_not_one_is_refused() {
    let messages = parse_messages(sample_stream().as_bytes()).expect("parse");
    assert!(matches!(&messages[2], Message::BuildScriptExecuted));
    assert!(matches!(&messages[3], Message::Other { reason } if reason == "something-new"));
    assert!(matches!(
        &messages[4],
        Message::BuildFinished { success: false }
    ));

    let error =
        parse_messages(b"{\"reason\":\"build-finished\",\"success\":true}\nCompiling demo\n")
            .unwrap_err();
    assert_eq!(error.kind(), CargoErrorKind::MessageUnparsable);
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn dep_info_lists_the_prerequisites_of_the_first_rule_with_escapes_undone() {
    let text = "/t/deps/demo-abc.d: src/lib.rs src/with\\ space.rs \\\n  crates/x/src/mod.rs\n\n/t/deps/libdemo-abc.rmeta: src/lib.rs\n\nsrc/lib.rs:\n";
    assert_eq!(
        parse_dep_info(text).expect("parse"),
        ["src/lib.rs", "src/with space.rs", "crates/x/src/mod.rs"]
    );
    assert_eq!(
        parse_dep_info("out.d: \\\n\n").expect("empty"),
        Vec::<String>::new()
    );
    let error = parse_dep_info("no rule here\n").unwrap_err();
    assert_eq!(error.kind(), CargoErrorKind::DepInfoUnreadable);
    let error = parse_dep_info("").unwrap_err();
    assert_eq!(error.kind(), CargoErrorKind::DepInfoUnreadable);
}

#[test]
fn the_dep_info_file_sits_beside_the_artifact_without_the_lib_prefix() {
    assert_eq!(
        dep_info_path(Path::new("/t/debug/deps/libdemo-abc.rmeta")),
        Some(PathBuf::from("/t/debug/deps/demo-abc.d"))
    );
    assert_eq!(
        dep_info_path(Path::new("/t/debug/deps/demo_bin-abc.rmeta")),
        Some(PathBuf::from("/t/debug/deps/demo_bin-abc.d"))
    );
    assert_eq!(
        dep_info_path(Path::new("/t/debug/deps/libdemo-abc.rlib")),
        Some(PathBuf::from("/t/debug/deps/demo-abc.d"))
    );
    assert_eq!(
        dep_info_path(Path::new("/t/debug/deps/demo-abc")),
        Some(PathBuf::from("/t/debug/deps/demo-abc.d"))
    );
    assert_eq!(dep_info_path(Path::new("/")), None);
}

fn artifact_of(message: &Message) -> &rust_mutants::cargo::Artifact {
    match message {
        Message::CompilerArtifact(artifact) => artifact,
        other => panic!("not an artifact: {other:?}"),
    }
}

fn diagnostic_of(message: &Message) -> &Diagnostic {
    match message {
        Message::CompilerMessage(message) => &message.message,
        other => panic!("not a compiler message: {other:?}"),
    }
}

#[test]
fn every_cargo_error_kind_has_a_code_in_the_workspace_area() {
    for kind in CargoErrorKind::ALL {
        let code = kind.code().code;
        assert!(code.starts_with("RM1") || code.starts_with("RM2"), "{code}");
    }
    let error: CargoError = parse_dep_info("").unwrap_err();
    assert_eq!(error.kind().code().code, "RM2001");
}
