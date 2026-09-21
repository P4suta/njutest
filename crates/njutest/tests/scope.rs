// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ported from goatest's `api_test.go`: the metadata is small, immutable, and strict about blank names.

use njutest::{InvalidScope, Scope, ScopeKind};
use njutest_devkit::result::{ResultState, result_state};

#[test]
fn unit_declares_no_capabilities() {
    let scope = Scope::unit();
    assert_eq!(scope.kind(), ScopeKind::Unit);
    assert!(scope.capabilities().is_empty());
}

#[test]
fn integration_trims_and_deduplicates_capabilities_in_first_seen_order() {
    let result = Scope::integration(["postgres", " redis ", "postgres"]);
    assert_eq!(result_state(&result), ResultState::Returned);
    let Ok(scope) = result else { return };
    assert_eq!(scope.kind(), ScopeKind::Integration);
    assert_eq!(scope.capabilities(), ["postgres", "redis"]);
}

#[test]
fn integration_requires_at_least_one_capability() {
    let result = Scope::integration(Vec::<&str>::new());
    assert_eq!(result_state(&result), ResultState::Refused);
    let Err(error) = result else { return };
    assert_eq!(error, InvalidScope::NoCapabilities);
    assert_eq!(
        error.to_string(),
        "integration requires at least one capability"
    );
}

#[test]
fn integration_refuses_a_blank_capability() {
    let result = Scope::integration(["postgres", "  "]);
    assert_eq!(result_state(&result), ResultState::Refused);
    let Err(error) = result else { return };
    assert_eq!(error, InvalidScope::BlankCapability { position: 1 });
    assert_eq!(
        error.to_string(),
        "integration capability 2 must not be blank"
    );
}
