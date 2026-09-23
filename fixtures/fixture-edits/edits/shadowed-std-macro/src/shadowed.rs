// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A module that names its own `vec!`, which an unqualified `vec![]` here then means.

/// Something dropped.
pub struct Gamma;

macro_rules! vec {
    (panics) => {
        impl Drop for Gamma {
            fn drop(&mut self) {
                panic!("a Gamma was dropped")
            }
        }
    };
    () => {};
}

/// A function nothing in the tests calls.
pub fn untouched() {
    vec![panics];
}
