// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The host's semantics function by function, driven by hand-written WASI commands.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rust_mutants_sealed::{
    Interrupt, OverlayState, Preopens, Refusal, RefusalReason, SealedError, SealedRunner,
    SealedStop, TrapKind, WasiFunction,
};

use crate::common::{command, emitted, invocation, run, runner, snapshot, tree, uninterrupted};

#[test]
fn a_socket_call_is_answered_notsup_and_recorded_as_a_refusal() {
    let bytes = command(
        &["sock_accept", "sock_recv", "sock_send", "sock_shutdown"],
        "",
        "(call $expect (call $sock_accept (i32.const 3) (i32.const 0) (i32.const 64)) (i32.const 58))
         (call $expect (call $sock_recv (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 64) (i32.const 68)) (i32.const 58))
         (call $expect (call $sock_send (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 64)) (i32.const 58))
         (call $expect (call $sock_shutdown (i32.const 3) (i32.const 3)) (i32.const 58))
         (call $expect (call $sock_shutdown (i32.const 9) (i32.const 3)) (i32.const 58))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    let refused = |function, count| Refusal {
        function,
        reason: RefusalReason::Network,
        count,
    };
    assert_eq!(
        transcript.refusals(),
        [
            refused(WasiFunction::SockAccept, 1),
            refused(WasiFunction::SockRecv, 1),
            refused(WasiFunction::SockSend, 1),
            refused(WasiFunction::SockShutdown, 2),
        ]
    );
}

#[test]
fn raising_a_signal_and_making_a_link_are_refused_and_recorded() {
    let bytes = command(
        &["proc_raise", "path_link", "path_symlink"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $proc_raise (i32.const 9)) (i32.const 52))
         (call $expect (call $path_link (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 3) (i32.const 100) (i32.const 8)) (i32.const 58))
         (call $expect (call $path_symlink (i32.const 100) (i32.const 8) (i32.const 3) (i32.const 100) (i32.const 8)) (i32.const 58))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    let reasons: Vec<(WasiFunction, RefusalReason)> = transcript
        .refusals()
        .iter()
        .map(|refusal| (refusal.function, refusal.reason))
        .collect();
    assert_eq!(
        reasons,
        [
            (WasiFunction::PathLink, RefusalReason::Link),
            (WasiFunction::PathSymlink, RefusalReason::Link),
            (WasiFunction::ProcRaise, RefusalReason::Signal),
        ]
    );
}

#[test]
fn a_path_that_leaves_its_directory_is_refused_notcapable() {
    let bytes = command(
        &["path_open", "path_filestat_get"],
        "(data (i32.const 100) \"../outside\")
         (data (i32.const 120) \"/etc/passwd\")
         (data (i32.const 140) \"empty/../../outside\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 10) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 76))
         (call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 11) (i32.const 200)) (i32.const 76))
         (call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 140) (i32.const 19) (i32.const 200)) (i32.const 76))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.refusals(),
        [
            Refusal {
                function: WasiFunction::PathFilestatGet,
                reason: RefusalReason::Escape,
                count: 2,
            },
            Refusal {
                function: WasiFunction::PathOpen,
                reason: RefusalReason::Escape,
                count: 1,
            },
        ]
    );
}

#[test]
fn every_clock_has_one_fixed_resolution_and_an_unknown_clock_is_invalid() {
    let bytes = command(
        &["clock_res_get"],
        "",
        "(call $expect (call $clock_res_get (i32.const 0) (i32.const 64)) (i32.const 0))
         (call $expect (call $clock_res_get (i32.const 1) (i32.const 72)) (i32.const 0))
         (call $expect (call $clock_res_get (i32.const 2) (i32.const 80)) (i32.const 0))
         (call $expect (call $clock_res_get (i32.const 3) (i32.const 88)) (i32.const 0))
         (call $expect (call $clock_res_get (i32.const 4) (i32.const 96)) (i32.const 28))
         (call $emit (i32.const 64) (i32.const 32))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(emitted(&transcript), [1, 1, 1, 1]);
}

/// A command reading the monotonic clock twice, back to back, and the realtime clock once.
fn two_readings() -> Vec<u8> {
    command(
        &["clock_time_get"],
        "",
        "(call $expect (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 64)) (i32.const 0))
         (call $expect (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 72)) (i32.const 0))
         (call $expect (call $clock_time_get (i32.const 0) (i64.const 1) (i32.const 80)) (i32.const 0))
         (call $emit (i32.const 64) (i32.const 24))",
    )
}

#[test]
fn the_clocks_move_only_with_the_fuel_spent_at_the_rate_the_policy_names() {
    let bytes = two_readings();
    let slow = invocation();
    let mut fast = invocation();
    fast.clock.nanos_per_fuel = NonZeroU64::new(1000).expect("nonzero");
    let [first, second, realtime] = emitted(&run(&bytes, &slow))[..] else {
        panic!("three readings")
    };
    let [fast_first, fast_second, _fast_realtime] = emitted(&run(&bytes, &fast))[..] else {
        panic!("three readings")
    };
    assert!(second > first && first > slow.clock.monotonic_origin);
    assert!(realtime > slow.clock.realtime_origin);
    assert_eq!(
        fast_second - fast_first,
        (second - first) * 1000,
        "the same fuel between the readings, a thousand times the nanoseconds"
    );
    assert_eq!(
        fast_first - fast.clock.monotonic_origin,
        (first - slow.clock.monotonic_origin) * 1000
    );
    assert_eq!(emitted(&run(&bytes, &slow)), [first, second, realtime]);
}

/// A command subscribing once to a clock, with `timeout` and `flags`, and emitting the event and the time after.
fn wait(clock: u32, timeout: &str, flags: u32) -> Vec<u8> {
    command(
        &["poll_oneoff", "clock_time_get"],
        "",
        &format!(
            "(i64.store (i32.const 100) (i64.const 77))
             (i32.store8 (i32.const 108) (i32.const 0))
             (i32.store (i32.const 116) (i32.const {clock}))
             (i64.store (i32.const 124) (i64.const {timeout}))
             (i32.store16 (i32.const 140) (i32.const {flags}))
             (call $expect (call $poll_oneoff (i32.const 100) (i32.const 200) (i32.const 1) (i32.const 64)) (i32.const 0))
             (call $expect (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 72)) (i32.const 0))
             (call $emit (i32.const 64) (i32.const 16))
             (call $emit (i32.const 200) (i32.const 32))"
        ),
    )
}

