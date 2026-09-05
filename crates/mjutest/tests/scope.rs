// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ported from goatest's `api_test.go`: the metadata is small, immutable, and
//! strict about blank names.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use mjutest::{InvalidScope, Scope, ScopeKind};

#[test]
fn unit_declares_no_capabilities() {
    let scope = Scope::unit();
    assert_eq!(scope.kind(), ScopeKind::Unit);
    assert!(scope.capabilities().is_empty());
}

#[test]
fn integration_trims_and_deduplicates_capabilities_in_first_seen_order() {
    let scope = Scope::integration(["postgres", " redis ", "postgres"]).expect("valid");
    assert_eq!(scope.kind(), ScopeKind::Integration);
    assert_eq!(scope.capabilities(), ["postgres", "redis"]);
}

#[test]
fn integration_requires_at_least_one_capability() {
    let error = Scope::integration(Vec::<&str>::new()).expect_err("empty is refused");
    assert_eq!(error, InvalidScope::NoCapabilities);
    assert_eq!(
        error.to_string(),
        "integration requires at least one capability"
    );
}

#[test]
fn integration_refuses_a_blank_capability() {
    let error = Scope::integration(["postgres", "  "]).expect_err("blank is refused");
    assert_eq!(error, InvalidScope::BlankCapability { position: 1 });
    assert_eq!(
        error.to_string(),
        "integration capability 2 must not be blank"
    );
}
