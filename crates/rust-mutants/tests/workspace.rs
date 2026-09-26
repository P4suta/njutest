// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opening a tree: what the engine asks the toolchain, and what it says when the answer is not one it can use.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use njutest_devkit::fake_cargo::{Installed, Invocation, Script, install};
use njutest_devkit::fixture::Fixture;
use njutest_devkit::result::{
    ResultState::{Refused, Returned},
    result_state,
};
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
    let mut env: rust_mutants::vars::Variables = installed.env().into_iter().collect();
    env.set("PATH", installed.bin());
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(OsString::from(installed.bin())),
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
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(result_state(&opened), Refused, "metadata fails: {opened:?}");
    let Err(error) = opened else { return };
    drop(installed_toolchain);
    assert_eq!(error.code().code, "RM1014", "{error}");
    assert!(
        error.to_string().contains("failed to load manifest"),
        "cargo's own words come back: {error}"
    );
}

#[test]
fn opening_refuses_a_cargo_that_is_not_a_file() {
    let fixture = Fixture::copy("fixture-simple");
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(PathBuf::from("/nonexistent/cargo")),
            temp_directory: fixture.temp().to_path_buf(),
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    assert_eq!(result_state(&opened), Refused, "no such cargo: {opened:?}");
    let Err(error) = opened else { return };
    assert_eq!(error.code().code, "RM1012", "{error}");
}

#[test]
fn opening_refuses_a_temporary_root_that_does_not_exist() {
    let fixture = Fixture::copy("fixture-simple");
    let missing = fixture.temp().join("not-created");
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(PathBuf::from("/nonexistent/cargo")),
            temp_directory: missing.clone(),
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    assert_eq!(
        result_state(&opened),
        Refused,
        "a missing temporary authority is refused: {opened:?}"
    );
    let Err(error) = opened else { return };
    assert_eq!(error.code().code, "RM5006", "{error}");
    assert!(
        error.to_string().contains(&missing.display().to_string()),
        "the typed refusal names the unbound directory: {error}"
    );
}

#[test]
fn opening_refuses_a_banner_without_a_release_line() {
    let fixture = Fixture::copy("fixture-simple");
    let script = Script::new()
        .answering(Invocation::new("cargo", &["-vV"]).printing("cargo 1.98.0\nhost: x86_64\n"));
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Refused,
        "a banner with no release: {opened:?}"
    );
    let Err(error) = opened else { return };
    drop(installed_toolchain);
    assert_eq!(error.code().code, "RM1013", "{error}");
}

#[test]
fn opening_refuses_metadata_that_is_not_its_document() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers()
        .answering(Invocation::new("cargo", &["metadata"]).printing("{\"not\": \"metadata\"}\n"));
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Refused,
        "metadata that is not one: {opened:?}"
    );
    let Err(error) = opened else { return };
    drop(installed_toolchain);
    assert_eq!(error.code().code, "RM1015", "{error}");
}

#[test]
fn opening_copies_the_tree_and_asks_the_toolchain_in_the_copy() {
    let fixture = Fixture::copy("fixture-simple");
    let script = toolchain_answers().answering(
        Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
    );
    let installed = install(&script);
    let mut env: rust_mutants::vars::Variables = installed.env().into_iter().collect();
    env.set("PATH", installed.bin());
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(OsString::from(installed.bin())),
            temp_directory: fixture.temp().to_path_buf(),
            env,
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    assert_eq!(
        result_state(&workspace),
        Returned,
        "the workspace opens: {workspace:?}"
    );
    let Ok(workspace) = workspace else { return };

    assert_ne!(
        workspace.snapshot_root(),
        fixture.root(),
        "the tree the engine works in is the copy"
    );
    let copied_source = std::fs::metadata(workspace.snapshot_root().join("src/lib.rs"));
    assert!(matches!(copied_source, Ok(metadata) if metadata.is_file()));
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

#[cfg(unix)]
#[derive(Debug, thiserror::Error)]
enum TemporaryAliasFixtureError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Engine(#[from] rust_mutants::EngineError),
    #[error(
        "snapshot {snapshot} was minted from alias {alias} instead of physical root {physical}"
    )]
    SnapshotOutside {
        alias: PathBuf,
        physical: PathBuf,
        snapshot: PathBuf,
    },
    #[error("build cache or scratch path was minted outside physical root {physical}")]
    SupportingPathOutside { physical: PathBuf },
    #[error("snapshot root {snapshot} is not its physical spelling {physical}")]
    SnapshotNotPhysical {
        snapshot: PathBuf,
        physical: PathBuf,
    },
}

