// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One child, its whole tree, and what came back. These tests drive a real
//! shell; they are the reason the runner can be trusted with somebody
//! else's test binary.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;
use std::time::{Duration, Instant};

use rust_mutants::runner::output::{TailBuffer, truncation_notice};
use rust_mutants::runner::{
    Cancel, DEFAULT_OUTPUT_LIMIT, EXIT_CODE_UNAVAILABLE, IO_DRAIN_GRACE, MIN_OUTPUT_LIMIT,
    OUTPUT_TRUNCATED_PREFIX, RunnerError, Spec, run,
};

fn sh(script: &str) -> Spec {
    Spec::new(["sh", "-c", script])
}

#[test]
fn a_child_that_runs_and_fails_is_data_not_an_error() {
    let result = run(
        &sh("echo out; echo err 1>&2; echo more; exit 3"),
        &Cancel::new(),
    );
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.exit_code, 3);
    assert!(!result.timed_out);
    assert_eq!(
        result.output, b"out\nerr\nmore\n",
        "one pipe: the interleaving is the child's own"
    );
    assert!(result.duration > Duration::ZERO);
    assert!(!result.ok());
    assert!(run(&sh("exit 0"), &Cancel::new()).ok());
}

#[test]
fn a_program_that_cannot_start_is_an_error_with_no_exit_status() {
    let result = run(
        &Spec::new(["/nonexistent/rust-mutants-test-program"]),
        &Cancel::new(),
    );
    assert!(
        matches!(result.error, Some(RunnerError::ProcessStartFailed { .. })),
        "{:?}",
        result.error
    );
    assert_eq!(result.exit_code, EXIT_CODE_UNAVAILABLE);
}

#[test]
fn a_spec_without_a_program_is_refused_before_anything_runs() {
    let empty = run(&Spec::new(Vec::<OsString>::new()), &Cancel::new());
    assert!(
        matches!(empty.error, Some(RunnerError::SpecInvalid { .. })),
        "{:?}",
        empty.error
    );
    let blank = run(&Spec::new(["  "]), &Cancel::new());
    assert!(
        matches!(blank.error, Some(RunnerError::SpecInvalid { .. })),
        "{:?}",
        blank.error
    );
}

#[test]
fn a_timeout_kills_the_whole_tree_and_says_so() {
    let temp = tempfile::tempdir().expect("tempdir");
    let marker = temp.path().join("grandchild-finished");
    let script = format!("(sleep 1; touch {}) & sleep 30", marker.display());
    let mut spec = sh(&script);
    spec.timeout = Some(Duration::from_millis(300));
    let started = Instant::now();
    let result = run(&spec, &Cancel::new());
    assert!(result.timed_out);
    assert_eq!(result.exit_code, EXIT_CODE_UNAVAILABLE);
    assert!(result.error.is_none(), "{:?}", result.error);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "did not wait for the sleep"
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "the grandchild was killed with the group");
}

#[test]
fn a_cancellation_kills_the_tree_and_is_not_a_timeout() {
    let cancel = Cancel::new();
    let trigger = cancel.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        trigger.cancel();
    });
    let result = run(&sh("sleep 30"), &cancel);
    handle.join().expect("joins");
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, EXIT_CODE_UNAVAILABLE);
    assert!(result.error.is_none());
    assert!(cancel.is_cancelled());
}

#[test]
fn an_already_cancelled_run_never_starts_the_child() {
    let cancel = Cancel::new();
    cancel.cancel();
    let temp = tempfile::tempdir().expect("tempdir");
    let marker = temp.path().join("ran");
    let result = run(&sh(&format!("touch {}", marker.display())), &cancel);
    assert_eq!(result.exit_code, EXIT_CODE_UNAVAILABLE);
    assert!(result.error.is_none() && !result.timed_out);
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn a_death_by_signal_is_reported_as_128_plus_the_signal() {
    let result = run(&sh("kill -9 $$"), &Cancel::new());
    assert_eq!(result.exit_code, 137);
    assert!(result.error.is_none());
}

#[test]
fn the_environment_and_the_directory_are_exactly_what_the_spec_says() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut spec = sh("echo \"$RM_TEST_VAR|$HOME|$(pwd)\"");
    spec.env = Some(vec![
        (OsString::from("RM_TEST_VAR"), OsString::from("value")),
        (
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_default(),
        ),
    ]);
    spec.dir = Some(temp.path().to_path_buf());
    let result = run(&spec, &Cancel::new());
    let output = String::from_utf8_lossy(&result.output);
    let canonical = temp.path().canonicalize().expect("canonical");
    assert_eq!(
        output.trim(),
        format!("value||{}", canonical.display()),
        "HOME is not inherited"
    );
    let inherited = run(&sh("echo \"$RM_TEST_VAR|$PATH\""), &Cancel::new());
    assert!(
        String::from_utf8_lossy(&inherited.output).starts_with('|'),
        "a None env inherits ours, which has no RM_TEST_VAR"
    );
}

#[test]
fn stdin_is_closed_so_a_reader_does_not_hang() {
    let mut spec = sh("cat; echo done");
    spec.timeout = Some(Duration::from_secs(10));
    let result = run(&spec, &Cancel::new());
    assert!(!result.timed_out);
    assert_eq!(result.output, b"done\n");
}