#[test]
fn a_relative_wait_moves_the_clocks_by_exactly_its_timeout_and_returns_at_once() {
    let transcript = run(&wait(1, "3000000000", 0), &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.waited(), 3_000_000_000);
    let [events, after, userdata, kind_and_error, bytes, flags] = emitted(&transcript)[..] else {
        panic!("two emissions")
    };
    assert_eq!(events & 0xffff_ffff, 1, "one event");
    assert!(after > invocation().clock.monotonic_origin + 3_000_000_000);
    assert_eq!(userdata, 77);
    assert_eq!(
        kind_and_error & 0xff_ffff,
        0,
        "a clock event without an error"
    );
    assert_eq!((bytes, flags), (0, 0));
}

#[test]
fn an_absolute_wait_moves_the_clocks_to_its_deadline_and_one_already_past_moves_nothing() {
    let origin = invocation().clock.monotonic_origin;
    let ahead = run(
        &wait(1, &(origin + 9_000_000_000).to_string(), 1),
        &invocation(),
    );
    assert_eq!(ahead.stop(), SealedStop::Returned);
    assert!(
        ahead.waited() > 8_999_000_000 && ahead.waited() < 9_000_000_000,
        "the wait is what was left to the deadline: {}",
        ahead.waited()
    );
    let past = run(&wait(1, &origin.to_string(), 1), &invocation());
    assert_eq!(past.stop(), SealedStop::Returned);
    assert_eq!(past.waited(), 0);
}

#[test]
fn a_wait_past_the_end_of_virtual_time_spends_the_whole_budget() {
    let transcript = run(&wait(0, "-1", 0), &invocation());
    assert_eq!(transcript.stop(), SealedStop::FuelExhausted);
    assert_eq!(transcript.fuel_spent(), invocation().fuel);
}

#[test]
fn a_wait_on_a_clock_of_time_run_is_refused_in_its_event() {
    let transcript = run(&wait(2, "1000", 0), &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.waited(), 0);
    let [_events, _after, _userdata, kind_and_error, _bytes, _flags] = emitted(&transcript)[..]
    else {
        panic!("two emissions")
    };
    assert_eq!(kind_and_error & 0xffff, 58, "the event carries notsup");
    assert_eq!(
        transcript.refusals(),
        [Refusal {
            function: WasiFunction::PollOneoff,
            reason: RefusalReason::CpuClockWait,
            count: 1,
        }]
    );
}

#[test]
fn descriptors_are_ready_at_once_and_standard_input_is_at_its_end() {
    let bytes = command(
        &["poll_oneoff", "fd_read"],
        "",
        "(i64.store (i32.const 100) (i64.const 1))
         (i32.store8 (i32.const 108) (i32.const 1))
         (i32.store (i32.const 116) (i32.const 0))
         (i64.store (i32.const 148) (i64.const 2))
         (i32.store8 (i32.const 156) (i32.const 2))
         (i32.store (i32.const 164) (i32.const 1))
         (i64.store (i32.const 196) (i64.const 3))
         (i32.store8 (i32.const 204) (i32.const 0))
         (i32.store (i32.const 212) (i32.const 1))
         (i64.store (i32.const 220) (i64.const 1000000))
         (call $expect (call $poll_oneoff (i32.const 100) (i32.const 300) (i32.const 3) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 400))
         (i32.store (i32.const 76) (i32.const 16))
         (call $expect (call $fd_read (i32.const 0) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $emit (i32.const 64) (i32.const 8))
         (call $emit (i32.const 80) (i32.const 8))
         (call $emit (i32.const 300) (i32.const 64))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.waited(),
        0,
        "a ready descriptor waits on no clock"
    );
    let [events, read, first, _, _, stdin_flags, second, _, _, _] = emitted(&transcript)[..] else {
        panic!("three emissions")
    };
    assert_eq!(
        events & 0xffff_ffff,
        2,
        "stdin readable and stdout writable, the clock not due"
    );
    assert_eq!(read & 0xffff_ffff, 0, "standard input is at its end");
    assert_eq!((first, second), (1, 2));
    assert_eq!(
        stdin_flags & 0xffff,
        1,
        "the end of standard input is a hangup"
    );
}

#[test]
fn standard_output_past_its_cap_is_counted_and_not_kept() {
    let bytes = command(
        &[],
        "(data (i32.const 100) \"0123456789\")",
        "(call $emit (i32.const 100) (i32.const 10))",
    );
    let mut capped = invocation();
    capped.limits.stdout = 4;
    let transcript = run(&bytes, &capped);
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "the guest was told all ten were written"
    );
    assert_eq!(transcript.stdout().bytes(), b"0123");
    assert_eq!(transcript.stdout().truncated(), 6);
}

#[test]
fn random_bytes_are_one_stream_of_the_seed_however_they_are_asked_for() {
    let whole = command(
        &["random_get"],
        "",
        "(call $expect (call $random_get (i32.const 100) (i32.const 32)) (i32.const 0))
         (call $emit (i32.const 100) (i32.const 32))",
    );
    let halves = command(
        &["random_get"],
        "",
        "(call $expect (call $random_get (i32.const 100) (i32.const 13)) (i32.const 0))
         (call $expect (call $random_get (i32.const 113) (i32.const 19)) (i32.const 0))
         (call $emit (i32.const 100) (i32.const 32))",
    );
    let mut other = invocation();
    other.seed = 12;
    let seeded = run(&whole, &invocation());
    assert_eq!(seeded.stdout().bytes().len(), 32);
    assert_eq!(seeded.stdout(), run(&halves, &invocation()).stdout());
    assert_ne!(seeded.stdout(), run(&whole, &other).stdout());
}

