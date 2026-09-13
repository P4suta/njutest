// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage is read by the engine and reported in this product's ledger.

use njutest_cli::coverage::{CoverageError, CoverageErrorKind, parse_export};

#[test]
fn an_export_that_is_not_one_is_refused_in_this_products_ledger() {
    let refused = parse_export(b"not json").expect_err("refused");
    let stated = CoverageError::from(refused);
    assert_eq!(stated.kind(), CoverageErrorKind::Unreadable);
    assert_eq!(stated.code().code, "NJ4001");
    assert!(stated.to_string().starts_with("NJ4001: "), "{stated}");
}

#[test]
fn every_failure_mode_the_engine_states_has_a_code_in_this_products_ledger() {
    let codes: Vec<&str> = CoverageErrorKind::ALL
        .iter()
        .map(|kind| kind.code().code)
        .collect();
    assert_eq!(codes, ["NJ4001", "NJ4002", "NJ4003", "NJ4004"]);
    for engine in rust_mutants::coverage::CoverageErrorKind::ALL {
        let mine = CoverageErrorKind::from(engine);
        assert!(codes.contains(&mine.code().code), "{engine:?}");
    }
}