#[test]
fn output_is_capped_by_keeping_the_tail_and_saying_so() {
    let mut spec = sh("i=0; while [ $i -lt 20000 ]; do echo \"line $i\"; i=$((i+1)); done");
    spec.output_limit = Some(1024);
    let result = run(&spec, &Cancel::new());
    assert_eq!(result.exit_code, 0);
    assert!(result.output.len() <= 1024, "{}", result.output.len());
    let text = String::from_utf8_lossy(&result.output);
    assert!(text.starts_with(OUTPUT_TRUNCATED_PREFIX), "{text}");
    assert!(text.ends_with("line 19999\n"), "{text}");
    let mut tiny = sh("echo small");
    tiny.output_limit = Some(1);
    let result = run(&tiny, &Cancel::new());
    assert_eq!(
        result.output, b"small\n",
        "a limit below the minimum is raised, and small output is kept whole"
    );
}

#[test]
fn a_descendant_holding_the_pipe_does_not_stall_the_run_past_the_drain_grace() {
    let started = Instant::now();
    let result = run(&sh("sleep 20 & exit 0"), &Cancel::new());
    assert_eq!(result.exit_code, 0, "{:?}", result.error);
    assert!(
        started.elapsed() < IO_DRAIN_GRACE + Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn concurrent_runs_share_nothing() {
    let handles: Vec<_> = (0..8i32)
        .map(|i| {
            std::thread::spawn(move || run(&sh(&format!("echo {i}; exit {i}")), &Cancel::new()))
        })
        .collect();
    for (i, handle) in (0..8i32).zip(handles) {
        let result = handle.join().expect("joins");
        assert_eq!(result.exit_code, i);
        assert_eq!(result.output, format!("{i}\n").as_bytes());
    }
}

#[test]
fn the_tail_buffer_keeps_the_last_bytes_and_pays_for_the_notice_out_of_the_budget() {
    let buffer = TailBuffer::new(0);
    assert_eq!(buffer.limit(), MIN_OUTPUT_LIMIT);
    assert_eq!(TailBuffer::new(4096).limit(), 4096);
    assert_eq!(DEFAULT_OUTPUT_LIMIT, 1 << 20);

    let small = TailBuffer::new(300);
    small.write(b"hello ");
    small.write(b"world");
    assert_eq!(small.capture(), b"hello world");

    let capped = TailBuffer::new(300);
    for i in 0..100 {
        capped.write(format!("chunk {i:03}\n").as_bytes());
    }
    let out = capped.capture();
    assert!(out.len() <= 300, "{}", out.len());
    let text = String::from_utf8_lossy(&out);
    let notice = truncation_notice(1000);
    assert!(text.starts_with(&notice), "{text}");
    assert!(text.ends_with("chunk 099\n"), "{text}");
    assert!(notice.starts_with(OUTPUT_TRUNCATED_PREFIX) && notice.contains("1000 bytes"));

    let huge = TailBuffer::new(300);
    huge.write(&vec![b'x'; 5000]);
    let out = huge.capture();
    assert!(out.len() <= 300);
    assert!(out.ends_with(b"xxxx"));
}

#[test]
fn structured_stdout_is_captured_on_its_own_and_stderr_stays_in_the_tail() {
    let mut spec = sh("echo out; echo err >&2; echo out2");
    spec.structured_stdout = Some(1024);
    let result = run(&spec, &Cancel::new());
    assert!(result.ok(), "{result:?}");
    assert_eq!(result.stdout, b"out\nout2\n");
    assert!(!result.stdout_truncated);
    assert_eq!(result.output, b"err\n");
}

#[test]
fn structured_stdout_is_head_capped_and_says_so() {
    let mut spec = sh("yes | head -c 5000");
    spec.structured_stdout = Some(1000);
    let result = run(&spec, &Cancel::new());
    assert!(result.ok(), "{result:?}");
    assert_eq!(result.stdout.len(), 1000);
    assert!(result.stdout.iter().all(|&b| b == b'y' || b == b'\n'));
    assert!(
        result.stdout_truncated,
        "the head is kept and the cut is admitted"
    );
}

#[test]
fn without_the_option_stdout_rides_in_the_combined_output() {
    let result = run(&sh("echo out; echo err >&2"), &Cancel::new());
    assert!(result.stdout.is_empty());
    assert!(!result.stdout_truncated);
    assert_eq!(result.output, b"out\nerr\n");
}

#[test]
fn the_head_buffer_keeps_the_first_bytes_and_admits_the_cut() {
    use rust_mutants::runner::HeadBuffer;
    let head = HeadBuffer::new(5);
    head.write(b"abc");
    head.write(b"defgh");
    head.write(b"ij");
    let (kept, truncated, total) = head.capture();
    assert_eq!(kept, b"abcde");
    assert!(truncated);
    assert_eq!(total, 10);
    let exact = HeadBuffer::new(3);
    exact.write(b"xyz");
    assert_eq!(exact.capture(), (b"xyz".to_vec(), false, 3));
}