#[test]
fn a_file_the_guest_makes_is_in_the_overlay_until_the_overlay_is_full() {
    let bytes = command(
        &["path_open", "fd_pwrite"],
        "(data (i32.const 100) \"made.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 1000))
         (i32.store (i32.const 76) (i32.const 400))
         (call $expect (call $fd_pwrite (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i64.const 0) (i32.const 80)) (i32.const 0))
         (call $expect (call $fd_pwrite (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i64.const 400) (i32.const 80)) (i32.const 51))",
    );
    let mut small = invocation();
    small.limits.overlay = 600;
    let transcript = run(&bytes, &small);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.refusals(),
        [Refusal {
            function: WasiFunction::FdPwrite,
            reason: RefusalReason::OverlayFull,
            count: 1,
        }]
    );
    let [made] = transcript.overlay() else {
        panic!("one entry: {:?}", transcript.overlay())
    };
    assert_eq!(made.path, "/sandbox/made.txt");
    match &made.state {
        OverlayState::File { contents, .. } => assert_eq!(contents.len(), 400),
        OverlayState::Directory { .. } | OverlayState::Removed => panic!("{made:?}"),
    }
}

#[test]
fn descriptors_renumber_close_advise_allocate_sync_and_narrow_their_rights() {
    let bytes = command(
        &[
            "path_open",
            "fd_renumber",
            "fd_close",
            "fd_advise",
            "fd_allocate",
            "fd_datasync",
            "fd_sync",
            "fd_fdstat_set_rights",
            "fd_fdstat_set_flags",
            "fd_filestat_get",
            "sched_yield",
        ],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (call $expect (i32.load (i32.const 64)) (i32.const 4))
         (call $expect (call $fd_advise (i32.const 4) (i64.const 0) (i64.const 4) (i32.const 1)) (i32.const 0))
         (call $expect (call $fd_advise (i32.const 4) (i64.const 0) (i64.const 4) (i32.const 9)) (i32.const 28))
         (call $expect (call $fd_allocate (i32.const 4) (i64.const 0) (i64.const 16)) (i32.const 0))
         (call $expect (call $fd_datasync (i32.const 4)) (i32.const 0))
         (call $expect (call $fd_sync (i32.const 4)) (i32.const 0))
         (call $expect (call $fd_fdstat_set_flags (i32.const 4) (i32.const 1)) (i32.const 0))
         (call $expect (call $fd_fdstat_set_rights (i32.const 4) (i64.const 2097154) (i64.const 0)) (i32.const 0))
         (call $expect (call $fd_fdstat_set_rights (i32.const 4) (i64.const 64) (i64.const 0)) (i32.const 76))
         (call $expect (call $fd_renumber (i32.const 4) (i32.const 2)) (i32.const 0))
         (call $expect (call $fd_filestat_get (i32.const 2) (i32.const 200)) (i32.const 0))
         (call $expect (call $fd_close (i32.const 4)) (i32.const 8))
         (call $expect (call $fd_close (i32.const 2)) (i32.const 0))
         (call $expect (call $fd_close (i32.const 2)) (i32.const 8))
         (call $expect (call $sched_yield) (i32.const 0))
         (call $emit (i32.const 232) (i32.const 8))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        emitted(&transcript),
        [16],
        "allocating grew the file to sixteen bytes"
    );
    assert!(transcript.refusals().is_empty());
}

/// Misuses of the filesystem and of memory, each as a case name, the functions it imports, its data, and calls checked by `$expect`.
const MISUSES: [(&str, &[&str], &str, &str); 14] = [
    (
        "a directory made where one is: exist",
        &["path_create_directory"],
        "(data (i32.const 100) \"empty\")",
        "(call $expect (call $path_create_directory (i32.const 3) (i32.const 100) (i32.const 5)) (i32.const 20))",
    ),
    (
        "a file made exclusively where one is: exist",
        &["path_open"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 5) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 20))",
    ),
    (
        "a path through a file: notdir",
        &["path_filestat_get"],
        "(data (i32.const 100) \"seen.txt/x\")",
        "(call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 10) (i32.const 200)) (i32.const 54))",
    ),
    (
        "a directory opened where a file is: notdir",
        &["path_open"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 2) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 54))",
    ),
    (
        "a directory unlinked as a file: isdir",
        &["path_unlink_file"],
        "(data (i32.const 100) \"empty\")",
        "(call $expect (call $path_unlink_file (i32.const 3) (i32.const 100) (i32.const 5)) (i32.const 31))",
    ),
    (
        "a directory removed while it holds an entry: notempty",
        &["path_create_directory", "path_open", "path_remove_directory"],
        "(data (i32.const 100) \"full\") (data (i32.const 120) \"full/entry\")",
        "(call $expect (call $path_create_directory (i32.const 3) (i32.const 100) (i32.const 4)) (i32.const 0))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 10) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (call $expect (call $path_remove_directory (i32.const 3) (i32.const 100) (i32.const 4)) (i32.const 55))",
    ),
    (
        "a position asked of a stream: spipe",
        &["fd_seek", "fd_tell"],
        "",
        "(call $expect (call $fd_seek (i32.const 1) (i64.const 0) (i32.const 0) (i32.const 64)) (i32.const 70))
         (call $expect (call $fd_tell (i32.const 0) (i32.const 64)) (i32.const 70))",
    ),
    (
        "a write that would end past the last offset: fbig",
        &["path_open", "fd_pwrite"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 100))
         (i32.store (i32.const 76) (i32.const 1))
         (call $expect (call $fd_pwrite (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i64.const -1) (i32.const 80)) (i32.const 22))",
    ),
    (
        "a pointer past the end of memory: fault",
        &["args_sizes_get"],
        "",
        "(call $expect (call $args_sizes_get (i32.const 65536) (i32.const 0)) (i32.const 21))",
    ),
    (
        "a path that is not UTF-8: ilseq",
        &["path_filestat_get"],
        "(data (i32.const 100) \"\\ff\")",
        "(call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 1) (i32.const 200)) (i32.const 25))",
    ),
    (
        "a file renamed onto a directory: isdir",
        &["path_rename"],
        "(data (i32.const 100) \"seen.txt\") (data (i32.const 120) \"empty\")",
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 3) (i32.const 120) (i32.const 5)) (i32.const 31))",
    ),
    (
        "a directory renamed onto a file: notdir",
        &["path_rename"],
        "(data (i32.const 100) \"seen.txt\") (data (i32.const 120) \"empty\")",
        "(call $expect (call $path_rename (i32.const 3) (i32.const 120) (i32.const 5) (i32.const 3) (i32.const 100) (i32.const 8)) (i32.const 54))",
    ),
    (
        "a directory renamed into itself: inval",
        &["path_rename"],
        "(data (i32.const 100) \"empty\") (data (i32.const 120) \"empty/inner\")",
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 5) (i32.const 3) (i32.const 120) (i32.const 11)) (i32.const 28))",
    ),
    (
        "a directory renamed onto one that holds an entry: notempty",
        &["path_create_directory", "path_open", "path_rename"],
        "(data (i32.const 100) \"full\") (data (i32.const 120) \"full/entry\") (data (i32.const 140) \"empty\")",
        "(call $expect (call $path_create_directory (i32.const 3) (i32.const 100) (i32.const 4)) (i32.const 0))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 10) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (call $expect (call $path_rename (i32.const 3) (i32.const 140) (i32.const 5) (i32.const 3) (i32.const 100) (i32.const 4)) (i32.const 55))",
    ),
];

