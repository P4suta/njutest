// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The derive that gives a closed fieldless enum its whole list.

#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
enum Phase {
    Red,
    Green,
    Blue,
}

#[test]
fn all_variants_is_generated_from_the_enum_declaration() {
    assert_eq!(Phase::ALL, [Phase::Red, Phase::Green, Phase::Blue]);
}

#[test]
fn declarations_that_cannot_be_read_are_compile_errors() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
