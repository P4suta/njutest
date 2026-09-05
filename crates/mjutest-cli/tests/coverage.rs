// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage is read by the engine and reported in this product's ledger.

use mjutest_cli::coverage::{CoverageError, CoverageErrorKind, parse_export};

#[test]
fn an_export_that_is_not_one_is_refused_in_this_products_ledger() {
    let refused = parse_export(b"not json").expect_err("refused");
    let stated = CoverageError::from(refused);
    assert_eq!(stated.kind(), CoverageErrorKind::Unreadable);
    assert_eq!(stated.code().code, "MJ4001");
    assert!(stated.to_string().starts_with("MJ4001: "), "{stated}");
}

#[test]
fn every_failure_mode_the_engine_states_has_a_code_in_this_products_ledger() {
    let codes: Vec<&str> = CoverageErrorKind::ALL
        .iter()
        .map(|kind| kind.code().code)
        .collect();
    assert_eq!(codes, ["MJ4001", "MJ4002", "MJ4003", "MJ4004"]);
    for engine in rust_mutants::coverage::CoverageErrorKind::ALL {
        let mine = CoverageErrorKind::from(engine);
        assert!(codes.contains(&mine.code().code), "{engine:?}");
    }
}