#[test]
fn a_misuse_is_answered_with_the_error_number_posix_gives_it() {
    for (case, imports, data, body) in MISUSES {
        let transcript = run(&command(imports, data, body), &invocation());
        assert_eq!(
            transcript.stop(),
            SealedStop::Returned,
            "{case}: the guest exits 1000 plus the error number it was given instead"
        );
        assert!(
            transcript.refusals().is_empty(),
            "{case}: nothing is refused"
        );
    }
}

/// The rights of WASI preview1 by the bit the specification gives each, written from it rather than from the crate's constants, so each checks the other.
mod right {
    /// Flushing a file's data.
    pub(super) const FD_DATASYNC: u64 = 1 << 0;
    /// Reading.
    pub(super) const FD_READ: u64 = 1 << 1;
    /// Moving the position, which implies asking it.
    pub(super) const FD_SEEK: u64 = 1 << 2;
    /// Setting the descriptor's flags.
    pub(super) const FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
    /// Flushing a file's data and metadata.
    pub(super) const FD_SYNC: u64 = 1 << 4;
    /// Asking the position.
    pub(super) const FD_TELL: u64 = 1 << 5;
    /// Writing.
    pub(super) const FD_WRITE: u64 = 1 << 6;
    /// Advising how a file will be used.
    pub(super) const FD_ADVISE: u64 = 1 << 7;
    /// Allocating space in a file.
    pub(super) const FD_ALLOCATE: u64 = 1 << 8;
    /// Making a directory.
    pub(super) const PATH_CREATE_DIRECTORY: u64 = 1 << 9;
    /// Opening a file that is made where it is absent.
    pub(super) const PATH_CREATE_FILE: u64 = 1 << 10;
    /// Opening a path.
    pub(super) const PATH_OPEN: u64 = 1 << 13;
    /// Reading a directory's entries.
    pub(super) const FD_READDIR: u64 = 1 << 14;
    /// Reading a symbolic link.
    pub(super) const PATH_READLINK: u64 = 1 << 15;
    /// Renaming from a directory.
    pub(super) const PATH_RENAME_SOURCE: u64 = 1 << 16;
    /// Renaming into a directory.
    pub(super) const PATH_RENAME_TARGET: u64 = 1 << 17;
    /// Asking the metadata of a path.
    pub(super) const PATH_FILESTAT_GET: u64 = 1 << 18;
    /// Opening a file that is truncated.
    pub(super) const PATH_FILESTAT_SET_SIZE: u64 = 1 << 19;
    /// Setting the times of a path.
    pub(super) const PATH_FILESTAT_SET_TIMES: u64 = 1 << 20;
    /// Asking the metadata of what the descriptor reaches.
    pub(super) const FD_FILESTAT_GET: u64 = 1 << 21;
    /// Setting the size of a file through its descriptor.
    pub(super) const FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
    /// Setting the times of what the descriptor reaches.
    pub(super) const FD_FILESTAT_SET_TIMES: u64 = 1 << 23;
    /// Removing a directory.
    pub(super) const PATH_REMOVE_DIRECTORY: u64 = 1 << 25;
    /// Removing a file.
    pub(super) const PATH_UNLINK_FILE: u64 = 1 << 26;
    /// Waiting on the descriptor in `poll_oneoff`.
    pub(super) const POLL_FD_READWRITE: u64 = 1 << 27;
}

/// Which descriptor a case takes rights away from before its call.
#[derive(Debug, Clone, Copy)]
enum Narrowed {
    /// The preopen's own rights.
    Preopen,
    /// The rights the preopen passes on to what is opened through it, before `seen.txt` is opened through it as descriptor 4.
    PassedOn,
    /// The rights of `seen.txt`, opened through the preopen with every right as descriptor 4.
    File,
}

/// A call a right governs: what it is, the descriptor the rights are taken from, the rights taken, the functions it imports, and the call checked by `$expect` against the answer it must now give.
type Governed = (
    &'static str,
    Narrowed,
    u64,
    &'static [&'static str],
    &'static str,
);

