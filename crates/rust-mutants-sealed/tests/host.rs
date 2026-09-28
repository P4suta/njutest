// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The host's semantics function by function, driven by hand-written WASI commands.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::num::NonZeroU64;
use std::time::Duration;

use rust_mutants_sealed::{
    OverlayState, Preopens, Refusal, RefusalReason, SealedError, SealedRunner, SealedStop,
    TrapKind, WasiFunction,
};

use crate::common::{command, emitted, invocation, run, snapshot};

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
         (call $expect (call $fd_fdstat_set_rights (i32.const 4) (i64.const 2) (i64.const 0)) (i32.const 0))
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

#[test]
fn a_rename_from_one_preopen_into_another_is_answered_xdev() {
    let bytes = command(
        &["path_rename"],
        "(data (i32.const 100) \"seen.txt\")",
        "(call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 8) (i32.const 4) (i32.const 100) (i32.const 8)) (i32.const 75))",
    );
    let mut two = invocation();
    two.preopens = Preopens::new(vec![
        ("/one".to_owned(), snapshot()),
        ("/two".to_owned(), snapshot()),
    ])
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
    let runner = crate::common::runner();
    let module = runner.prepare(&bytes).expect("the command is valid");
    let alone = module
        .invoke(&invocation())
        .expect("an answer about the guest");
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_worker| {
                let module = &module;
                njutest_devkit::thread::ScopedThread::launch(scope, move || {
                    (0..8)
                        .map(|_run| module.invoke(&invocation()).expect("an answer"))
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
    match module.invoke(&endless) {
        Err(SealedError::WatchdogExpired { limit }) => {
            assert_eq!(limit, Duration::from_millis(100));
        }
        other => panic!("the watchdog is an error, not an answer: {other:?}"),
    }
}
