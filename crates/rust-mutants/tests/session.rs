// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a session is asked, in the words a caller says it in. Nothing here starts a toolchain.

use std::time::Duration;

use std::sync::atomic::Ordering;

use rust_mutants::run::Quiet;
use rust_mutants::session::{
    DEFAULT_MUTANT_TIMEOUT, Request, Timeout, TimeoutSource, derived, rewrite_needed,
};

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
        derived(Duration::from_secs(20)),
        Duration::from_secs(100),
        "a mutation that takes five times what the whole target took is one nothing is waiting \
         for, and the multiple is of what this target measured rather than of a number"
    );
    assert_eq!(
        derived(Duration::from_millis(40)),
        Duration::from_secs(30),
        "a fast target would derive a budget shorter than a machine's own noise, and a timeout \
         a slow machine trips is a finding about the machine"
    );
}

#[test]
fn a_configured_timeout_wins_over_auto() {
    assert_eq!(
        Timeout::Fixed(Duration::from_secs(7)).of(Some(Duration::from_secs(20))),
        (Duration::from_secs(7), TimeoutSource::Configured)
    );
    assert_eq!(
        Timeout::Auto.of(Some(Duration::from_secs(20))),
        (Duration::from_secs(100), TimeoutSource::Derived)
    );
    assert_eq!(
        Timeout::Auto.of(None),
        (DEFAULT_MUTANT_TIMEOUT, TimeoutSource::Derived),
        "a target nothing verified has no baseline to be a multiple of, and the run says what \
         it fell back to rather than waiting for ever"
    );
}

#[test]
fn a_confirming_retry_takes_the_quiet_lock_alone() {
    let quiet = Quiet::default();
    let running = std::sync::atomic::AtomicU32::new(0);
    let most = std::sync::atomic::AtomicU32::new(0);
    std::thread::scope(|scope| {
        for _worker in 0..4 {
            let _handle = scope.spawn(|| {
                for _turn in 0..8 {
                    quiet.shared(|| {
                        let now = running.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(1));
                        running.fetch_sub(1, Ordering::SeqCst);
                    });
                    quiet.alone(|| {
                        assert_eq!(
                            running.load(Ordering::SeqCst),
                            0,
                            "a run that has to decide whether a budget really expired measures \
                             with the machine to itself"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    });
                }
            });
        }
    });
    assert!(
        most.load(Ordering::SeqCst) > 1,
        "and shares it the rest of the time"
    );
}
