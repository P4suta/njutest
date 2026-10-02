// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One actual preparation per module per compatible engine: what a process holds, and what stays its own.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use njutest_devkit::temporary::CacheDirectory;
use rust_mutants_sealed::{
    Compilation, CompilerTier, Interrupt, ModuleOwner, SealedError, SealedRunner, SealedStop, Spent,
};

use crate::common::{command, invocation, uninterrupted};

/// A WASI command whose invocation prints `mark`, whose bytes differ from every other mark's.
fn printing(mark: &str) -> Vec<u8> {
    command(
        &[],
        &format!("(data (i32.const 100) \"{mark}\")"),
        &format!("(call $emit (i32.const 100) (i32.const {}))", mark.len()),
    )
}

/// A WASI command that spins until something stops it.
fn spinning() -> Vec<u8> {
    command(&[], "", "(loop $again (br $again))")
}

/// One runner's counters with its compilation, which a preparation was asked of it.
fn spent_of(runner: &SealedRunner) -> (Spent, Compilation) {
    let spent = runner
        .spent()
        .expect("a preparation was asked of this runner");
    let compilation = spent.compilation.expect("the preparation is measured");
    (spent, compilation)
}

/// Prints keyed physical work observed directly by the runner for the retained measurement receipt.
fn measured(case: &str, runner: &SealedRunner, bytes: &[u8]) {
    eprintln!(
        "module-work case={case} module={} configuration={} spent={}",
        rust_mutants_sealed::SealedDigest::of(bytes),
        runner.configuration(),
        serde_json::to_string(&runner.spent()).expect("the actual counters serialize")
    );
}

/// Measures cold and repeated real preparations while retaining the runner through the full comparison.
fn measure_repeat(
    bytes: &[u8],
) -> (
    CacheDirectory,
    SealedRunner,
    rust_mutants_sealed::Transcript,
) {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-owner-").expect("an owned cold cache domain");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the measured runner");
    let first = runner.prepare(bytes).expect("a cold actual module");
    let control = first
        .invoke(&invocation(), &uninterrupted())
        .expect("actual cold execution");
    assert_eq!(control.stop(), SealedStop::Returned);
    assert_eq!(control.stdout().bytes(), b"measured");
    measured("physical-cold", &runner, bytes);
    let before_repeat = spent_of(&runner).1.duration_ns;
    let repeat = runner.prepare(bytes).expect("the repeated actual module");
    let repeated = repeat
        .invoke(&invocation(), &uninterrupted())
        .expect("actual repeated execution");
    assert_eq!(
        repeated, control,
        "a preparation request preserves the actual observation"
    );
    measured("physical-repeat", &runner, bytes);
    eprintln!(
        "module-work repeat-added-preparation-ns={}",
        spent_of(&runner)
            .1
            .duration_ns
            .checked_sub(before_repeat)
            .expect("monotonic measured work")
    );

    drop(first);
    drop(repeat);
    (directory, runner, control)
}

