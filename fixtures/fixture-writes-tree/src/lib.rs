// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose test writes into the tree it is being measured in.

/// One more than `n`.
#[must_use]
pub fn next(n: u32) -> u32 {
    n + 1
}

#[cfg(test)]
mod tests {
    use super::next;

    /// Writes a file beside the source on purpose. A run measures one instrumented snapshot, so
    /// a test that writes into it makes every later mutation a measurement of what it wrote.
    #[test]
    fn counting_on_writes_a_note_beside_the_source() {
        std::fs::write("src/note.txt", b"a test was here").expect("the note");
        assert_eq!(next(1), 2);
    }
}
