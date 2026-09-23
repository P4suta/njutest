// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every enum that spells its wire name twice, and that the two agree.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a value this crate declares that will not serialize leaves nothing to compare"
)]

/// What serde writes for `value`, which is the name a report and a trace carry.
fn serialized<T: serde::Serialize + std::fmt::Debug>(value: &T) -> String {
    let encoded = serde_json::to_value(value).expect("a fieldless variant serializes");
    match encoded.as_str() {
        Some(name) => name.to_owned(),
        None => panic!("{value:?} serializes as a string"),
    }
}

/// Requires every variant's `name()` to be the name serde writes.
macro_rules! agrees {
    ($($enum:path),* $(,)?) => {
        $({
            use $enum as Held;
            for one in Held::ALL {
                assert_eq!(
                    one.name(),
                    serialized(&one),
                    "{} spells its wire name twice: once in #[serde(rename_all)] and once \
                     in name(). A variant whose Rust name does not transform to the word \
                     that was meant writes one spelling into the JSON report and the \
                     other into the trace note and the console, which is two names for \
                     one fact in one run's evidence",
                    stringify!($enum)
                );
            }
        })*
    };
}

#[test]
fn every_wire_name_a_value_carries_is_the_one_serde_writes() {
    agrees!(
        rust_mutants::outcome::Outcome,
        rust_mutants::session::Proof,
        rust_mutants::session::Granularity,
        rust_mutants::session::Fallback,
        rust_mutants::syntax::SkipReason,
        rust_mutants::run::FindingKind,
        rust_mutants::run::NotRunReason,
    );
}
