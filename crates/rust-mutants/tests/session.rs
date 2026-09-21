// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a session is asked, in the words a caller says it in. Nothing here starts a toolchain.

#![expect(
    clippy::expect_used,
    reason = "bounded fixture counters turn an impossible exhaustion into the test failure"
)]

use std::time::Duration;

use std::sync::atomic::Ordering;

use njutest_devkit::thread::ScopedThread;
use rust_mutants::run::Quiet;
use rust_mutants::session::{
    DEFAULT_MUTANT_TIMEOUT, Request, Timeout, TimeoutSource, derived, rewrite_needed,
};

fn increment(counter: &std::sync::atomic::AtomicU32) -> u32 {
    let previous = counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_add(1)
        })
        .expect("the bounded fixture counter has room");
    previous
        .checked_add(1)
        .expect("fetch_update established this successor")
}

fn decrement(counter: &std::sync::atomic::AtomicU32) {
    let previous = counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_sub(1)
        })
        .expect("a fixture worker only leaves after entering");
    assert!(previous > 0, "a fixture worker only leaves after entering");
}

#[test]
fn a_request_built_step_by_step_equals_the_literal_it_replaces() {
    let built = Request::new("abc")
        .with_target("demo/lib/demo")
        .test(Some("tests::one".to_owned()))
        .with_args(vec!["--nocapture".to_owned()])
        .with_timeout(Some(Duration::from_secs(30)));
    assert_eq!(built.mutant, "abc");
    assert_eq!(built.target.as_deref(), Some("demo/lib/demo"));
    assert_eq!(built.test.as_deref(), Some("tests::one"));
    assert_eq!(built.args, vec!["--nocapture".to_owned()]);
    assert_eq!(built.timeout, Some(Duration::from_secs(30)));

    let plain = Request::new("abc");
    assert_eq!(plain.target, None);
    assert_eq!(plain.test, None);
    assert!(plain.args.is_empty());
    assert_eq!(plain.timeout, None);
}

#[test]
fn asking_for_the_whole_target_again_is_one_call_rather_than_a_struct_update() {
    let one = Request::new("abc")
        .with_target("demo/lib/demo")
        .test(Some("tests::one".to_owned()));
    let whole = one.clone().test(None);
    assert_eq!(whole.test, None);
    assert_eq!(
        whole.target, one.target,
        "asking for the whole target changes what runs and nothing else"
    );
    assert_eq!(whole.mutant, one.mutant);
}

#[test]
fn a_file_whose_kept_set_did_not_change_is_not_rewritten_between_rounds() {
    let instrumented = "the file as one round wrote it".to_owned();
    assert!(
        rewrite_needed(None, &instrumented),
        "a file nothing has written yet is a file to write"
    );
    assert!(
        !rewrite_needed(Some(&instrumented), &instrumented),
        "a round condemns mutants of some files and not others, and a file whose live set did \
         not change holds what it already holds"
    );
    assert!(rewrite_needed(
        Some(&instrumented),
        "the file with one guard fewer"
    ));
}

#[test]
fn the_default_timeout_is_five_times_the_baseline_of_the_target_and_never_below_thirty_seconds() {
    assert_eq!(
        derived(Duration::from_secs(20)).expect("twenty seconds times five fits"),
        Duration::from_secs(100),
        "a mutation that takes five times what the whole target took is one nothing is waiting \
         for, and the multiple is of what this target measured rather than of a number"
    );
    assert_eq!(
        derived(Duration::from_millis(40)).expect("forty milliseconds times five fits"),
        Duration::from_secs(30),
        "a fast target would derive a budget shorter than a machine's own noise, and a timeout \
         a slow machine trips is a finding about the machine"
    );
}