/// Every call a right governs, each made once the right is taken away, and each answered as a descriptor without it: `notcapable`, or `badf` for reading and writing, as POSIX answers a descriptor not open for them.
const GOVERNED: [Governed; 33] = [
    (
        "path_create_directory without path_create_directory",
        Narrowed::Preopen,
        right::PATH_CREATE_DIRECTORY,
        &["path_create_directory"],
        "(call $expect (call $path_create_directory (i32.const 3) (i32.const 120) (i32.const 4)) (i32.const 76))",
    ),
    (
        "path_open creating a file without path_create_file",
        Narrowed::Preopen,
        right::PATH_CREATE_FILE,
        &["path_open"],
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 4) (i32.const 1) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "path_open without path_open",
        Narrowed::Preopen,
        right::PATH_OPEN,
        &["path_open"],
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 0) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "path_open truncating a file without path_filestat_set_size",
        Narrowed::Preopen,
        right::PATH_FILESTAT_SET_SIZE,
        &["path_open"],
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 8) (i64.const 0) (i64.const 0) (i32.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "fd_readdir without fd_readdir",
        Narrowed::Preopen,
        right::FD_READDIR,
        &["fd_readdir"],
        "(call $expect (call $fd_readdir (i32.const 3) (i32.const 700) (i32.const 256) (i64.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "path_readlink without path_readlink",
        Narrowed::Preopen,
        right::PATH_READLINK,
        &["path_readlink"],
        "(call $expect (call $path_readlink (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 700) (i32.const 64) (i32.const 64)) (i32.const 76))",
    ),
    (
        "path_rename without path_rename_source",
        Narrowed::Preopen,
        right::PATH_RENAME_SOURCE,
        &["path_rename"],
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 3) (i32.const 120) (i32.const 4)) (i32.const 76))",
    ),
    (
        "path_rename without path_rename_target",
        Narrowed::Preopen,
        right::PATH_RENAME_TARGET,
        &["path_rename"],
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 3) (i32.const 120) (i32.const 4)) (i32.const 76))",
    ),
    (
        "path_filestat_get without path_filestat_get",
        Narrowed::Preopen,
        right::PATH_FILESTAT_GET,
        &["path_filestat_get"],
        "(call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 200)) (i32.const 76))",
    ),
    (
        "path_filestat_set_times without path_filestat_set_times",
        Narrowed::Preopen,
        right::PATH_FILESTAT_SET_TIMES,
        &["path_filestat_set_times"],
        "(call $expect (call $path_filestat_set_times (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i64.const 0) (i64.const 0) (i32.const 1)) (i32.const 76))",
    ),
    (
        "path_remove_directory without path_remove_directory",
        Narrowed::Preopen,
        right::PATH_REMOVE_DIRECTORY,
        &["path_remove_directory"],
        "(call $expect (call $path_remove_directory (i32.const 3) (i32.const 140) (i32.const 5)) (i32.const 76))",
    ),
    (
        "path_unlink_file without path_unlink_file",
        Narrowed::Preopen,
        right::PATH_UNLINK_FILE,
        &["path_unlink_file"],
        "(call $expect (call $path_unlink_file (i32.const 3) (i32.const 100) (i32.const 8)) (i32.const 76))",
    ),
    (
        "fd_filestat_get of a directory without fd_filestat_get",
        Narrowed::Preopen,
        right::FD_FILESTAT_GET,
        &["fd_filestat_get"],
        "(call $expect (call $fd_filestat_get (i32.const 3) (i32.const 200)) (i32.const 76))",
    ),
    (
        "fd_filestat_set_times of a directory without fd_filestat_set_times",
        Narrowed::Preopen,
        right::FD_FILESTAT_SET_TIMES,
        &["fd_filestat_set_times"],
        "(call $expect (call $fd_filestat_set_times (i32.const 3) (i64.const 0) (i64.const 0) (i32.const 1)) (i32.const 76))",
    ),
    (
        "fd_fdstat_set_flags without fd_fdstat_set_flags",
        Narrowed::Preopen,
        right::FD_FDSTAT_SET_FLAGS,
        &["fd_fdstat_set_flags"],
        "(call $expect (call $fd_fdstat_set_flags (i32.const 3) (i32.const 0)) (i32.const 76))",
    ),
    (
        "fd_sync without fd_sync",
        Narrowed::Preopen,
        right::FD_SYNC,
        &["fd_sync"],
        "(call $expect (call $fd_sync (i32.const 3)) (i32.const 76))",
    ),
    (
        "fd_datasync without fd_datasync",
        Narrowed::Preopen,
        right::FD_DATASYNC,
        &["fd_datasync"],
        "(call $expect (call $fd_datasync (i32.const 3)) (i32.const 76))",
    ),
    (
        "a file opened through a directory that no longer passes on fd_write",
        Narrowed::PassedOn,
        right::FD_WRITE,
        &["fd_write", "fd_fdstat_get"],
        "(call $expect (call $fd_write (i32.const 4) (i32.const 300) (i32.const 1) (i32.const 64)) (i32.const 8))
         (call $expect (call $fd_fdstat_get (i32.const 4) (i32.const 400)) (i32.const 0))
         (call $expect (i32.wrap_i64 (i64.and (i64.load (i32.const 408)) (i64.const 64))) (i32.const 0))",
    ),
    (
        "fd_read without fd_read",
        Narrowed::File,
        right::FD_READ,
        &["fd_read"],
        "(call $expect (call $fd_read (i32.const 4) (i32.const 300) (i32.const 1) (i32.const 64)) (i32.const 8))",
    ),
    (
        "fd_write without fd_write",
        Narrowed::File,
        right::FD_WRITE,
        &["fd_write"],
        "(call $expect (call $fd_write (i32.const 4) (i32.const 300) (i32.const 1) (i32.const 64)) (i32.const 8))",
    ),
    (
        "fd_pread without fd_read",
        Narrowed::File,
        right::FD_READ,
        &["fd_pread"],
        "(call $expect (call $fd_pread (i32.const 4) (i32.const 300) (i32.const 1) (i64.const 0) (i32.const 64)) (i32.const 8))",
    ),
    (
        "fd_pread without fd_seek",
        Narrowed::File,
        right::FD_SEEK,
        &["fd_pread"],
        "(call $expect (call $fd_pread (i32.const 4) (i32.const 300) (i32.const 1) (i64.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "fd_pwrite without fd_write",
        Narrowed::File,
        right::FD_WRITE,
        &["fd_pwrite"],
        "(call $expect (call $fd_pwrite (i32.const 4) (i32.const 300) (i32.const 1) (i64.const 0) (i32.const 64)) (i32.const 8))",
    ),
    (
        "fd_pwrite without fd_seek",
        Narrowed::File,
        right::FD_SEEK,
        &["fd_pwrite"],
        "(call $expect (call $fd_pwrite (i32.const 4) (i32.const 300) (i32.const 1) (i64.const 0) (i32.const 64)) (i32.const 76))",
    ),
    (
        "fd_seek moving the position without fd_seek, where fd_tell still asks it",
        Narrowed::File,
        right::FD_SEEK,
        &["fd_seek", "fd_tell"],
        "(call $expect (call $fd_seek (i32.const 4) (i64.const 1) (i32.const 0) (i32.const 64)) (i32.const 76))
         (call $expect (call $fd_seek (i32.const 4) (i64.const 0) (i32.const 1) (i32.const 64)) (i32.const 0))
         (call $expect (call $fd_tell (i32.const 4) (i32.const 64)) (i32.const 0))",
    ),
    (
        "fd_tell without fd_tell, where fd_seek still implies it",
        Narrowed::File,
        right::FD_TELL,
        &["fd_tell"],
        "(call $expect (call $fd_tell (i32.const 4) (i32.const 64)) (i32.const 0))",
    ),
    (
        "fd_tell without fd_tell or fd_seek",
        Narrowed::File,
        right::FD_TELL | right::FD_SEEK,
        &["fd_seek", "fd_tell"],
        "(call $expect (call $fd_tell (i32.const 4) (i32.const 64)) (i32.const 76))
         (call $expect (call $fd_seek (i32.const 4) (i64.const 0) (i32.const 1) (i32.const 64)) (i32.const 76))",
    ),
    (
        "fd_advise without fd_advise",
        Narrowed::File,
        right::FD_ADVISE,
        &["fd_advise"],
        "(call $expect (call $fd_advise (i32.const 4) (i64.const 0) (i64.const 4) (i32.const 1)) (i32.const 76))",
    ),
    (
        "fd_allocate without fd_allocate",
        Narrowed::File,
        right::FD_ALLOCATE,
        &["fd_allocate"],
        "(call $expect (call $fd_allocate (i32.const 4) (i64.const 0) (i64.const 16)) (i32.const 76))",
    ),
    (
        "fd_filestat_set_size without fd_filestat_set_size",
        Narrowed::File,
        right::FD_FILESTAT_SET_SIZE,
        &["fd_filestat_set_size"],
        "(call $expect (call $fd_filestat_set_size (i32.const 4) (i64.const 0)) (i32.const 76))",
    ),
    (
        "fd_filestat_get of a file without fd_filestat_get",
        Narrowed::File,
        right::FD_FILESTAT_GET,
        &["fd_filestat_get"],
        "(call $expect (call $fd_filestat_get (i32.const 4) (i32.const 200)) (i32.const 76))",
    ),
    (
        "fd_filestat_set_times of a file without fd_filestat_set_times",
        Narrowed::File,
        right::FD_FILESTAT_SET_TIMES,
        &["fd_filestat_set_times"],
        "(call $expect (call $fd_filestat_set_times (i32.const 4) (i64.const 0) (i64.const 0) (i32.const 1)) (i32.const 76))",
    ),
    (
        "poll_oneoff waiting to read without poll_fd_readwrite",
        Narrowed::File,
        right::POLL_FD_READWRITE,
        &["poll_oneoff"],
        "(i32.store8 (i32.const 508) (i32.const 1))
         (i32.store (i32.const 516) (i32.const 4))
         (call $expect (call $poll_oneoff (i32.const 500) (i32.const 600) (i32.const 1) (i32.const 64)) (i32.const 0))
         (call $expect (i32.load16_u (i32.const 608)) (i32.const 76))",
    ),
];

