// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a proc macro answers.

/// The answer the proc macro gave while this was compiled.
pub fn answer() -> u32 {
    edits_answer::answer!()
}