#[test]
fn a_configured_timeout_wins_over_auto() {
    assert_eq!(
        Timeout::Fixed(Duration::from_secs(7))
            .of(Some(Duration::from_secs(20)))
            .expect("a configured duration is already bounded"),
        (Duration::from_secs(7), TimeoutSource::Configured)
    );
    assert_eq!(
        Timeout::Auto
            .of(Some(Duration::from_secs(20)))
            .expect("the measured duration fits its multiplier"),
        (Duration::from_secs(100), TimeoutSource::Derived)
    );
    assert_eq!(
        Timeout::Auto
            .of(None)
            .expect("the default duration is bounded"),
        (DEFAULT_MUTANT_TIMEOUT, TimeoutSource::Derived),
        "a target nothing verified has no baseline to be a multiple of, and the run says what \
         it fell back to rather than waiting for ever"
    );
    assert!(
        derived(Duration::MAX).is_err(),
        "an overflowing derived timeout is a refusal, not an infinite-looking fabricated budget"
    );
}

#[test]
fn a_confirming_retry_takes_the_quiet_lock_alone() {
    let quiet = Quiet::default();
    let running = std::sync::atomic::AtomicU32::new(0);
    let most = std::sync::atomic::AtomicU32::new(0);
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _worker in 0..4 {
            let worker = ScopedThread::launch(scope, || {
                for _turn in 0..8 {
                    quiet
                        .shared(|| {
                            let now = increment(&running);
                            most.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(1));
                            decrement(&running);
                        })
                        .expect("shared coordination remains healthy");
                    quiet
                        .alone(|| {
                            assert_eq!(
                                running.load(Ordering::SeqCst),
                                0,
                                "a run that has to decide whether a budget really expired measures \
                             with the machine to itself"
                            );
                            std::thread::sleep(Duration::from_millis(1));
                        })
                        .expect("exclusive coordination remains healthy");
                }
            });
            workers.push(worker);
        }
        for worker in workers {
            worker.join().expect("fixture worker joins");
        }
    });
    assert!(
        most.load(Ordering::SeqCst) > 1,
        "and shares it the rest of the time"
    );
}

/// A reader names a mutation again after they have changed the file, which is the next thing they do.
#[test]
fn a_locator_is_read_from_the_spelling_a_report_prints() {
    use rust_mutants::session::Locator;
    let one = Locator::parse("src/policy/gate.rs:reject:or-to-and").expect("a locator");
    assert_eq!(one.path, "src/policy/gate.rs");
    assert_eq!(one.item, "reject");
    assert_eq!(one.rule, "or-to-and");
    assert_eq!(
        one.line, None,
        "the line is the part a reader adds only when they need it"
    );
    let narrowed =
        Locator::parse("src/policy/gate.rs:Gate::reject:or-to-and@261").expect("a locator");
    assert_eq!(narrowed.item, "Gate::reject");
    assert_eq!(narrowed.line, Some(261));
}

/// An identity is hexadecimal and holds no colon, so nothing that used to resolve stops resolving.
#[test]
fn an_identity_is_not_read_as_a_locator() {
    use rust_mutants::session::Locator;
    assert!(Locator::parse("8aabace61e628bed23e7").is_none());
    assert!(Locator::parse("").is_none());
    assert!(
        Locator::parse("src/lib.rs::or-to-and").is_none(),
        "an empty item names every item rather than one, which is not what a reader meant"
    );
}

/// Whether several survivors of one rule are several findings is a property of the rule.
#[test]
fn a_survivor_of_an_error_path_rule_says_a_path_was_never_taken() {
    let registry = rust_mutants::rule::Registry::canonical();
    for name in [
        "question-to-unwrap",
        "ignore-question-statement",
        "return-ok-default",
    ] {
        let rule = registry.lookup(name).expect("a canonical rule");
        assert!(
            rule.survivor_names_an_unexecuted_path(),
            "{name} replaces the failing half of a fallible expression, so surviving it \
             follows from the failing half never having run"
        );
    }
    for name in ["le-to-lt", "or-to-and", "int-increment"] {
        let rule = registry.lookup(name).expect("a canonical rule");
        assert!(
            !rule.survivor_names_an_unexecuted_path(),
            "{name} is a different boundary on every line it is on, and folding them would \
             hide every one but the first"
        );
    }
}