#[cfg(unix)]
#[test]
fn a_temporary_root_alias_is_resolved_before_any_workspace_path_is_minted()
-> Result<(), TemporaryAliasFixtureError> {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::copy("fixture-simple");
    let physical = fixture.temp().join("physical-temporary-root");
    std::fs::create_dir_all(&physical)?;
    let alias = fixture.temp().join("temporary-root-alias");
    symlink(&physical, &alias)?;

    let script = toolchain_answers().answering(
        Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
    );
    let installed = install(&script);
    let mut env: rust_mutants::vars::Variables = installed.env().into_iter().collect();
    env.set("PATH", installed.bin());
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(OsString::from(installed.bin())),
            temp_directory: alias.clone(),
            env,
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )?;

    let physical = std::fs::canonicalize(&physical)?;
    if !workspace.snapshot_root().starts_with(&physical) {
        return Err(TemporaryAliasFixtureError::SnapshotOutside {
            alias,
            physical,
            snapshot: workspace.snapshot_root().to_path_buf(),
        });
    }
    if !workspace.target_dir().starts_with(&physical)
        || !workspace.scratch_dir().starts_with(&physical)
    {
        return Err(TemporaryAliasFixtureError::SupportingPathOutside { physical });
    }
    let canonical_snapshot = std::fs::canonicalize(workspace.snapshot_root())?;
    if canonical_snapshot != workspace.snapshot_root() {
        return Err(TemporaryAliasFixtureError::SnapshotNotPhysical {
            snapshot: workspace.snapshot_root().to_path_buf(),
            physical: canonical_snapshot,
        });
    }
    Ok(())
}

#[test]
fn opening_a_member_directory_names_the_workspace_root_and_the_flag() {
    let fixture = Fixture::copy("fixture-simple");
    let elsewhere = fixture.temp().join("the-workspace");
    let document = metadata_document_rooted(fixture.root(), &elsewhere);
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Refused,
        "a member is not a workspace: {opened:?}"
    );
    let Err(error) = opened else { return };
    drop(installed_toolchain);
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
    let elsewhere = fixture.temp().join("elsewhere");
    let document = metadata_document_reading(fixture.root(), "outside", &elsewhere);
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Refused,
        "a tree that reads from outside itself: {opened:?}"
    );
    let Err(error) = opened else { return };
    drop(installed_toolchain);
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
    let created = std::fs::create_dir_all(allowed.join("src"));
    assert_eq!(
        result_state(&created),
        Returned,
        "the directory beside the tree: {created:?}"
    );
    let manifest = std::fs::write(
        allowed.join("Cargo.toml"),
        "[package]\nname = \"outside\"\n",
    );
    assert_eq!(
        result_state(&manifest),
        Returned,
        "its manifest: {manifest:?}"
    );
    let source = std::fs::write(allowed.join("src/lib.rs"), "pub fn f() {}\n");
    assert_eq!(result_state(&source), Returned, "its source: {source:?}");
    let document = metadata_document_reading(fixture.root(), "outside", &allowed);
    let script =
        toolchain_answers().answering(Invocation::new("cargo", &["metadata"]).printing(&document));
    let installed = install(&script);
    let mut env: rust_mutants::vars::Variables = installed.env().into_iter().collect();
    env.set("PATH", installed.bin());
    let opened = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(installed.cargo()),
            search_path: Some(OsString::from(installed.bin())),
            temp_directory: fixture.temp().to_path_buf(),
            env,
            locked: true,
            offline: true,
            allow_outside: vec![allowed.clone()],
            ..OpenOptions::default()
        },
        &Cancel::new(),
    );
    assert_eq!(
        result_state(&opened),
        Returned,
        "a directory somebody named is one the run may read: {opened:?}"
    );
    let Ok(workspace) = opened else { return };
    let as_written = between(fixture.root(), &allowed);
    let in_the_copy = folded(&workspace.snapshot_root().join(&as_written));
    let reached = std::fs::metadata(in_the_copy.join("Cargo.toml"));
    assert!(
        matches!(reached, Ok(entry) if entry.is_file()),
        "the path the tree writes to reach it is {}, and following that from the copy of \
         the tree has to reach the copy of it: a test that checked a place it computed \
         the way the code does could not tell whether anything resolved. {} holds no \
         manifest",
        as_written.display(),
        in_the_copy.display()
    );
}

/// The relative path a manifest inside `from` writes to reach `to`.
fn between(from: &Path, to: &Path) -> PathBuf {
    let mine: Vec<Component<'_>> = from.components().collect();
    let theirs: Vec<Component<'_>> = to.components().collect();
    let shared = mine
        .iter()
        .zip(&theirs)
        .take_while(|(one, other)| one == other)
        .count();
    let mut found = PathBuf::new();
    for _climbed in shared..mine.len() {
        found.push("..");
    }
    for part in theirs.iter().skip(shared) {
        found.push(part);
    }
    found
}

/// `path` with every `..` folded, which is what cargo does with a declared path.
fn folded(path: &Path) -> PathBuf {
    let mut parts: Vec<OsString> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = parts.len().checked_sub(1)
                    && last > 0
                {
                    parts.truncate(last);
                }
            }
            other => parts.push(other.as_os_str().to_owned()),
        }
    }
    parts.iter().collect()
}

