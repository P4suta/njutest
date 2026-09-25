// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate that denies every lint the code the engine plants could trip, with the runtime reached from modules that do and do not glob-import their parent.

/// The larger of two numbers, spelled with a comparison a mutant can flip.
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

/// Whether `n` is strictly positive, asked of a module that glob-imports this one.
pub fn is_positive(n: i32) -> bool {
    signs::positive(n)
}

/// Whether `n` is strictly negative, asked of a module two levels down that imports nothing.
pub fn is_negative(n: i32) -> bool {
    signs::negative(n)
}

mod signs {
    use super::*;

    pub(crate) fn positive(n: i32) -> bool {
        max(n, 0) > 0
    }

    pub(crate) fn negative(n: i32) -> bool {
        plain::negative(n)
    }

    mod plain {
        pub(crate) fn negative(n: i32) -> bool {
            n < 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_are_strict() {
        assert!(is_positive(1) && !is_positive(0) && !is_positive(-1));
        assert!(is_negative(-1) && !is_negative(0) && !is_negative(1));
    }

    #[test]
    fn max_picks_the_larger() {
        assert_eq!(max(1, 2), 2);
        assert_eq!(max(3, 2), 3);
    }
}
