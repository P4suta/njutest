// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The attributes leave the test an ordinary `#[test]`.

#[njutest::integration("postgres", "redis")]
#[test]
fn an_integration_test_still_runs_as_an_ordinary_test() {
    assert_eq!(1 + 1, 2);
}

#[njutest::unit]
#[test]
fn a_unit_test_still_runs_as_an_ordinary_test() {
    assert_eq!(2 + 2, 4);
}

#[test]
#[njutest::integration("postgres")]
fn the_attribute_order_does_not_matter() {}

#[test]
fn declarations_that_cannot_be_read_are_compile_errors() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
