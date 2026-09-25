// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A comparison whose operands come from a file the build includes and from a value a build script generates.

/// The limit the build script wrote.
const LIMIT: u32 = include!(concat!(env!("OUT_DIR"), "/limit.rs"));

/// The count the included file holds.
fn recorded() -> u32 {
    include_str!("answer.txt").trim().parse().unwrap_or_default()
}

/// Whether the recorded count is over the limit.
pub fn over() -> bool {
    recorded() > LIMIT
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_recorded_count_is_over_the_limit() {
        assert!(super::over());
    }
}
