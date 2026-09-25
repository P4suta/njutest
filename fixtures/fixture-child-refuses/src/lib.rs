// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A value handed on to a child process, which a mutation can corrupt on the way.

/// `value`, as it is handed on.
#[must_use]
pub fn handed_on(value: String) -> String {
    value
}
