// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Talking to a real provider process: what a run holds, and what it refuses to hold.

#![cfg_attr(
    unix,
    expect(
        clippy::expect_used,
        reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
    )
)]
#![cfg(unix)]
use std::path::Path;
use std::time::Duration;

use njutest::config::Resource;
use njutest::resource::{Manager, ResourceError, Where};

/// What the provider says it stopped.
const STOPPED: &str = r#"{"version":1,"status":"stopped","instance":"pg-1"}"#;

/// The names the provider reads its answers from, which it may see because the resource says so.
const SPOKEN: [&str; 3] = [
    "FAKE_PROVIDER_READY",
    "FAKE_PROVIDER_STOPPED",
    "FAKE_PROVIDER_SILENT",
];

/// The provider every test here drives, as a program every platform can start.
fn provider(own: &Path) -> Vec<String> {
    vec![
        njutest_devkit::fake_cargo::example_in("fake_provider", own)
            .to_str()
            .expect("test protocol paths are UTF-8")
            .to_owned(),
        "resource".to_owned(),
    ]
}

/// A resource whose provider is given long enough to answer on a machine that has not seen its program before.
fn resource(command: Vec<String>) -> Resource {
    Resource {
        command,
        timeout: Duration::from_secs(30),
        shared: false,
        exclusive: false,
        environment: SPOKEN.map(str::to_owned).to_vec(),
        interpose: String::new(),
        wire: njutest::wire::Wire::Raw,
        hold: Duration::from_secs(30),
    }
}

/// What a provider is told, which is what it answers with.
fn saying(ready: &str, silent: bool) -> rust_mutants::vars::Variables {
    let mut env: rust_mutants::vars::Variables = std::env::vars_os()
        .filter(|(name, _)| njutest_devkit::paths::same_name(name, std::ffi::OsStr::new("PATH")))
        .collect();
    env.set("FAKE_PROVIDER_READY", ready);
    env.set("FAKE_PROVIDER_STOPPED", STOPPED);
    if silent {
        env.set("FAKE_PROVIDER_SILENT", "1");
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
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"DATABASE_URL":"postgres://127.0.0.1/test"}}"#,
    ));
    let lease = manager
        .start("postgres", &resource(provider(own.path())))
        .expect("the provider is ready");
    assert_eq!(lease.instance.as_str(), "pg-1");
    assert_eq!(
        manager.environment(),
        vec![(
            "DATABASE_URL".to_owned(),
            "postgres://127.0.0.1/test".to_owned()
        )]
    );
    assert!(manager.release().is_empty());
    assert!(manager.leases().is_empty());
}

#[test]
fn asking_twice_for_one_capability_holds_one_instance() {
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{}}"#,
    ));
    let first = manager
        .start("postgres", &resource(provider(own.path())))
        .expect("ready")
        .instance
        .clone();
    let again = manager
        .start("postgres", &resource(provider(own.path())))
        .expect("ready")
        .instance
        .clone();
    assert_eq!(first, again);
    assert_eq!(manager.leases().len(), 1);
    assert!(manager.release().is_empty());
}

#[test]
fn a_provider_that_says_nothing_ends_the_lease_rather_than_the_run_waiting_on_it() {
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(Where {
        dir: dir.path().to_path_buf(),
        env: saying("", true),
    });
    let mut slow = resource(provider(own.path()));
    slow.timeout = Duration::from_millis(200);
    let refused = manager
        .start("silent", &slow)
        .expect_err("nothing was said");
    assert_eq!(refused.code().code, "NJ5002", "{refused}");
    assert!(manager.leases().is_empty());
}

#[test]
fn a_provider_that_offers_what_a_run_composes_itself_is_refused_and_stopped() {
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"ready","instance":"pg-1","environment":{"CARGO_TARGET_DIR":"/elsewhere"}}"#,
    ));
    let refused = manager
        .start("greedy", &resource(provider(own.path())))
        .expect_err("refused");
    assert!(matches!(refused, ResourceError::EnvironmentRefused { .. }));
    assert_eq!(refused.code().code, "NJ5005");
    assert!(manager.leases().is_empty());
}

#[test]
fn a_provider_that_answers_another_protocol_is_not_guessed_at() {
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":2,"status":"ready","instance":"pg-1"}"#,
    ));
    let refused = manager
        .start("future", &resource(provider(own.path())))
        .expect_err("refused");
    assert_eq!(refused.code().code, "NJ5003", "{refused}");
}

#[test]
fn a_provider_that_says_it_cannot_says_so_in_the_run() {
    let own = tempfile::tempdir().expect("a directory of this test's own");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manager = Manager::new(place(
        dir.path(),
        r#"{"version":1,"status":"error","message":"no docker"}"#,
    ));
    let refused = manager
        .start("unwilling", &resource(provider(own.path())))
        .expect_err("refused");
    assert_eq!(refused.code().code, "NJ5004", "{refused}");
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
                dir.path()
                    .join("nothing")
                    .to_str()
                    .expect("test protocol paths are UTF-8")
                    .to_owned(),
            ]),
        )
        .expect_err("refused");
    assert_eq!(refused.code().code, "NJ5001", "{refused}");
}
