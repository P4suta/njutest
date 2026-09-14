// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that only passes beside its neighbour, which is what takes a target off test routing.
//!
//! Routing a mutation to the tests that reached it is only sound where running
//! those tests on their own asks the same question as running the target. Here
//! it does not: `beside` waits for `prepares` to have run, and a process that
//! runs `beside` alone fails for a reason that is about the pair rather than
//! about any mutation. A run establishes that once, with nothing active, and
//! then runs every test of this target for every mutation of it.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the test that prepares has run, which stays true once it has.
static PREPARED: AtomicBool = AtomicBool::new(false);

/// The larger of two numbers.
#[must_use]
pub fn larger(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}

/// One more than what it is given.
#[must_use]
pub fn next(a: u32) -> u32 {
    a + 1
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    #[test]
    fn prepares() {
        super::PREPARED.store(true, Ordering::SeqCst);
        assert_eq!(super::larger(1, 2), 2);
    }

    #[test]
    fn beside() {
        let until = Instant::now() + Duration::from_secs(2);
        while !super::PREPARED.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(
            super::PREPARED.load(Ordering::SeqCst),
            "this test passes only beside the one that prepares"
        );
        assert_eq!(super::next(1), 2);
    }
}