#[test]
fn physical_module_preparation_cases_execute_actual_guests() {
    let modules = ModuleOwner::default();
    let bytes = printing("measured");
    let (directory_owner, retained_runner, control) = measure_repeat(&bytes);
    let directory = CacheDirectory::make("sm-").expect("cache");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the concurrent runner");
    let start = std::sync::Barrier::new(4);
    let answers = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_request| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    start.wait();
                    runner
                        .prepare(&bytes)
                        .expect("the actual concurrent module")
                        .invoke(&invocation(), &uninterrupted())
                })
            })
            .collect();
        let mut answers = Vec::with_capacity(workers.len());
        for worker in workers {
            answers.push(worker.join());
        }
        answers
    });
    for answer in answers {
        assert_eq!(
            answer
                .expect("every worker is joined")
                .expect("actual concurrent execution"),
            control
        );
    }
    measured("physical-same-runner-concurrent", &runner, &bytes);

    let directory =
        CacheDirectory::make("sealed-owner-").expect("an owned separate-runner cold cache domain");
    let start = std::sync::Barrier::new(3);
    let answers = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_request| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    let runner = SealedRunner::cached(
                        &modules,
                        Duration::from_secs(60),
                        &crate::common::cache_for(directory.path()),
                    )
                    .expect("a separate compatible runner");
                    start.wait();
                    let answer = runner
                        .prepare(&bytes)
                        .expect("the separate runner's actual module")
                        .invoke(&invocation(), &uninterrupted());
                    (runner.spent().expect("actual measured work"), answer)
                })
            })
            .collect();
        let mut answers = Vec::with_capacity(workers.len());
        for worker in workers {
            answers.push(worker.join());
        }
        answers
    });
    for answer in answers {
        let (spent, transcript) = answer.expect("every separate runner is joined");
        assert_eq!(
            transcript.expect("the separate runner actually executes"),
            control
        );
        eprintln!(
            "module-work case=physical-separate-runners-concurrent module={} spent={}",
            rust_mutants_sealed::SealedDigest::of(&bytes),
            serde_json::to_string(&spent).expect("the actual counters serialize")
        );
    }
    drop(retained_runner);
    drop(directory_owner);
}

#[test]
fn a_repeated_request_of_one_module_is_answered_by_what_the_process_holds() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the runner starts");
    let bytes = printing("once");
    let first = runner.prepare(&bytes).expect("the command is valid");
    measured("cold", &runner, &bytes);
    let (spent, compilation) = spent_of(&runner);
    assert_eq!(
        (spent.compiles, compilation.hits, compilation.misses),
        (1, 0, 1),
        "the first request finds nothing held and prepares the module"
    );
    let worked = compilation.duration_ns;
    let second = runner
        .prepare(&bytes)
        .expect("the same command is valid again");
    measured("repeat", &runner, &bytes);
    let (spent, compilation) = spent_of(&runner);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            compilation.process
        ),
        (2, 1, 1, Some(1)),
        "one request prepared the module and the second is answered by what the process holds"
    );
    assert_eq!(
        compilation.duration_ns, worked,
        "an answer from the process's held module adds no preparation work"
    );
    assert_eq!(
        first.digest(),
        second.digest(),
        "the same bytes are the same module"
    );
    for module in [first, second] {
        let transcript = module
            .invoke(&invocation(), &uninterrupted())
            .expect("an answer about the guest");
        assert_eq!(transcript.stop(), SealedStop::Returned);
        assert_eq!(transcript.stdout().bytes(), b"once");
    }
}

#[test]
fn a_separate_compatible_runner_is_answered_by_what_the_process_holds() {
    let modules = ModuleOwner::default();
    let cloned = modules.clone();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let first = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("a runner starts");
    let second = SealedRunner::cached(
        &cloned,
        Duration::from_millis(500),
        &crate::common::cache_for(directory.path()),
    )
    .expect("another runner starts");
    let bytes = printing("both");
    first.prepare(&bytes).expect("the command is valid");
    let (_, compilation) = spent_of(&first);
    assert_eq!(
        (compilation.hits, compilation.misses),
        (0, 1),
        "the first runner prepares the module"
    );
    let module = second.prepare(&bytes).expect("the command is valid again");
    measured("separate-runner", &second, &bytes);
    let (spent, compilation) = spent_of(&second);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            compilation.process
        ),
        (1, 1, 0, Some(1)),
        "a second runner of the same tier and cache directory prepares nothing: the process holds the module"
    );
    let transcript = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest on the shared engine");
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"both");
    assert_eq!(
        first.configuration(),
        second.configuration(),
        "compatible runners are one engine configuration"
    );
}

#[test]
fn a_module_survives_the_runner_that_prepared_it() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    {
        let brief = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &crate::common::cache_for(directory.path()),
        )
        .expect("a runner starts");
        brief
            .prepare(&printing("kept"))
            .expect("the command is valid");
    }
    let later = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("a runner starts");
    let module = later
        .prepare(&printing("kept"))
        .expect("the command is valid");
    let (spent, compilation) = spent_of(&later);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            compilation.process
        ),
        (1, 1, 0, Some(1)),
        "the process holds the module beyond the runner that first prepared it"
    );
    let transcript = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"kept");
}

