// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the library holds of its own source, and what it computes.

const WRITTEN: &str = "// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Doubling, in a module the library also holds as text.

/// Twice `value`.
pub fn twice(value: i32) -> i32 {
    value * 2
}
";

#[test]
fn the_library_holds_its_module_as_it_is_written() {
    assert_eq!(fixture_reads_its_source::TWICE_SOURCE, WRITTEN);
    assert_eq!(fixture_reads_its_source::TWICE_BYTES, WRITTEN.as_bytes());
}

#[test]
fn a_test_that_reads_the_module_reads_it_as_it_is_written() {
    assert_eq!(include_str!("../src/twice.rs"), WRITTEN);
}

#[test]
fn three_doubled_is_six() {
    assert_eq!(fixture_reads_its_source::twice::twice(3), 6);
}
