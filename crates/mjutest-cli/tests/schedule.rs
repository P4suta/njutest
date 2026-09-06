// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How many mutations a run measures at once, and the order the answers come back in.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mjutest_cli::assure::schedule::{CAP, measure, workers};

#[test]
fn a_run_that_does_not_say_takes_the_processors_it_has_up_to_the_cap() {
    assert_eq!(workers(0, 1, false), 1);
    assert_eq!(workers(0, 3, false), 3);
    assert_eq!(
        workers(0, 64, false),
        CAP,
        "a run that helps itself to every processor starves the tests it is measuring"
    );
}

#[test]
fn a_run_that_says_how_many_workers_it_wants_gets_them() {
    assert_eq!(workers(2, 64, false), 2);
    assert_eq!(workers(1, 64, false), 1);
}

#[test]
fn an_exclusive_resource_leaves_one_worker() {
    assert_eq!(
        workers(8, 64, true),
        1,
        "a resource only one test may hold at a time is a resource no two tests may hold"
    );
}

#[test]
fn answers_come_back_in_the_order_the_items_came_in() {
    let items: Vec<usize> = (0..16).collect();

    let answers = measure(&items, 4, |at, item| {
        std::thread::sleep(Duration::from_millis(u64::try_from(16 - at).unwrap_or(0)));
        item * 2
    });

    assert_eq!(
        answers,
        items.iter().map(|item| item * 2).collect::<Vec<usize>>(),
        "what a report says cannot depend on which worker finished first"
    );
}

#[test]
fn more_than_one_item_is_measured_at_a_time() {
    let items: Vec<usize> = (0..8).collect();
    let inside = AtomicUsize::new(0);
    let most = AtomicUsize::new(0);

    let answers = measure(&items, 4, |_at, item| {
        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
        most.fetch_max(now, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(2);
        while inside.load(Ordering::SeqCst) < 2 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        inside.fetch_sub(1, Ordering::SeqCst);
        *item
    });

    assert_eq!(answers.len(), items.len());
    let most = most.load(Ordering::SeqCst);
    assert!(
        most >= 2,
        "a run that measures one mutation at a time leaves every other processor idle: {most}"
    );
}