#[test]
fn concurrent_requests_of_one_module_prepare_once() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = Arc::new(
        SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &crate::common::cache_for(directory.path()),
        )
        .expect("the runner starts"),
    );
    let bytes = printing("many");
    let answers = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_worker| {
                let runner = Arc::clone(&runner);
                let bytes = bytes.clone();
                njutest_devkit::thread::ScopedThread::launch(scope, move || {
                    let module = runner.prepare(&bytes).expect("the command is valid");
                    module
                        .invoke(&invocation(), &uninterrupted())
                        .expect("an answer about the guest")
                })
            })
            .collect();
        let mut answers = Vec::with_capacity(workers.len());
        for worker in workers {
            answers.push(worker.join());
        }
        answers
    });
    for answer in answers {
        let transcript = answer.expect("every worker is joined");
        assert_eq!(transcript.stop(), SealedStop::Returned);
        assert_eq!(transcript.stdout().bytes(), b"many");
    }
    let (spent, compilation) = spent_of(&runner);
    measured("same-runner-concurrent", &runner, &bytes);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            compilation.process
        ),
        (4, 3, 1, Some(3)),
        "four concurrent requests make one actual preparation and share its answer"
    );
}

#[test]
fn concurrent_cold_requests_through_separate_runners_prepare_once() {
    let modules = ModuleOwner::default();
    let answers: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_worker| {
                njutest_devkit::thread::ScopedThread::launch(scope, || {
                    let runner = SealedRunner::new(&modules, Duration::from_secs(60))
                        .expect("a runner starts");
                    let module = runner
                        .prepare(&printing("apart"))
                        .expect("the command is valid");
                    let transcript = module
                        .invoke(&invocation(), &uninterrupted())
                        .expect("an answer about the guest");
                    (runner.spent().expect("the request is counted"), transcript)
                })
            })
            .collect();
        let mut answers = Vec::with_capacity(workers.len());
        for worker in workers {
            answers.push(worker.join());
        }
        answers
    })
    .into_iter()
    .map(|answer| answer.expect("every worker is joined"))
    .collect();
    let misses: u64 = answers
        .iter()
        .map(|(spent, _transcript)| spent.compilation.expect("each request is measured").misses)
        .sum();
    let held = answers
        .iter()
        .filter(|(spent, _transcript)| {
            spent.compilation.expect("each request is measured").process == Some(1)
        })
        .count();
    assert_eq!(
        (misses, held),
        (1, 2),
        "three runners asking cold at once make one actual preparation, and the process answers the other two"
    );
    for (spent, transcript) in &answers {
        eprintln!(
            "module-work case=separate-runners-concurrent module={} spent={}",
            rust_mutants_sealed::SealedDigest::of(&printing("apart")),
            serde_json::to_string(spent).expect("the actual counters serialize")
        );
        assert_eq!(spent.compiles, 1, "each runner asked once");
        assert_eq!(transcript.stop(), SealedStop::Returned);
        assert_eq!(transcript.stdout().bytes(), b"apart");
    }
}

#[test]
fn a_module_already_held_answers_every_invocation_afresh() {
    let modules = ModuleOwner::default();
    let runner = SealedRunner::new(&modules, Duration::from_secs(60)).expect("the runner starts");
    let module = runner
        .prepare(&printing("anew"))
        .expect("the command is valid");
    let alone = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    for _attempt in 0..2 {
        let again = module
            .invoke(&invocation(), &uninterrupted())
            .expect("an answer about the guest");
        assert_eq!(
            again, alone,
            "every invocation is a fresh store over the same held module"
        );
    }
    let spent = runner.spent().expect("the held module was counted");
    assert_eq!(spent.compiles, 1, "invocations prepare nothing");
}