/// The WAT that takes `taken` away from the rights `narrowed` names, leaving `seen.txt` open as descriptor 4 where the case is about a file.
fn narrowing(narrowed: Narrowed, taken: u64) -> String {
    let kept = (!taken).cast_signed();
    let open = "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (call $expect (i32.load (i32.const 64)) (i32.const 4))";
    let (descriptor, base, passed_on) = match narrowed {
        Narrowed::Preopen => (3, kept, -1_i64),
        Narrowed::PassedOn => (3, -1_i64, kept),
        Narrowed::File => (4, kept, -1_i64),
    };
    let narrow = format!(
        "(call $expect (call $fd_fdstat_get (i32.const {descriptor}) (i32.const 400)) (i32.const 0))
         (call $expect (call $fd_fdstat_set_rights (i32.const {descriptor}) (i64.and (i64.load (i32.const 408)) (i64.const {base})) (i64.and (i64.load (i32.const 416)) (i64.const {passed_on}))) (i32.const 0))"
    );
    match narrowed {
        Narrowed::Preopen => narrow,
        Narrowed::PassedOn => format!("{narrow}\n{open}"),
        Narrowed::File => format!("{open}\n{narrow}"),
    }
}

#[test]
fn a_right_taken_away_refuses_every_call_it_governs() {
    let mut granted = Vec::new();
    for (case, narrowed, taken, imports, call) in GOVERNED {
        let narrowing_imports = ["path_open", "fd_fdstat_get", "fd_fdstat_set_rights"];
        let mut all = narrowing_imports.to_vec();
        all.extend(
            imports
                .iter()
                .filter(|name| !narrowing_imports.contains(name)),
        );
        let bytes = command(
            &all,
            "(data (i32.const 100) \"seen.txt\")
             (data (i32.const 120) \"made\")
             (data (i32.const 140) \"empty\")
             (data (i32.const 300) \"\\40\\01\\00\\00\\04\\00\\00\\00\")",
            &format!("{}\n{call}", narrowing(narrowed, taken)),
        );
        let transcript = run(&bytes, &invocation());
        if transcript.stop() != SealedStop::Returned {
            granted.push(format!("{case}: {:?}", transcript.stop()));
        }
        assert!(
            transcript.refusals().is_empty(),
            "{case}: a right the guest gave up is its own narrowing, never the sandbox refusing it"
        );
    }
    assert!(
        granted.is_empty(),
        "a right taken away is one the descriptor no longer has, so the call it governs is \
         answered as a descriptor without it; each of these was carried out, or answered \
         otherwise, and exited 1000 plus the error number it was given:\n  {}",
        granted.join("\n  ")
    );
}

#[test]
fn a_rename_from_one_preopen_into_another_is_answered_xdev() {
    let bytes = command(
        &["path_rename"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 4) (i32.const 100) (i32.const 8)) (i32.const 75))",
    );
    let mut two = invocation();
    two.preopens = Preopens::new(vec![tree("/one", snapshot()), tree("/two", snapshot())])
        .expect("two preopens");
    let transcript = run(&bytes, &two);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert!(
        transcript.overlay().is_empty(),
        "the refused rename moved nothing"
    );
}

#[test]
fn descriptors_run_out_at_one_count_whatever_the_machine_allows() {
    let bytes = command(
        &["path_open"],
        "(data (i32.const 100) \"seen.txt\")",
        "(local $opened i32) (local $answer i32)
         (block $out
           (loop $again
             (local.set $answer (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)))
             (br_if $out (local.get $answer))
             (local.set $opened (i32.add (local.get $opened) (i32.const 1)))
             (br $again)))
         (call $expect (local.get $answer) (i32.const 33))
         (i64.store (i32.const 200) (i64.extend_i32_u (local.get $opened)))
         (call $emit (i32.const 200) (i32.const 8))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        emitted(&transcript),
        [4092],
        "4096 descriptors in all, three standard streams and the preopen among them"
    );
}

