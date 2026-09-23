// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How many mutations a run measures at once, and the order the answers come back in.

#![expect(
    clippy::expect_used,
    reason = "bounded fixture counters turn an impossible exhaustion into the test failure"
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use njutest::assure::mutation::quiet_measurement_due;
use njutest::assure::schedule::{CAP, Quiet, measure, workers};
use njutest_devkit::thread::ScopedThread;
use rust_mutants::outcome::Outcome;

fn increment(counter: &AtomicUsize) -> usize {
    let previous = counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_add(1)
        })
        .expect("the bounded fixture counter has room");
    previous
        .checked_add(1)
        .expect("fetch_update established this successor")
}

fn decrement(counter: &AtomicUsize) {
    let previous = counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_sub(1)
        })
        .expect("a fixture worker only leaves after entering");
    assert!(previous > 0, "a worker leaves only after entering");
}

#[test]
fn a_run_that_does_not_say_takes_the_processors_it_has_up_to_the_cap() {
    assert_eq!(workers(0, 0, false).expect("valid count"), 1);
    assert_eq!(workers(0, 1, false).expect("valid count"), 1);
    assert_eq!(workers(0, 2, false).expect("valid count"), 2);
    assert_eq!(workers(0, 3, false).expect("valid count"), 3);
    assert_eq!(workers(0, CAP, false).expect("valid count"), CAP);
    assert_eq!(
        workers(0, CAP.checked_add(1).expect("small fixture"), false).expect("valid count"),
        CAP
    );
    assert_eq!(
        workers(0, 64, false).expect("valid count"),
        CAP,
        "a run that helps itself to every processor starves the tests it is measuring"
    );
}

#[test]
fn a_run_that_says_how_many_workers_it_wants_gets_them() {
    assert_eq!(workers(2, 64, false).expect("valid count"), 2);
    assert_eq!(workers(1, 64, false).expect("valid count"), 1);
    assert_eq!(
        workers(u32::MAX, 1, false).expect("u32 fits every supported target"),
        usize::try_from(u32::MAX).expect("u32 fits every supported target")
    );
}

#[test]
fn one_worker_or_one_item_stays_on_the_calling_thread() {
    let caller = std::thread::current().id();
    let two =
        measure(&["a", "b"], 1, |_at, _item| std::thread::current().id()).expect("workers finish");
    let one =
        measure(&["a"], 4, |_at, _item| std::thread::current().id()).expect("worker finishes");

    assert_eq!(two, [caller, caller]);
    assert_eq!(one, [caller]);
    assert!(
        measure::<u8, u8, _>(&[], 4, |_at, item| *item)
            .expect("empty work finishes")
            .is_empty()
    );
}

#[test]
fn an_exclusive_resource_leaves_one_worker() {
    assert_eq!(
        workers(8, 64, true).expect("valid count"),
        1,
        "a resource only one test may hold at a time is a resource no two tests may hold"
    );
}

#[test]
fn answers_come_back_in_the_order_the_items_came_in() {
    let items: Vec<usize> = (0..16).collect();

    let answers = measure(&items, 4, |at, item| {
        std::thread::sleep(Duration::from_millis(
            u64::try_from(16 - at).expect("fixture index fits"),
        ));
        item * 2
    })
    .expect("workers finish");

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
        let now = increment(&inside);
        most.fetch_max(now, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(2);
        while inside.load(Ordering::SeqCst) < 2 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        decrement(&inside);
        *item
    })
    .expect("workers finish");

    assert_eq!(answers.len(), items.len());
    let most = most.load(Ordering::SeqCst);
    assert!(
        most >= 2,
        "a run that measures one mutation at a time leaves every other processor idle: {most}"
    );
}

#[test]
fn a_parallel_measurement_starts_exactly_the_workers_it_was_given() {
    let items: Vec<usize> = (0..12).collect();
    let inside = AtomicUsize::new(0);
    let most = AtomicUsize::new(0);
    let entered = AtomicUsize::new(0);

    let answers = measure(&items, 3, |_at, item| {
        let now = increment(&inside);
        most.fetch_max(now, Ordering::SeqCst);
        let entered_now = increment(&entered);
        assert!(entered_now > 0, "the fixture counter records an entry");
        let until = Instant::now() + Duration::from_secs(2);
        while entered.load(Ordering::SeqCst) < 3 && Instant::now() < until {
            std::thread::yield_now();
        }
        decrement(&inside);
        *item
    })
    .expect("workers finish");

    assert_eq!(answers, items);
    assert_eq!(most.load(Ordering::SeqCst), 3);
}

#[test]
fn a_measurement_given_the_machine_has_nothing_this_run_started_beside_it() {
    let quiet = Quiet::default();
    let inside = AtomicUsize::new(0);
    let alone_ran = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        let mut workers = Vec::with_capacity(4);
        for _ in 0..4 {
            let (quiet, inside) = (&quiet, &inside);
            workers.push(ScopedThread::launch(scope, move || {
                for _ in 0..500 {
                    quiet
                        .shared(|| {
                            let inside_now = increment(inside);
                            assert!(inside_now > 0, "the shared worker entered");
                            std::thread::yield_now();
                            decrement(inside);
                        })
                        .expect("shared state remains sound");
                }
            }));
        }
        let (quiet, inside, alone_ran) = (&quiet, &inside, &alone_ran);
        let exclusive = ScopedThread::launch(scope, move || {
            for _ in 0..100 {
                quiet
                    .alone(|| {
                        assert_eq!(
                            inside.load(Ordering::SeqCst),
                            0,
                            "a budget is five times a duration measured on this machine, and \
                         the measurement that decides whether it really expired is taken \
                         with nothing else on it"
                        );
                        let ran = increment(alone_ran);
                        assert!(ran <= 100, "the bounded fixture ran at most 100 times");
                    })
                    .expect("exclusive state remains sound");
            }
        });
        exclusive.join().expect("the exclusive worker finishes");
        for worker in workers {
            worker.join().expect("a shared worker finishes");
        }
    });

    assert_eq!(alone_ran.load(Ordering::SeqCst), 100);
}

#[test]
fn only_an_expired_budget_buys_a_quiet_measurement_and_a_stopped_run_buys_nothing() {
    assert!(quiet_measurement_due(Outcome::Waited, false));
    assert!(
        !quiet_measurement_due(Outcome::Waited, true),
        "a run that has been asked to stop starts nothing else"
    );
    for outcome in Outcome::ALL {
        assert_eq!(
            quiet_measurement_due(outcome, false),
            outcome == Outcome::Waited,
            "a mutation the tests answered has been answered: {outcome:?}"
        );
    }
}