#[test]
fn changed_bytes_prepare_again() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the runner starts");
    let one = runner.prepare(&printing("ones")).expect("a valid command");
    let other = runner
        .prepare(&printing("twos"))
        .expect("another valid command");
    let (spent, compilation) = spent_of(&runner);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            compilation.process
        ),
        (2, 0, 2, None),
        "changed bytes are a different module, whatever the process holds"
    );
    assert_ne!(one.digest(), other.digest());
    let first = one
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    let second = other
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    assert_eq!(first.stdout().bytes(), b"ones");
    assert_eq!(second.stdout().bytes(), b"twos");
}

#[test]
fn a_changed_tier_prepares_its_own_but_an_operational_directory_does_not() {
    let modules = ModuleOwner::default();
    let held = CacheDirectory::make("sealed-owner-").expect("one cache directory");
    let elsewhere = CacheDirectory::make("sealed-owner-").expect("another cache directory");
    let speed = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(held.path()),
    )
    .expect("a runner starts");
    let plain = SealedRunner::with_compiler(
        &modules,
        Duration::from_secs(60),
        CompilerTier::Unoptimized,
        Some(&crate::common::cache_for(held.path())),
    )
    .expect("another runner starts");
    let apart = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(elsewhere.path()),
    )
    .expect("a third runner starts");
    let bytes = printing("tier");
    speed.prepare(&bytes).expect("a valid command");
    plain.prepare(&bytes).expect("a valid command");
    apart.prepare(&bytes).expect("a valid command");
    for runner in [&speed, &plain] {
        let (spent, compilation) = spent_of(runner);
        assert_eq!(
            (
                spent.compiles,
                compilation.hits,
                compilation.misses,
                compilation.process
            ),
            (1, 0, 1, None),
            "a different compiler tier requires a separate actual preparation"
        );
    }
    let (spent, compilation) = spent_of(&apart);
    assert_eq!(
        (spent.compiles, compilation.attempts, compilation.process),
        (1, None, Some(1)),
        "changing an operational cache directory cannot cause another physical preparation"
    );
    assert_eq!(speed.configuration(), apart.configuration());
    assert_ne!(
        speed.configuration(),
        plain.configuration(),
        "a different tier is a different engine configuration"
    );
}

#[test]
fn a_failed_preparation_is_counted_and_holds_nothing() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the runner starts");
    for _attempt in 0..2 {
        assert!(
            runner.prepare(b"not webassembly").is_err(),
            "the bytes are no module"
        );
    }
    let spent = runner.spent().expect("the attempts are counted");
    assert_eq!(
        (spent.compiles, spent.failures),
        (0, Some(2)),
        "each failed request is counted, and no answer is claimed for it"
    );
    assert!(
        spent.compilation.is_none(),
        "a refusal before `Module::new` attempts no preparation, so no work or attempt is claimed"
    );
    let outside = wat::parse_str(
        "(module\n\
         (import \"wasi_snapshot_preview1\" \"not_a_table_function\" (func))\n\
         (memory (export \"memory\") 1)\n\
         (func (export \"_start\")))",
    )
    .expect("valid WAT");
    for _request in 0..2 {
        assert!(
            matches!(
                runner.prepare(&outside),
                Err(SealedError::ModuleImport { .. })
            ),
            "a real compiled core module is refused by the host's table"
        );
    }
    let (spent, compilation) = spent_of(&runner);
    assert_eq!(
        (
            spent.compiles,
            spent.failures,
            compilation.hits,
            compilation.misses,
            compilation.attempts,
            compilation.failed_cold,
            compilation.failed_disk
        ),
        (0, Some(4), 0, 0, Some(2), Some(1), Some(1)),
        "a refused interface counts real cold and disk preparation attempts without holding success"
    );
    assert!(
        compilation.duration_ns > 0,
        "the refused attempt's real work stands, never an invented zero"
    );
    let module = runner
        .prepare(&printing("late"))
        .expect("a failure holds nothing for later requests");
    let (spent, compilation) = spent_of(&runner);
    assert_eq!(
        (
            spent.compiles,
            compilation.hits,
            compilation.misses,
            spent.failures,
            compilation.attempts
        ),
        (1, 0, 1, Some(4), Some(3)),
        "a failed attempt never becomes a held answer"
    );
    let transcript = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"late");
}

