// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A second target, which measures what the unit tests decline to and never looks at the answer.

#[test]
fn halves_without_looking() {
    std::hint::black_box(fixture_declines::halved(8));
}
