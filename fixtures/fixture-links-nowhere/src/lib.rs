// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate that type-checks and does not link: it calls a symbol no library supplies.

unsafe extern "C" {
    /// A symbol nothing in this program defines, which only the linker finds out.
    fn a_symbol_no_library_supplies() -> i32;
}

/// The larger of two numbers.
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// What the missing symbol says, which nothing ever gets to ask.
///
/// # Safety
/// There is none: the call is what the linker refuses.
pub unsafe fn ask() -> i32 {
    unsafe { a_symbol_no_library_supplies() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_larger_is_the_larger() {
        assert_eq!(super::max(1, 2), 2);
    }

    #[test]
    fn asking_is_what_the_linker_refuses() {
        assert_eq!(unsafe { super::ask() }, 0);
    }
}