/// The one package every document below reports, at `root`.
fn demo(root: &Path) -> njutest_devkit::cargo_double::Package {
    use njutest_devkit::cargo_double::{Package, Target};

    Package::at("demo", root).building(Target::library("demo", &root.join("src/lib.rs")))
}

/// A `cargo metadata` document for a one-package workspace at `root`.
fn metadata_document(root: &Path) -> String {
    njutest_devkit::cargo_double::Document::of(root)
        .holding(demo(root))
        .json()
}

/// The same, for a workspace cargo says is rooted somewhere else.
fn metadata_document_rooted(root: &Path, workspace_root: &Path) -> String {
    njutest_devkit::cargo_double::Document::of(workspace_root)
        .holding(demo(root))
        .json()
}

/// The same, where the one package reads `name` from `directory`.
fn metadata_document_reading(root: &Path, name: &str, directory: &Path) -> String {
    use njutest_devkit::cargo_double::{Document, PathDependency};

    Document::of(root)
        .holding(demo(root).reading(PathDependency::on(name, directory)))
        .json()
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
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Returned,
        "the workspace opens: {opened:?}"
    );
    let Ok(workspace) = opened else { return };
    let error = workspace.prepare(&PrepareOptions::default(), &Cancel::new());
    assert_eq!(
        result_state(&error),
        Refused,
        "a tree that does not compile: {error:?}"
    );
    let Err(error) = error else { return };
    drop(installed_toolchain);
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
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Returned,
        "the workspace opens: {opened:?}"
    );
    let Ok(workspace) = opened else { return };
    let error = workspace.prepare(&PrepareOptions::default(), &Cancel::new());
    assert_eq!(
        result_state(&error),
        Refused,
        "a stream that is not messages: {error:?}"
    );
    let Err(error) = error else { return };
    drop(installed_toolchain);
    assert_eq!(error.code().code, "RM1016", "{error}");
    assert!(
        error.to_string().contains("line 2"),
        "the line a reader would go to: {error}"
    );
}

#[test]
fn a_check_that_takes_longer_than_the_build_timeout_says_it_timed_out() {
    let fixture = Fixture::copy("fixture-simple");
    let finished = fixture.temp().join("the-check-was-allowed-to-finish");
    let script = toolchain_answers()
        .answering(
            Invocation::new("cargo", &["metadata"]).printing(&metadata_document(fixture.root())),
        )
        .answering(
            Invocation::new("cargo", &["check"])
                .taking(5_000)
                .writing_after(njutest_devkit::paths::utf8(&finished), "it was"),
        );
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(
        result_state(&opened),
        Returned,
        "the workspace opens: {opened:?}"
    );
    let Ok(workspace) = opened else { return };
    let options = PrepareOptions {
        build_timeout: Some(std::time::Duration::from_millis(200)),
        ..PrepareOptions::default()
    };
    let error = workspace.prepare(&options, &Cancel::new());
    assert_eq!(
        result_state(&error),
        Refused,
        "a build that runs long: {error:?}"
    );
    let Err(error) = error else { return };
    drop(installed_toolchain);
    assert!(
        matches!(std::fs::metadata(&finished), Err(ref why) if why.kind() == std::io::ErrorKind::NotFound),
        "the timeout is what ended it, not the command: the check writes this once its \
         five seconds are up, and a test that read its own clock instead would fail on a \
         machine that was merely busy"
    );
    assert_eq!(error.code().code, "RM1014", "{error}");
    assert!(error.to_string().contains("timed out"), "{error}");
}

/// A `compiler-message` a check would print for a tree that does not parse.
fn compiler_message(root: &Path, said: &str) -> String {
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
    let (opened, installed_toolchain) = opened(&fixture, &script);
    assert_eq!(result_state(&opened), Returned, "open: {opened:?}");
    let Ok(workspace) = opened else { return };

    let cancel = Cancel::new();
    let waiting = cancel.clone();
    let stopping = njutest_devkit::thread::JoinedThread::launch(move || {
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
    );
    assert_eq!(
        result_state(&error),
        Refused,
        "a cancelled compilation does not answer: {error:?}"
    );
    let Err(error) = error else { return };
    let stopped = stopping.join();
    assert_eq!(
        result_state(&stopped),
        Returned,
        "the canceller: {stopped:?}"
    );
    drop(installed_toolchain);

    assert_eq!(
        error.kind(),
        rust_mutants::cargo::CargoErrorKind::Cancelled,
        "a build nobody waited for printed nothing about the tree, and reading its silence as \
         a failed build condemns mutants the compiler never saw"
    );
    assert_eq!(error.kind().code().code, "RM0001");
    let closed = workspace.close();
    assert_eq!(result_state(&closed), Returned, "close: {closed:?}");
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