#[test]
fn the_preopen_is_named_to_the_guest_and_no_path_is_a_symbolic_link() {
    let bytes = command(
        &["fd_prestat_get", "fd_prestat_dir_name", "path_readlink"],
        "(data (i32.const 100) \"seen.txt\")
         (data (i32.const 120) \"absent.txt\")",
        "(call $expect (call $fd_prestat_get (i32.const 3) (i32.const 64)) (i32.const 0))
         (call $expect (call $fd_prestat_get (i32.const 4) (i32.const 64)) (i32.const 8))
         (call $expect (call $fd_prestat_dir_name (i32.const 3) (i32.const 200) (i32.const 3)) (i32.const 37))
         (call $expect (call $fd_prestat_dir_name (i32.const 3) (i32.const 200) (i32.const 8)) (i32.const 0))
         (call $expect (call $path_readlink (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 300) (i32.const 64) (i32.const 72)) (i32.const 28))
         (call $expect (call $path_readlink (i32.const 3) (i32.const 120) (i32.const 10) (i32.const 300) (i32.const 64) (i32.const 72)) (i32.const 44))
         (call $emit (i32.const 200) (i32.const 8))",
    );
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"/sandbox");
}

#[test]
fn fuel_that_runs_out_inside_a_host_call_is_fuel_exhausted() {
    let bytes = command(&["sched_yield"], "", "(drop (call $sched_yield))");
    let mut starved = invocation();
    starved.fuel = 40;
    let transcript = run(&bytes, &starved);
    assert_eq!(transcript.stop(), SealedStop::FuelExhausted);
    assert_eq!(transcript.fuel_spent(), 40);
}

#[test]
fn a_host_call_the_fuel_left_cannot_pay_for_ends_the_guest_as_fuel_exhausted() {
    let bytes = command(
        &["random_get"],
        "",
        "(call $expect (call $random_get (i32.const 100) (i32.const 60000)) (i32.const 0))
         (call $emit (i32.const 100) (i32.const 8))",
    );
    let mut short = invocation();
    short.fuel = 5_000;
    let transcript = run(&bytes, &short);
    assert_eq!(
        transcript.stop(),
        SealedStop::FuelExhausted,
        "sixty thousand bytes cost sixty thousand fuel, and five thousand were left"
    );
    assert_eq!(transcript.fuel_spent(), 5_000);
    assert!(
        transcript.stdout().bytes().is_empty(),
        "the call stopped the guest"
    );
    let mut enough = invocation();
    enough.fuel = 100_000;
    assert_eq!(run(&bytes, &enough).stop(), SealedStop::Returned);
}

#[test]
fn a_trap_is_classified_by_its_kind() {
    let divide = command(
        &[],
        "",
        "(drop (i32.div_s (i32.const 1) (i32.load (i32.const 500))))",
    );
    assert_eq!(
        run(&divide, &invocation()).stop(),
        SealedStop::Trapped {
            kind: TrapKind::IntegerDivisionByZero
        }
    );
    let unreachable = command(&[], "", "unreachable");
    assert_eq!(
        run(&unreachable, &invocation()).stop(),
        SealedStop::Trapped {
            kind: TrapKind::Unreachable
        }
    );
    let out_of_bounds = command(&[], "", "(drop (i32.load (i32.const 70000)))");
    assert_eq!(
        run(&out_of_bounds, &invocation()).stop(),
        SealedStop::Trapped {
            kind: TrapKind::MemoryOutOfBounds
        }
    );
}

#[test]
fn a_refused_growth_and_then_a_trap_is_memory_exhausted_and_a_refused_growth_alone_is_not() {
    let grow_then_trap = command(
        &[],
        "",
        "(call $expect (memory.grow (i32.const 200)) (i32.const -1))
         unreachable",
    );
    let transcript = run(&grow_then_trap, &invocation());
    assert_eq!(transcript.stop(), SealedStop::MemoryExhausted);
    assert_eq!(transcript.denials().memory(), 1);
    assert_eq!(transcript.denials().memory_requests(), [201 * 65536]);
    assert_eq!(transcript.peak_memory(), 65536);
    let grow_then_return = command(
        &[],
        "",
        "(call $expect (memory.grow (i32.const 200)) (i32.const -1))
         (call $expect (memory.grow (i32.const 1)) (i32.const 1))",
    );
    let transcript = run(&grow_then_return, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.denials().memory(), 1);
    assert_eq!(transcript.peak_memory(), 2 * 65536);
}

#[test]
fn a_module_whose_first_memory_is_past_the_limit_is_memory_exhausted() {
    let bytes =
        wat::parse_str("(module (memory (export \"memory\") 100) (func (export \"_start\")))")
            .expect("valid WAT");
    let transcript = run(&bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::MemoryExhausted);
    assert_eq!(transcript.fuel_spent(), 0);
}

