// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A function a test enters and leaves before its first mutation site, and the shapes an entry marker has to compile in without a warning.

#![deny(warnings, unused)]

/// Half of the number `text` spells, refusing text that spells none before the division is reached.
pub fn halve(text: &str) -> u32 {
    let n: u32 = text.parse().unwrap();
    n / 2
}

/// Stops, which is a body whose type is `!`.
pub fn stop(reason: &str) -> ! {
    panic!("{reason}")
}

/// Does nothing, which is a body with no statement at all.
pub fn nothing() {}

/// One more than `n`, which is a body that is a single expression.
pub fn next(n: u32) -> u32 {
    n + 1
}

/// One more than `n`, in a body that is unsafe to call.
///
/// # Safety
/// Nothing is required of the caller; the function is unsafe so that its body is one.
pub unsafe fn next_unchecked(n: u32) -> u32 {
    n + 1
}

/// Twice `n`, once it is polled.
pub async fn later(n: u32) -> u32 {
    n * 2
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    #[test]
    #[should_panic(expected = "InvalidDigit")]
    fn a_word_is_refused() {
        let _ = super::halve("four");
    }

    #[test]
    fn half_of_four_is_two() {
        assert_eq!(super::halve("4"), 2);
    }

    #[test]
    #[should_panic(expected = "stopped")]
    fn stopping_stops() {
        super::stop("stopped")
    }

    #[test]
    fn nothing_does_nothing() {
        super::nothing();
    }

    #[test]
    fn the_next_of_one_is_two() {
        assert_eq!(super::next(1), 2);
        assert_eq!(unsafe { super::next_unchecked(1) }, 2);
    }

    #[test]
    fn later_is_twice_once_polled() {
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(pin!(super::later(3)).poll(&mut context), Poll::Ready(6));
    }
}
