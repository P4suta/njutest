// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A function whose signature is on a line of its own and whose body opens on the next.

/// What a function is called by the type it returns.
pub trait Named {
    /// The name.
    const NAME: &'static str;
}

impl Named for () {
    const NAME: &'static str = "unit";
}

impl Named for u64 {
    const NAME: &'static str = "u64";
}

/// The name of what `made` returns, read off its type without calling it.
pub fn named<T: Named>(_made: fn() -> T) -> &'static str {
    T::NAME
}

/// Something to name.
pub fn make()
{
    Default::default()
}
