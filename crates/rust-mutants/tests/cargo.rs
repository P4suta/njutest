// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cargo boundary: locating the toolchain, reading `cargo metadata`, parsing `--message-format=json`, and reading dep-info to learn which files a unit really compiled.
//! The end-to-end tests drive the cargo that built this test binary against the fixtures, offline.

#![expect(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use njutest_devkit::result::{
    OptionState::Present,
    ResultState::{Refused, Returned},
    option_state, result_state,
};
use rust_mutants::cargo::{
    BuildConfig, CargoError, CargoErrorKind, CompileKind, CompileOptions, Diagnostic,
    LocateOptions, Message, Metadata, Toolchain, compile_arguments, dep_info_path, env_deps,
    parse_dep_info, parse_messages, parse_version, resolve_executable,
};
use rust_mutants::runner::Cancel;

#[test]
fn verbose_version_output_is_parsed_into_its_fields() {
    let cargo = "cargo 1.98.1 (797e8a9bc 2026-08-05)\nrelease: 1.98.1\ncommit-hash: 797e8a9bca276c1c9f9f738d2a20f484fa4eea9d\ncommit-date: 2026-08-05\nhost: x86_64-unknown-linux-gnu\nlibgit2: 1.9.4 (sys:0.21.0 vendored)\nos: Linux Mint 22.3.0 (zena) [64-bit]\n";
    let parsed = parse_version(cargo);
    assert_eq!(result_state(&parsed), Returned, "cargo version: {parsed:?}");
    let Ok(parsed) = parsed else { return };
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
    let parsed = parse_version(rustc);
    assert_eq!(result_state(&parsed), Returned, "rustc version: {parsed:?}");
    let Ok(parsed) = parsed else { return };
    assert_eq!(parsed.release, "1.98.1");
    assert_eq!(parsed.llvm_version.as_deref(), Some("22.1.8"));

    let nightly = "rustc 1.100.0-nightly (abcdef012 2026-09-01)\nbinary: rustc\ncommit-hash: unknown\ncommit-date: unknown\nhost: aarch64-apple-darwin\nrelease: 1.100.0-nightly\nLLVM version: 23.0.0\n";
    let parsed = parse_version(nightly);
    assert_eq!(
        result_state(&parsed),
        Returned,
        "nightly version: {parsed:?}"
    );
    let Ok(parsed) = parsed else { return };
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
        let error = parse_version(bad);
        assert_eq!(result_state(&error), Refused, "{bad:?}: {error:?}");
        let Err(error) = error else { continue };
        assert_eq!(error.kind(), CargoErrorKind::VersionUnreadable, "{bad:?}");
    }
}

#[test]
fn a_cargo_that_cannot_run_is_a_typed_error() {
    let temp = tempfile::tempdir();
    assert_eq!(result_state(&temp), Returned, "tempdir: {temp:?}");
    let Ok(temp) = temp else { return };
    let broken = temp.path().join("cargo");
    let written = std::fs::write(&broken, "not a program");
    assert_eq!(
        result_state(&written),
        Returned,
        "write {broken:?}: {written:?}"
    );
    let options = LocateOptions {
        cargo: Some(broken),
        ..LocateOptions::default()
    };
    let error = Toolchain::locate(&options, temp.path(), &Cancel::new());
    assert_eq!(result_state(&error), Refused, "broken cargo: {error:?}");
    let Err(error) = error else { return };
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
    let metadata = Metadata::parse(json.as_bytes());
    assert_eq!(result_state(&metadata), Returned, "metadata: {metadata:?}");
    let Ok(metadata) = metadata else { return };
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

    let error = Metadata::parse(b"{\"version\": 1}");
    assert_eq!(
        result_state(&error),
        Refused,
        "incomplete metadata: {error:?}"
    );
    let Err(error) = error else { return };
    assert_eq!(error.kind(), CargoErrorKind::MetadataUnparsable);
    let error = Metadata::parse(b"not json");
    assert_eq!(
        result_state(&error),
        Refused,
        "malformed metadata: {error:?}"
    );
    let Err(error) = error else { return };
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
    let messages = parse_messages(sample_stream().as_bytes());
    assert_eq!(result_state(&messages), Returned, "messages: {messages:?}");
    let Ok(messages) = messages else { return };
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
    let primary = diagnostic.primary_span();
    assert_eq!(option_state(primary), Present, "primary span");
    let Some(primary) = primary else { return };
    assert_eq!(
        (
            primary.file_name.as_str(),
            primary.byte_start,
            primary.byte_end
        ),
        ("src/lib.rs", 69, 70)
    );
    assert_eq!(diagnostic.children[0].level, "note");
    let rendered = diagnostic.rendered.as_deref();
    assert!(rendered.is_some(), "rendered diagnostic");
    let Some(rendered) = rendered else { return };
    assert!(rendered.starts_with("error[E0369]"));
}

