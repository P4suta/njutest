// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Talking to a real provider process: what a run holds, and what it refuses to hold.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::time::Duration;

use mjutest_cli::config::Resource;
use mjutest_cli::resource::{Manager, ResourceError, Where};

/// A provider that answers `answers` to a start and stops when asked.
fn provider(dir: &std::path::Path, name: &str, body: &str) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = dir.join(name);
    std::fs::write(&path, body).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    vec![path.to_string_lossy().into_owned()]
}

fn speaking(ready: &str) -> String {
    format!(
        "#!/bin/sh\n\
         while IFS= read -r line; do\n\
         \x20 case \"$line\" in\n\
         \x20   *'\"action\":\"start\"'*) printf '%s\\n' '{ready}' ;;\n\
         \x20   *'\"action\":\"stop\"'*) printf '%s\\n' '{{\"version\":1,\"status\":\"stopped\",\"instance\":\"pg-1\"}}'; exit 0 ;;\n\
         \x20 esac\n\
         done\n"
    )
}

const fn resource(command: Vec<String>) -> Resource {
    Resource {
        command,
        timeout: Duration::from_secs(5),
        shared: false,
        exclusive: false,
        environment: Vec::new(),
    }
}

fn environment() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(name, _)| name == "PATH")
        .collect()
}

fn place(dir: &std::path::Path) -> Where {
    Where {
        dir: dir.to_path_buf(),
        env: environment(),
    }
}

#[test]
fn a_run_holds_what_the_provider_started_and_tells_its_tests_where_it_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = provider(
        dir.path(),
        "postgres",
        &speaking(
            r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#,
        ),
    );
    let mut manager = Manager::new(place(dir.path()));
    let lease = manager
        .start("postgres", &resource(command))
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
    let command = provider(
        dir.path(),
        "postgres",
        &speaking(r#"{"version":1,"status":"ready","instance":"pg-1","environment":{}}"#),
    );
    let mut manager = Manager::new(place(dir.path()));
    let first = manager
        .start("postgres", &resource(command.clone()))
        .expect("ready")
        .instance
        .clone();
    let again = manager
        .start("postgres", &resource(command))
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
    let command = provider(
        dir.path(),
        "silent",
        "#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
    );
    let mut manager = Manager::new(place(dir.path()));
    let mut slow = resource(command);
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
    let command = provider(
        dir.path(),
        "greedy",
        &speaking(
            r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"CARGO_TARGET_DIR":"/elsewhere"}}"#,
        ),
    );
    let mut manager = Manager::new(place(dir.path()));
    let refused = manager
        .start("greedy", &resource(command))
        .expect_err("refused");
    assert!(matches!(refused, ResourceError::EnvironmentRefused { .. }));
    assert_eq!(refused.code().code, "MJ5005");
    assert!(manager.is_empty());
}

#[test]
fn a_provider_that_answers_another_protocol_is_not_guessed_at() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = provider(
        dir.path(),
        "future",
        &speaking(r#"{"version":2,"status":"ready","instance":"pg-1"}"#),
    );
    let mut manager = Manager::new(place(dir.path()));
    let refused = manager
        .start("future", &resource(command))
        .expect_err("refused");
    assert_eq!(refused.code().code, "MJ5003", "{refused}");
}

#[test]
fn a_provider_that_says_it_cannot_says_so_in_the_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let command = provider(
        dir.path(),
        "unwilling",
        &speaking(r#"{"version":1,"status":"error","message":"no docker"}"#),
    );
    let mut manager = Manager::new(place(dir.path()));
    let refused = manager
        .start("unwilling", &resource(command))
        .expect_err("refused");
    assert_eq!(refused.code().code, "MJ5004", "{refused}");
    assert!(refused.to_string().contains("no docker"), "{refused}");
}

#[test]
fn a_provider_that_is_not_there_is_not_a_resource_the_run_has() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(dir.path()));
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
