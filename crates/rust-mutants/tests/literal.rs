// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Respelling an integer literal: the radix and the suffix it was written with, and the values it cannot move to.

use rust_mutants::syntax::respell_int;

fn literal(text: &str) -> syn::LitInt {
    syn::LitInt::new(text, proc_macro2::Span::call_site())
}

#[test]
fn a_literal_moves_by_one_in_the_radix_it_was_written_in() {
    for (text, up, down) in [
        ("10", Some("11"), Some("9")),
        ("0", Some("1"), None),
        ("0xff", Some("0x100"), Some("0xfe")),
        ("0o17", Some("0o20"), Some("0o16")),
        ("0b1010", Some("0b1011"), Some("0b1001")),
        ("1_000i64", Some("1001i64"), Some("999i64")),
        ("255u8", None, Some("254u8")),
        ("127i8", None, Some("126i8")),
    ] {
        assert_eq!(
            respell_int(&literal(text), 1).as_deref(),
            up,
            "one more than {text}"
        );
        assert_eq!(
            respell_int(&literal(text), -1).as_deref(),
            down,
            "one less than {text}"
        );
    }
}

proptest::proptest! {
    /// A literal moved one way and then the other is the value it started as.
    ///
    /// The spelling may lose an underscore, so what round trips is the value
    /// the compiler reads, not the bytes the author wrote.
    #[test]
    fn respelling_round_trips_plus_and_minus_one(value in 1u64..u64::from(u32::MAX), radix in 0usize..4) {
        let text = match radix {
            0 => format!("{value}"),
            1 => format!("{value:#x}"),
            2 => format!("{value:#o}"),
            _ => format!("{value:#b}"),
        };
        let up = respell_int(&literal(&text), 1).expect("one more");
        let back = respell_int(&literal(&up), -1).expect("one less again");
        proptest::prop_assert_eq!(
            literal(&back).base10_digits().to_owned(),
            literal(&text).base10_digits().to_owned()
        );
    }
}