#[test]
fn other_message_kinds_are_typed_and_a_line_that_is_not_one_is_refused() {
    let messages = parse_messages(sample_stream().as_bytes());
    assert_eq!(result_state(&messages), Returned, "messages: {messages:?}");
    let Ok(messages) = messages else { return };
    assert!(matches!(&messages[2], Message::BuildScriptExecuted(_)));
    assert!(matches!(&messages[3], Message::Other { reason } if reason == "something-new"));
    assert!(matches!(
        &messages[4],
        Message::BuildFinished { success: false }
    ));

    let error =
        parse_messages(b"{\"reason\":\"build-finished\",\"success\":true}\nCompiling demo\n");
    assert_eq!(result_state(&error), Refused, "mixed stream: {error:?}");
    let Err(error) = error else { return };
    assert_eq!(error.kind(), CargoErrorKind::MessageUnparsable);
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn dep_info_says_which_variables_the_compiler_read_and_what_it_read() {
    let text = "/t/deps/demo-abc.d: src/lib.rs\n\nsrc/lib.rs:\n\n# env-dep:WHO=a\\\\b\\nc\n# env-dep:ABSENT\n";
    assert_eq!(
        env_deps(text),
        std::collections::BTreeMap::from([
            ("ABSENT".to_owned(), None),
            ("WHO".to_owned(), Some("a\\b\nc".to_owned())),
        ]),
        "rustc escapes a backslash and a line feed to keep the value on its line, and an unset \
         variable it read is written without a value"
    );
}

#[test]
fn dep_info_lists_the_prerequisites_of_the_first_rule_with_escapes_undone() {
    let text = "/t/deps/demo-abc.d: src/lib.rs src/with\\ space.rs \\\n  crates/x/src/mod.rs\n\n/t/deps/libdemo-abc.rmeta: src/lib.rs\n\nsrc/lib.rs:\n";
    let parsed = parse_dep_info(text);
    assert_eq!(result_state(&parsed), Returned, "dep-info: {parsed:?}");
    let Ok(parsed) = parsed else { return };
    assert_eq!(
        parsed,
        ["src/lib.rs", "src/with space.rs", "crates/x/src/mod.rs"]
    );
    let empty = parse_dep_info("out.d: \\\n\n");
    assert_eq!(result_state(&empty), Returned, "empty dep-info: {empty:?}");
    let Ok(empty) = empty else { return };
    assert_eq!(empty, Vec::<String>::new());
    let error = parse_dep_info("no rule here\n");
    assert_eq!(result_state(&error), Refused, "missing rule: {error:?}");
    let Err(error) = error else { return };
    assert_eq!(error.kind(), CargoErrorKind::DepInfoUnreadable);
    let error = parse_dep_info("");
    assert_eq!(result_state(&error), Refused, "empty input: {error:?}");
    let Err(error) = error else { return };
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
    let Message::CompilerArtifact(artifact) = message else {
        panic!("not an artifact: {message:?}");
    };
    artifact
}

fn diagnostic_of(message: &Message) -> &Diagnostic {
    let Message::CompilerMessage(message) = message else {
        panic!("not a compiler message: {message:?}");
    };
    &message.message
}

#[test]
fn every_cargo_error_kind_has_a_code_in_the_workspace_area() {
    for kind in CargoErrorKind::ALL {
        let code = kind.code().code;
        assert!(
            code.starts_with("RM1") || code.starts_with("RM2") || kind == CargoErrorKind::Cancelled,
            "{code}"
        );
    }
    assert_eq!(
        CargoErrorKind::Cancelled.code().code,
        "RM0001",
        "a command nobody waited for is the caller's cancellation, not a fact about the \
         workspace, and it is the one cargo failure that is not"
    );
    let error = parse_dep_info("");
    assert_eq!(result_state(&error), Refused, "empty dep-info: {error:?}");
    let Err(error): Result<_, CargoError> = error else {
        return;
    };
    assert_eq!(error.kind().code().code, "RM2001");
}

#[test]
fn a_build_script_executed_message_carries_its_out_dir_and_environment() {
    let stream = r#"{"reason":"build-script-executed","package_id":"demo 0.1.0","linked_libs":[],"linked_paths":[],"cfgs":[],"env":[["FIXTURE_TAG","written"],["OTHER","2"]],"out_dir":"/t/debug/build/demo-abc/out"}
{"reason":"build-finished","success":true}
"#;
    let messages = parse_messages(stream.as_bytes());
    assert_eq!(result_state(&messages), Returned, "messages: {messages:?}");
    let Ok(messages) = messages else { return };
    let Message::BuildScriptExecuted(script) = &messages[0] else {
        panic!("{messages:?}");
    };
    assert_eq!(script.package_id, "demo 0.1.0");
    assert_eq!(
        script.out_dir.as_deref(),
        Some(Path::new("/t/debug/build/demo-abc/out")),
        "a unit that reads a build script reads its OUT_DIR back at run time, so a run that \
         starts the test process itself has to say where it is"
    );
    assert_eq!(
        script.env,
        vec![
            ("FIXTURE_TAG".to_owned(), "written".to_owned()),
            ("OTHER".to_owned(), "2".to_owned()),
        ],
        "and what the script put in the environment, in the order it said them"
    );
}

#[test]
fn a_build_script_that_wrote_nowhere_says_so_rather_than_guessing() {
    let stream = "{\"reason\":\"build-script-executed\",\"package_id\":\"demo 0.1.0\"}\n";
    let messages = parse_messages(stream.as_bytes());
    assert_eq!(result_state(&messages), Returned, "messages: {messages:?}");
    let Ok(messages) = messages else { return };
    let Message::BuildScriptExecuted(script) = &messages[0] else {
        panic!("{messages:?}");
    };
    assert_eq!(script.out_dir, None);
    assert!(script.env.is_empty());
}

#[test]
fn compile_arguments_spell_every_build_option_once() {
    let options = CompileOptions {
        kind: CompileKind::Tests,
        packages: vec!["one".to_owned()],
        target_dir: Some(PathBuf::from("/tmp/out")),
        locked: true,
        offline: true,
        build: BuildConfig {
            features: vec!["a".to_owned(), "b".to_owned()],
            all_features: false,
            no_default_features: true,
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            profile: Some("release".to_owned()),
            jobs: Some(3),
            debug: false,
        },
        ..CompileOptions::default()
    };
    assert_eq!(
        compile_arguments(&options),
        [
            "test",
            "--package",
            "one",
            "--all-targets",
            "--no-run",
            "--message-format=json",
            "--locked",
            "--offline",
            "--target-dir",
            "/tmp/out",
            "--no-default-features",
            "--features",
            "a,b",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--profile",
            "release",
            "--jobs",
            "3",
        ]
    );
}

#[test]
fn a_build_configured_with_nothing_asks_for_nothing_but_the_bytes_nobody_reads() {
    let bare = compile_arguments(&CompileOptions::default());
    assert_eq!(
        bare,
        [
            "check",
            "--workspace",
            "--all-targets",
            "--message-format=json",
            "--config",
            "profile.dev.debug=0",
            "--config",
            "profile.test.debug=0"
        ]
    );
}

#[test]
fn all_features_and_a_named_feature_are_both_spelled_because_cargo_accepts_both() {
    let options = CompileOptions {
        build: BuildConfig {
            features: vec!["a".to_owned()],
            all_features: true,
            ..BuildConfig::default()
        },
        ..CompileOptions::default()
    };
    let args = compile_arguments(&options);
    assert!(args.iter().any(|argument| argument == "--all-features"));
    assert_eq!(
        args.iter().filter(|arg| *arg == "--features").count(),
        1,
        "every feature the run asked for is one argument, not one argument each"
    );
}

#[test]
fn a_build_writes_no_debug_information_unless_it_is_asked_to() {
    use rust_mutants::cargo::{BuildConfig, CompileKind, CompileOptions, compile_arguments};
    let plain = compile_arguments(&CompileOptions {
        kind: CompileKind::Tests,
        ..CompileOptions::default()
    });
    assert!(
        plain
            .windows(2)
            .any(|pair| pair == ["--config", "profile.test.debug=0"]),
        "the engine reads what a test harness printed and never a backtrace, so the debug \
         information a build writes is bytes nobody reads: {plain:?}"
    );
    assert!(
        plain
            .windows(2)
            .any(|pair| pair == ["--config", "profile.dev.debug=0"]),
        "and a check compiles with the dev profile: {plain:?}"
    );

    let asked = compile_arguments(&CompileOptions {
        kind: CompileKind::Tests,
        build: BuildConfig {
            debug: true,
            ..BuildConfig::default()
        },
        ..CompileOptions::default()
    });
    assert!(
        !asked
            .iter()
            .any(|one| { one.as_os_str().as_encoded_bytes().starts_with(b"profile.") }),
        "somebody who wants a debugger on a kept snapshot says so, and then nothing overrides \
         the profile they wrote: {asked:?}"
    );

    let named = compile_arguments(&CompileOptions {
        kind: CompileKind::Tests,
        build: BuildConfig {
            profile: Some("bench".to_owned()),
            ..BuildConfig::default()
        },
        ..CompileOptions::default()
    });
    assert!(
        !named
            .iter()
            .any(|one| { one.as_os_str().as_encoded_bytes().starts_with(b"profile.") }),
        "a profile somebody named is one they meant, and this engine does not edit it: {named:?}"
    );
}

#[test]
fn a_run_told_not_to_touch_the_network_tells_every_command_it_starts() {
    use rust_mutants::cargo::{MetadataOptions, metadata_arguments};

    let promised = MetadataOptions {
        locked: true,
        offline: true,
    };
    assert_eq!(
        metadata_arguments(promised, false),
        ["metadata", "--format-version", "1", "--locked", "--offline"],
        "resolving the workspace is a command a run starts, and one that reached out anyway \
         would keep the promise for the builds and break it before the first of them"
    );
    assert_eq!(
        metadata_arguments(
            MetadataOptions {
                locked: false,
                offline: false,
            },
            true,
        ),
        ["metadata", "--format-version", "1", "--no-deps"],
        "and a run that promised neither asks for neither, or every project would be resolved \
         against a lock file it did not agree to; the resolve that reads no dependencies is \
         the one that says so"
    );
}

#[test]
fn a_windows_path_in_dep_info_keeps_its_separators() {
    let parsed = parse_dep_info("C:\\t\\demo.d: C:\\src\\lib.rs C:\\with\\ space\\x.rs\n");
    let Ok(parsed) = parsed else {
        panic!("dep-info: {parsed:?}");
    };
    assert_eq!(
        parsed,
        ["C:\\src\\lib.rs", "C:\\with space\\x.rs"],
        "only a space or another backslash follows an escaping backslash, so every other one is \
         a separator the path keeps"
    );
}

#[test]
fn a_dep_info_names_the_environment_it_read_set_or_not() {
    let text = "target/debug/deps/lib.rmeta: src/lib.rs src/answer.txt\n\nsrc/lib.rs:\nsrc/answer.txt:\n\n# env-dep:OUT_DIR=/tmp/out\n# env-dep:NEVER_SET\n";
    assert_eq!(
        env_deps(text),
        std::collections::BTreeMap::from([
            ("NEVER_SET".to_owned(), None),
            ("OUT_DIR".to_owned(), Some("/tmp/out".to_owned())),
        ]),
        "an `env!` a compilation read is an input to what it computes, and an unset one as much as a set one"
    );
}

#[test]
fn a_search_path_entry_the_shell_would_pass_over_does_not_stop_the_search() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let blocker = temp.path().join("not-a-directory");
    std::fs::write(
        &blocker,
        "a file where a directory of programs was expected",
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", blocker.display()));
    let bin = temp.path().join("bin");
    std::fs::create_dir_all(&bin)
        .unwrap_or_else(|error| panic!("mkdir {}: {error}", bin.display()));
    let cargo = bin.join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
    std::fs::write(&cargo, "").unwrap_or_else(|error| panic!("write {}: {error}", cargo.display()));
    let search = std::env::join_paths([&blocker, &bin])
        .unwrap_or_else(|error| panic!("join paths: {error}"));
    let found = resolve_executable(Path::new("cargo"), Some(&search));
    assert!(
        matches!(&found, Ok(path) if *path == cargo),
        "a shell looking for `cargo` passes over an entry of PATH it cannot look inside and runs \
         the next one, so the engine must find the same program: {found:?}"
    );
}

#[test]
fn a_search_that_passed_over_entries_and_found_nothing_names_them() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let blocker = temp.path().join("not-a-directory");
    std::fs::write(
        &blocker,
        "a file where a directory of programs was expected",
    )
    .unwrap_or_else(|error| panic!("write {}: {error}", blocker.display()));
    let search =
        std::env::join_paths([&blocker]).unwrap_or_else(|error| panic!("join paths: {error}"));
    let Err(error) = resolve_executable(Path::new("cargo"), Some(&search)) else {
        panic!("nothing on this search path is cargo");
    };
    assert_eq!(error.kind(), CargoErrorKind::ToolchainNotFound, "{error}");
    let said = error.to_string();
    if !cfg!(windows) {
        assert!(
            said.contains(&blocker.join("cargo").display().to_string()),
            "a miss that passed over an entry it could not read says which, so it is never \
             mistaken for a clean miss: {said}"
        );
    }
}
