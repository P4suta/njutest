// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The resource protocol: what a provider is asked, what it may answer, and what a run refuses to let it decide.

use std::collections::BTreeMap;

use njutest::provider::{InstanceId, ProviderErrorKind, Request, VERSION};
use njutest::resource::{ResourceError, admissible, visible};

#[test]
fn a_start_request_is_the_document_the_protocol_states() {
    let written = serde_json::to_string(&Request::start("postgres", 1)).expect("json");
    assert_eq!(
        written,
        r#"{"version":1,"action":"start","capability":"postgres","request_id":"resource-000001"}"#
    );
}

#[test]
fn a_stop_request_names_the_instance_it_stops() {
    let instance = InstanceId::checked("pg-1").expect("instance id");
    let written = serde_json::to_string(&Request::stop("postgres", &instance, 2)).expect("json");
    assert_eq!(
        written,
        r#"{"version":1,"action":"stop","capability":"postgres","request_id":"resource-000002","instance":"pg-1"}"#
    );
}

#[test]
fn a_provider_may_tell_a_test_where_its_database_is() {
    let offered = BTreeMap::from([(
        "DATABASE_URL".to_owned(),
        "postgres://127.0.0.1/test".to_owned(),
    )]);
    let admitted = admissible("postgres", &offered).expect("admitted");
    assert_eq!(
        admitted,
        vec![(
            "DATABASE_URL".to_owned(),
            "postgres://127.0.0.1/test".to_owned()
        )]
    );
}

#[test]
fn a_provider_may_not_decide_what_every_test_process_measures() {
    for name in [
        "CARGO",
        "CARGO_TARGET_DIR",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "LLVM_PROFILE_FILE",
        "NJUTEST_TRACE",
        "RUST_MUTANTS_ACTIVE",
        "TMPDIR",
    ] {
        let offered = BTreeMap::from([(name.to_owned(), "anything".to_owned())]);
        let refused = admissible("postgres", &offered).expect_err("refused");
        assert_eq!(refused.code().code, "NJ5005", "{name}");
        assert!(matches!(refused, ResourceError::EnvironmentRefused { .. }));
    }
}

#[test]
fn a_provider_sees_the_path_and_exactly_what_its_configuration_names() {
    let env = [
        ("PATH", "/usr/bin"),
        ("HOME", "/home/someone"),
        ("SECRET", "no"),
    ]
    .map(|(name, value)| {
        (
            std::ffi::OsString::from(name),
            std::ffi::OsString::from(value),
        )
    });
    let seen = visible(
        &env.into_iter().collect::<rust_mutants::vars::Variables>(),
        &["HOME".to_owned()],
    );
    let names: Vec<String> = seen
        .for_process()
        .map(|(name, _)| {
            name.to_str()
                .expect("test protocol paths are UTF-8")
                .to_owned()
        })
        .collect();
    assert_eq!(names, ["PATH", "HOME"]);
}

#[test]
fn the_protocol_version_this_release_speaks_is_fixed() {
    assert_eq!(VERSION, 1);
    assert_eq!(
        ProviderErrorKind::ALL.map(|kind| kind.code().code),
        ["NJ5001", "NJ5002", "NJ5003", "NJ5004"]
    );
}