#[test]
fn a_growth_past_the_modules_own_maximum_is_no_denial_and_a_table_past_the_cap_is_one() {
    let bytes = wat::parse_str(
        "(module
           (import \"wasi_snapshot_preview1\" \"proc_exit\" (func $proc_exit (param i32)))
           (memory (export \"memory\") 1 2)
           (table 1 funcref)
           (func (export \"_start\")
             (if (i32.ne (memory.grow (i32.const 5)) (i32.const -1)) (then (call $proc_exit (i32.const 1))))
             (if (i32.ne (table.grow (ref.null func) (i32.const 2000000)) (i32.const -1)) (then (call $proc_exit (i32.const 2))))
             (drop (i32.load (i32.const 70000)))))",
    )
    .expect("valid WAT");
    let transcript = run(&bytes, &invocation());
    assert_eq!(
        transcript.stop(),
        SealedStop::Trapped {
            kind: TrapKind::MemoryOutOfBounds
        },
        "exit 1 is a memory grown past the module's maximum, exit 2 a table grown past the cap, \
         and MemoryExhausted the module's own maximum read as the host's limit"
    );
    assert_eq!(transcript.denials().memory(), 0);
    assert_eq!(transcript.denials().table(), 1);
}

/// A command whose memory starts with a data segment of `bytes` bytes, and whose `_start` does nothing.
fn with_data(bytes: usize) -> Vec<u8> {
    wat::parse_str(format!(
        "(module (memory (export \"memory\") 1) (data (i32.const 500) \"{}\") (func (export \"_start\")))",
        "x".repeat(bytes)
    ))
    .expect("valid WAT")
}

#[test]
fn instantiation_copies_data_segments_with_the_guests_own_fuel_on_every_platform() {
    let small = run(&with_data(8), &invocation());
    let large = run(&with_data(1000), &invocation());
    assert_eq!(small.stop(), SealedStop::Returned);
    assert_eq!(
        (small.fuel_spent(), large.fuel_spent()),
        (11, 1003),
        "wasmtime copies data segments with a compiled startup function whose fuel is the \
         guest's; a copy-on-write image would skip it where the platform has one, which is \
         Linux alone, and make the same guest spend differently on each platform"
    );
    let mut starved = invocation();
    starved.fuel = 100;
    let transcript = run(&with_data(1000), &starved);
    assert_eq!(
        transcript.stop(),
        SealedStop::FuelExhausted,
        "running out of fuel while instantiating is a stop like any other"
    );
    assert_eq!(transcript.fuel_spent(), 100);
}

/// Whether `T` may be handed to and shared between threads, which the compiler answers.
const fn shared_between_threads<T: Send + Sync>() {}

#[test]
fn one_module_answers_the_same_on_many_threads_at_once() {
    const _: () = shared_between_threads::<SealedRunner>();
    const _: () = shared_between_threads::<rust_mutants_sealed::SealedModule<'static>>();
    let bytes = command(
        &["random_get", "clock_time_get"],
        "",
        "(call $expect (call $random_get (i32.const 100) (i32.const 32)) (i32.const 0))
         (call $expect (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 200)) (i32.const 0))
         (call $emit (i32.const 100) (i32.const 32))
         (call $emit (i32.const 200) (i32.const 8))",
    );
    let runner = runner();
    let module = runner.prepare(&bytes).expect("the command is valid");
    let alone = module
        .invoke(&invocation(), &uninterrupted())
        .expect("an answer about the guest");
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_worker| {
                let module = &module;
                njutest_devkit::thread::ScopedThread::launch(scope, move || {
                    (0..8)
                        .map(|_run| {
                            module
                                .invoke(&invocation(), &uninterrupted())
                                .expect("an answer")
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for worker in workers {
            for transcript in worker.join().expect("the worker answers") {
                assert_eq!(transcript, alone, "a thread beside another changes nothing");
            }
        }
    });
}

#[test]
fn the_watchdog_stops_a_runaway_guest_as_an_error_and_never_as_a_stop() {
    let bytes = command(&[], "", "(loop $again (br $again))");
    let runner = SealedRunner::new(Duration::from_millis(100)).expect("the runner starts");
    let module = runner.prepare(&bytes).expect("the command is valid");
    let mut endless = invocation();
    endless.fuel = u64::MAX;
    match module.invoke(&endless, &uninterrupted()) {
        Err(SealedError::WatchdogExpired { limit }) => {
            assert_eq!(limit, Duration::from_millis(100));
        }
        other => panic!("the watchdog is an error, not an answer: {other:?}"),
    }
}

#[test]
fn an_interrupt_stops_a_runaway_guest_as_an_error_before_its_watchdog_would() {
    let bytes = command(&[], "", "(loop $again (br $again))");
    let runner = SealedRunner::new(Duration::from_secs(5)).expect("the runner starts");
    let module = runner.prepare(&bytes).expect("the command is valid");
    let mut endless = invocation();
    endless.fuel = u64::MAX;
    let raised = Arc::new(AtomicBool::new(false));
    let interrupt = Interrupt::of(vec![Arc::clone(&raised)]);
    let answer = std::thread::scope(|scope| {
        let stopping = njutest_devkit::thread::ScopedThread::launch(scope, || {
            std::thread::sleep(Duration::from_millis(50));
            raised.store(true, Ordering::SeqCst);
        });
        let answer = module.invoke(&endless, &interrupt);
        stopping.join().expect("the caller stops");
        answer
    });
    assert!(
        matches!(answer, Err(SealedError::Interrupted)),
        "a raised interrupt stops the guest at its next epoch, as an error that says nothing \
         about the guest, and not the watchdog five seconds later: {answer:?}"
    );
}

#[test]
fn a_guest_interrupted_before_it_starts_is_never_run() {
    let bytes = command(&[], "", "(loop $again (br $again))");
    let runner = runner();
    let module = runner.prepare(&bytes).expect("the command is valid");
    let raised = Interrupt::of(vec![Arc::new(AtomicBool::new(true))]);
    let answer = module.invoke(&invocation(), &raised);
    assert!(
        matches!(answer, Err(SealedError::Interrupted)),
        "an invocation asked after its caller stopped is refused, not started: {answer:?}"
    );
}
