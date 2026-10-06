// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library whose tests the sealed host runs one at a time through libtest, built by the test that runs it with `cargo test --no-run --target wasm32-wasip1`.

/// Adds two numbers, which is what every test here checks.
#[must_use]
pub const fn add(left: u32, right: u32) -> u32 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn passes() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    fn panics() {
        assert_eq!(add(2, 2), 5, "the failing test panicked on purpose");
    }

    #[test]
    fn returns_an_error() -> Result<(), std::io::Error> {
        if add(2, 2) == 4 {
            return Err(std::io::Error::other("the failing test returned an error on purpose"));
        }
        Ok(())
    }
}
