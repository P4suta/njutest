// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The shapes a modern crate is written in, each with a test that notices a mutation of it.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

/// What went wrong.
#[derive(Debug, PartialEq, Eq)]
pub enum BoundError {
    /// The value is past the bound.
    TooLarge,
}

/// Whether the value is within the inclusive bound.
pub fn within(value: i32, bound: i32) -> bool {
    value <= bound
}

/// The value, or why it is not within the bound.
pub fn checked(value: i32, bound: i32) -> Result<i32, BoundError> {
    if within(value, bound) {
        Ok(value)
    } else {
        Err(BoundError::TooLarge)
    }
}

/// The values within the bound, doubled.
pub fn doubled_within(values: &[i32], bound: i32) -> Vec<i32> {
    values
        .iter()
        .filter(|one| within(**one, bound))
        .map(|one| one * 2)
        .collect()
}

/// The first value past the bound.
pub fn first_past(values: &[i32], bound: i32) -> Option<i32> {
    values.iter().copied().find(|one| !within(*one, bound))
}

/// The total of everything the iterator yields.
pub fn total(values: impl IntoIterator<Item = i32>) -> i32 {
    let mut sum = 0;
    for one in values {
        sum += one;
    }
    sum
}

/// The largest of the values, by the comparison the caller gives.
pub fn largest<T: Copy, F: Fn(T, T) -> bool>(values: &[T], greater: F) -> Option<T> {
    let mut best: Option<T> = None;
    for one in values {
        best = match best {
            Some(current) if greater(current, *one) => Some(current),
            _ => Some(*one),
        };
    }
    best
}

/// A counter that says how far it has come.
pub struct Counter {
    at: u32,
    limit: u32,
}

impl Counter {
    /// A counter that stops at `limit`.
    #[must_use]
    pub const fn new(limit: u32) -> Self {
        Self { at: 0, limit }
    }

    /// Whether the counter has more to give.
    #[must_use]
    pub fn more(&self) -> bool {
        self.at < self.limit
    }
}

impl Iterator for Counter {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if !self.more() {
            return None;
        }
        self.at += 1;
        Some(self.at)
    }
}

/// What a shape can say about itself.
pub trait Named {
    /// How many letters the name has.
    fn letters(&self) -> usize {
        self.name().len()
    }

    /// The name.
    fn name(&self) -> &str;
}

impl Named for Counter {
    fn name(&self) -> &str {
        "counter"
    }
}

/// The bound, awaited.
pub async fn checked_later(value: i32, bound: i32) -> Result<i32, BoundError> {
    checked(value, bound)
}

/// The sum of two awaited bounds, or the first error.
pub async fn both(value: i32, bound: i32) -> Result<i32, BoundError> {
    let first = checked_later(value, bound).await?;
    let second = checked_later(value + 1, bound).await?;
    Ok(first + second)
}

/// Runs a future to completion on this thread, which is all a test of one needs.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoundError, Counter, Named, block_on, both, checked, doubled_within, first_past, largest,
        total, within,
    };

    #[test]
    fn the_bound_is_inclusive() {
        assert!(within(3, 3));
        assert!(!within(4, 3));
    }

    #[test]
    fn a_value_past_the_bound_is_an_error() {
        assert_eq!(checked(3, 3), Ok(3));
        assert_eq!(checked(4, 3), Err(BoundError::TooLarge));
    }

    #[test]
    fn only_the_values_within_are_doubled() {
        assert_eq!(doubled_within(&[1, 2, 3, 4], 2), vec![2, 4]);
    }

    #[test]
    fn the_first_past_the_bound_is_found() {
        assert_eq!(first_past(&[1, 2, 3], 2), Some(3));
        assert_eq!(first_past(&[1, 2], 2), None);
    }

    #[test]
    fn the_total_is_the_sum() {
        assert_eq!(total([1, 2, 3]), 6);
    }

    #[test]
    fn the_largest_is_the_one_nothing_beats() {
        assert_eq!(largest(&[1, 5, 3], |a, b| a > b), Some(5));
        assert_eq!(largest::<i32, _>(&[], |a, b| a > b), None);
    }

    #[test]
    fn a_counter_stops_at_its_limit() {
        assert_eq!(Counter::new(3).take(5).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(
            Counter::new(0).take(5).count(),
            0,
            "bounded on purpose: a mutation that stops the counter stopping must fail rather \
             than run for ever"
        );
    }

    #[test]
    fn a_name_is_as_long_as_it_reads() {
        assert_eq!(Counter::new(1).letters(), 7);
    }

    #[test]
    fn both_awaits_each_bound_in_turn() {
        assert_eq!(block_on(both(1, 3)), Ok(3));
        assert_eq!(
            block_on(both(4, 3)),
            Err(BoundError::TooLarge),
            "the first bound is the one that fails here, and the second one there, so each \
             question mark is a place a test has been past"
        );
        assert_eq!(block_on(both(3, 3)), Err(BoundError::TooLarge));
    }
}
