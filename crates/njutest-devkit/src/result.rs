// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Closed branch shapes for assertions that inspect a `Result` or `Option`
//! before extracting its payload.

/// Which branch a [`Result`] inhabits, without discarding either payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultState {
    /// The operation returned its value.
    Returned,
    /// The operation retained a typed refusal.
    Refused,
}

/// Returns the branch of `result` while leaving its payload available to the
/// caller's following exhaustive match.
#[must_use]
pub const fn result_state<T, E>(result: &Result<T, E>) -> ResultState {
    match result {
        Ok(_) => ResultState::Returned,
        Err(_) => ResultState::Refused,
    }
}

/// Which branch an [`Option`] inhabits, without discarding its payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionState {
    /// The optional value is present.
    Present,
    /// The optional value is absent.
    Absent,
}

/// Returns the branch of `option` while leaving its payload available to the
/// caller's following exhaustive match.
#[must_use]
pub const fn option_state<T>(option: Option<&T>) -> OptionState {
    match option {
        Some(_) => OptionState::Present,
        None => OptionState::Absent,
    }
}
