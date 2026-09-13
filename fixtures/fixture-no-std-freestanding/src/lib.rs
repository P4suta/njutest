// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `#![no_std]` library with a panic handler of its own, which is one of the things `std` also supplies.

#![no_std]

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(not(test))]
#[panic_handler]
fn panicked(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