#[test]
fn failed_module_new_attempts_keep_their_actual_work() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the runner starts");
    let invalid = wat::parse_str(
        "(module (memory (export \"memory\") 1) (func (export \"_start\") (i32.add)))",
    )
    .expect("syntactically valid WAT with an invalid operand stack");
    for _request in 0..2 {
        assert!(
            matches!(runner.prepare(&invalid), Err(SealedError::Compile { .. })),
            "the shape passes but the actual Module::new validator refuses the code"
        );
    }
    let (spent, compilation) = spent_of(&runner);
    eprintln!(
        "failed-module key={} configuration={} spent={spent:?}",
        rust_mutants_sealed::SealedDigest::of(&invalid),
        runner.configuration()
    );
    assert_eq!(
        (
            spent.compiles,
            spent.failures,
            compilation.attempts,
            compilation.failed_cold,
            compilation.failed_disk
        ),
        (0, Some(2), Some(2), Some(2), None)
    );
    assert!(
        compilation.duration_ns > 0,
        "observed failed work is retained"
    );
    let module = runner
        .prepare(&printing("valid"))
        .expect("a succeeding control");
    let transcript = module
        .invoke(&invocation(), &uninterrupted())
        .expect("actual execution");
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"valid");
}

#[test]
fn dropping_one_runner_stops_no_ticker_another_runner_uses() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    {
        let brief = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &crate::common::cache_for(directory.path()),
        )
        .expect("a runner starts");
        brief
            .prepare(&printing("tick"))
            .expect("the command is valid");
    }
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_millis(100),
        &crate::common::cache_for(directory.path()),
    )
    .expect("another runner starts");
    let spinning = runner
        .prepare(&spinning())
        .expect("the spinning command is valid");
    let mut endless = invocation();
    endless.fuel = u64::MAX;
    match spinning.invoke(&endless, &uninterrupted()) {
        Err(SealedError::WatchdogExpired { limit }) => {
            assert_eq!(limit, Duration::from_millis(100));
        }
        other => panic!("the shared ticker still stands behind every later runner: {other:?}"),
    }
}

#[test]
fn each_runner_keeps_its_own_watchdog_on_a_shared_engine() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let strict = SealedRunner::cached(
        &modules,
        Duration::from_millis(100),
        &crate::common::cache_for(directory.path()),
    )
    .expect("a strict runner starts");
    let patient = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("a patient runner starts");
    let spinning = strict
        .prepare(&spinning())
        .expect("the spinning command is valid");
    let quick = patient.prepare(&printing("calm")).expect("a valid command");
    let mut endless = invocation();
    endless.fuel = u64::MAX;
    let answer = spinning.invoke(&endless, &uninterrupted());
    assert!(
        matches!(answer, Err(SealedError::WatchdogExpired { .. })),
        "the strict runner's own wall clock stops its own guest: {answer:?}"
    );
    let transcript = quick
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"calm");
}

#[test]
fn an_interrupt_is_one_invocation_s_and_no_other_s() {
    let modules = ModuleOwner::default();
    let directory = CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
    let runner = SealedRunner::cached(
        &modules,
        Duration::from_secs(60),
        &crate::common::cache_for(directory.path()),
    )
    .expect("the runner starts");
    let module = runner
        .prepare(&printing("quite"))
        .expect("the command is valid");
    let raised = Arc::new(AtomicBool::new(true));
    let stopped = module.invoke(&invocation(), &Interrupt::of(vec![Arc::clone(&raised)]));
    assert!(
        matches!(stopped, Err(SealedError::Interrupted)),
        "one invocation's interrupt stops that invocation alone: {stopped:?}"
    );
    let fine = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    assert_eq!(fine.stop(), SealedStop::Returned);
    assert_eq!(fine.stdout().bytes(), b"quite");
}
