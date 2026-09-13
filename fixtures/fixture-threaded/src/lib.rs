// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that reaches the code on a thread of its own, which nothing can attribute to a test.

/// The larger of two numbers, called from a thread the test spawns.
#[must_use]
pub fn larger(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}

/// One more, called from the test's own thread.
#[must_use]
pub fn next(a: u32) -> u32 {
    a + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_thread_of_its_own_reaches_the_larger() {
        let answer = std::thread::spawn(|| super::larger(1, 2))
            .join()
            .expect("the thread finishes");
        assert_eq!(answer, 2);
    }

    #[test]
    fn the_test_itself_reaches_the_next() {
        assert_eq!(super::next(1), 2);
    }
}
