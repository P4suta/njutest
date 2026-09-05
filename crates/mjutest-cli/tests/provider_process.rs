// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Talking to a real provider process: what a run holds, and what it refuses to hold.

#![cfg(unix)]
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use mjutest_cli::config::Resource;
use mjutest_cli::resource::{Manager, ResourceError, Where};

/// What the provider says it stopped.
const STOPPED: &str = r#"{"version":1,"status":"stopped","instance":"pg-1"}"#;

/// The names the provider reads its answers from, which it may see because the resource says so.
const SPOKEN: [&str; 3] = [
    "FAKE_PROVIDER_READY",
    "FAKE_PROVIDER_STOPPED",
    "FAKE_PROVIDER_SILENT",
];

/// The provider every test here drives. It is read rather than written: see the script's own note.
fn provider() -> Vec<String> {
    vec![
        "/bin/sh".to_owned(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/fake-provider.sh")
            .to_string_lossy()
            .into_owned(),
    ]
}

fn resource(command: Vec<String>) -> Resource {
    Resource {
        command,
        timeout: Duration::from_secs(5),
        shared: false,
        exclusive: false,
        environment: SPOKEN.map(str::to_owned).to_vec(),
    }
}

/// What a provider is told, which is what it answers with.
fn saying(ready: &str, silent: bool) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| name == "PATH")
        .collect();
    env.push((OsString::from("FAKE_PROVIDER_READY"), OsString::from(ready)));
    env.push((
        OsString::from("FAKE_PROVIDER_STOPPED"),
        OsString::from(STOPPED),
    ));
    if silent {
        env.push((OsString::from("FAKE_PROVIDER_SILENT"), OsString::from("1")));
    }
    env
}

fn place(dir: &Path, ready: &str) -> Where {
    Where {
        dir: dir.to_path_buf(),
        env: saying(ready, false),
    }
}

#[test]
fn a_run_holds_what_the_provider_started_and_tells_its_tests_where_it_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#,
    ));
    let lease = manager
        .start("postgres", &resource(provider()))
        .expect("the provider is ready");
    assert_eq!(lease.instance, "pg-1");
    assert_eq!(
        manager.environment(),
        vec![(
            "DATABASE_URL".to_owned(),
            "postgres://127.0.0.1/test".to_owned()
        )]
    );
    assert!(manager.release().is_empty());
    assert!(manager.is_empty());
}

#[test]
fn asking_twice_for_one_capability_holds_one_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{}}"#,
    ));
    let first = manager
        .start("postgres", &resource(provider()))
        .expect("ready")
        .instance
        .clone();
    let again = manager
        .start("postgres", &resource(provider()))
        .expect("ready")
        .instance
        .clone();
    assert_eq!(first, again);
    assert_eq!(manager.leases().len(), 1);
    assert!(manager.release().is_empty());
}

#[test]
fn a_provider_that_says_nothing_ends_the_lease_rather_than_the_run_waiting_on_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(Where {
        dir: dir.path().to_path_buf(),
        env: saying("", true),
    });
    let mut slow = resource(provider());
    slow.timeout = Duration::from_millis(200);
    let refused = manager
        .start("silent", &slow)
        .expect_err("nothing was said");
    assert_eq!(refused.code().code, "MJ5002", "{refused}");
    assert!(manager.is_empty());
}

#[test]
fn a_provider_that_offers_what_a_run_composes_itself_is_refused_and_stopped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"CARGO_TARGET_DIR":"/elsewhere"}}"#,
    ));
    let refused = manager
        .start("greedy", &resource(provider()))
        .expect_err("refused");
    assert!(matches!(refused, ResourceError::EnvironmentRefused { .. }));
    assert_eq!(refused.code().code, "MJ5005");
    assert!(manager.is_empty());
}

#[test]
fn a_provider_that_answers_another_protocol_is_not_guessed_at() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":2,"status":"ready","instance":"pg-1"}"#,
    ));
    let refused = manager
        .start("future", &resource(provider()))
        .expect_err("refused");
    assert_eq!(refused.code().code, "MJ5003", "{refused}");
}

#[test]
fn a_provider_that_says_it_cannot_says_so_in_the_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"error","message":"no docker"}"#,
    ));
    let refused = manager
        .start("unwilling", &resource(provider()))
        .expect_err("refused");
    assert_eq!(refused.code().code, "MJ5004", "{refused}");
    assert!(refused.to_string().contains("no docker"), "{refused}");
}

#[test]
fn a_provider_that_is_not_there_is_not_a_resource_the_run_has() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(dir.path(), ""));
    let refused = manager
        .start(
            "missing",
            &resource(vec![
                dir.path().join("nothing").to_string_lossy().into_owned(),
            ]),
        )
        .expect_err("refused");
    assert_eq!(refused.code().code, "MJ5001", "{refused}");
}
