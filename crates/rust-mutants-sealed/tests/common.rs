// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the suite's modules share: hand-written WASI commands, and an invocation to run them with.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use std::fmt::Write as _;
use std::num::NonZeroU64;
use std::time::Duration;

use rust_mutants_sealed::{
    Arguments, ClockPolicy, Environment, Interrupt, Invocation, Limits, Preopen, Preopens,
    SealedRunner, Snapshot, Transcript,
};

/// Every function of WASI preview1 and its signature in the text format, written from the specification rather than from the crate's table, so each checks the other.
pub const SPECIFICATION: [(&str, &str); 46] = [
    ("args_get", "(param i32 i32) (result i32)"),
    ("args_sizes_get", "(param i32 i32) (result i32)"),
    ("environ_get", "(param i32 i32) (result i32)"),
    ("environ_sizes_get", "(param i32 i32) (result i32)"),
    ("clock_res_get", "(param i32 i32) (result i32)"),
    ("clock_time_get", "(param i32 i64 i32) (result i32)"),
    ("fd_advise", "(param i32 i64 i64 i32) (result i32)"),
    ("fd_allocate", "(param i32 i64 i64) (result i32)"),
    ("fd_close", "(param i32) (result i32)"),
    ("fd_datasync", "(param i32) (result i32)"),
    ("fd_fdstat_get", "(param i32 i32) (result i32)"),
    ("fd_fdstat_set_flags", "(param i32 i32) (result i32)"),
    ("fd_fdstat_set_rights", "(param i32 i64 i64) (result i32)"),
    ("fd_filestat_get", "(param i32 i32) (result i32)"),
    ("fd_filestat_set_size", "(param i32 i64) (result i32)"),
    (
        "fd_filestat_set_times",
        "(param i32 i64 i64 i32) (result i32)",
    ),
    ("fd_pread", "(param i32 i32 i32 i64 i32) (result i32)"),
    ("fd_prestat_get", "(param i32 i32) (result i32)"),
    ("fd_prestat_dir_name", "(param i32 i32 i32) (result i32)"),
    ("fd_pwrite", "(param i32 i32 i32 i64 i32) (result i32)"),
    ("fd_read", "(param i32 i32 i32 i32) (result i32)"),
    ("fd_readdir", "(param i32 i32 i32 i64 i32) (result i32)"),
    ("fd_renumber", "(param i32 i32) (result i32)"),
    ("fd_seek", "(param i32 i64 i32 i32) (result i32)"),
    ("fd_sync", "(param i32) (result i32)"),
    ("fd_tell", "(param i32 i32) (result i32)"),
    ("fd_write", "(param i32 i32 i32 i32) (result i32)"),
    ("path_create_directory", "(param i32 i32 i32) (result i32)"),
    (
        "path_filestat_get",
        "(param i32 i32 i32 i32 i32) (result i32)",
    ),
    (
        "path_filestat_set_times",
        "(param i32 i32 i32 i32 i64 i64 i32) (result i32)",
    ),
    (
        "path_link",
        "(param i32 i32 i32 i32 i32 i32 i32) (result i32)",
    ),
    (
        "path_open",
        "(param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)",
    ),
    (
        "path_readlink",
        "(param i32 i32 i32 i32 i32 i32) (result i32)",
    ),
    ("path_remove_directory", "(param i32 i32 i32) (result i32)"),
    (
        "path_rename",
        "(param i32 i32 i32 i32 i32 i32) (result i32)",
    ),
    ("path_symlink", "(param i32 i32 i32 i32 i32) (result i32)"),
    ("path_unlink_file", "(param i32 i32 i32) (result i32)"),
    ("poll_oneoff", "(param i32 i32 i32 i32) (result i32)"),
    ("proc_exit", "(param i32)"),
    ("proc_raise", "(param i32) (result i32)"),
    ("sched_yield", "(result i32)"),
    ("random_get", "(param i32 i32) (result i32)"),
    ("sock_accept", "(param i32 i32 i32) (result i32)"),
    ("sock_recv", "(param i32 i32 i32 i32 i32 i32) (result i32)"),
    ("sock_send", "(param i32 i32 i32 i32 i32) (result i32)"),
    ("sock_shutdown", "(param i32 i32) (result i32)"),
];

/// The WASI signature of `name` in the text format.
fn signature(name: &str) -> &'static str {
    match SPECIFICATION
        .iter()
        .find(|(function, _signature)| *function == name)
    {
        Some((_function, signature)) => signature,
        None => panic!("{name} is not a function of WASI preview1"),
    }
}

/// A WASI command importing `imports` whose `_start` runs `body`, beside `$expect`, which exits 1000 plus a surprise, and `$emit`, which prints memory.
///
/// # Panics
/// The text is not valid WAT, which is a defect in the test that wrote it.
#[must_use]
pub fn command(imports: &[&str], data: &str, body: &str) -> Vec<u8> {
    let mut text = String::from("(module\n");
    for name in ["proc_exit", "fd_write"].iter().chain(
        imports
            .iter()
            .filter(|name| !["proc_exit", "fd_write"].contains(name)),
    ) {
        writeln!(
            text,
            "  (import \"wasi_snapshot_preview1\" \"{name}\" (func ${name} {}))",
            signature(name)
        )
        .expect("writing to a String");
    }
    text.push_str(
        "  (memory (export \"memory\") 1)\n\
         (func $expect (param $got i32) (param $want i32)\n\
           (if (i32.ne (local.get $got) (local.get $want))\n\
             (then (call $proc_exit (i32.add (i32.const 1000) (local.get $got))))))\n\
         (func $emit (param $at i32) (param $len i32)\n\
           (i32.store (i32.const 0) (local.get $at))\n\
           (i32.store (i32.const 4) (local.get $len))\n\
           (call $expect (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)) (i32.const 0)))\n",
    );
    text.push_str(data);
    text.push_str("\n  (func (export \"_start\")\n");
    text.push_str(body);
    text.push_str("))\n");
    wat::parse_str(&text).unwrap_or_else(|error| panic!("the WAT is valid: {error}\n{text}"))
}

/// A runner for one test, its watchdog a backstop far past anything a test here takes.
///
/// # Panics
/// The runner cannot start on this machine.
#[must_use]
pub fn runner() -> SealedRunner {
    SealedRunner::new(Duration::from_secs(60)).expect("the sealed runner starts")
}

/// A snapshot holding one file and one empty directory.
///
/// # Panics
/// The snapshot is refused, which is a defect in the test.
#[must_use]
pub fn snapshot() -> Snapshot {
    Snapshot::builder()
        .file("seen.txt", b"seen".to_vec())
        .and_then(|built| built.directory("empty"))
        .and_then(rust_mutants_sealed::SnapshotBuilder::build)
        .expect("the snapshot is valid")
}

/// `snapshot` preopened as a tree at `path`.
#[must_use]
pub fn tree(path: &str, snapshot: Snapshot) -> Preopen {
    Preopen::Tree {
        path: path.to_owned(),
        snapshot,
    }
}

/// The working directory `directory` of the tree preopened at `tree`.
#[must_use]
pub fn working(tree: &str, directory: &str) -> Preopen {
    Preopen::Working {
        tree: tree.to_owned(),
        directory: directory.to_owned(),
    }
}

/// An invocation with no arguments past the program name, the snapshot preopened at `/sandbox`.
///
/// # Panics
/// An input is refused, which is a defect in the test.
#[must_use]
pub fn invocation() -> Invocation {
    Invocation {
        arguments: Arguments::new(vec!["command".to_owned()]).expect("valid arguments"),
        environment: Environment::new(Vec::new()).expect("valid environment"),
        preopens: Preopens::new(vec![tree("/sandbox", snapshot())]).expect("valid preopens"),
        seed: 11,
        fuel: 10_000_000,
        limits: Limits {
            memory: 4 << 20,
            stdout: 1 << 16,
            stderr: 1 << 16,
            overlay: 1 << 20,
        },
        clock: ClockPolicy {
            realtime_origin: 1_000_000_000_000_000_000,
            monotonic_origin: 5_000_000_000,
            nanos_per_fuel: NonZeroU64::MIN,
        },
        halt: None,
    }
}

/// An interrupt nothing raises, for an invocation nobody stops.
#[must_use]
pub const fn uninterrupted() -> Interrupt {
    Interrupt::of(Vec::new())
}

/// The transcript of `bytes` run as `invocation`, which must be an answer about the guest.
///
/// # Panics
/// The command is refused, or its invocation is no answer about the guest.
#[must_use]
pub fn run(bytes: &[u8], invocation: &Invocation) -> Transcript {
    runner()
        .prepare(bytes)
        .expect("the command is valid")
        .invoke(invocation, &uninterrupted())
        .expect("the invocation is an answer about the guest")
}

/// The little-endian numbers `$emit` wrote to standard output, eight bytes each.
#[must_use]
pub fn emitted(transcript: &Transcript) -> Vec<u64> {
    transcript
        .stdout()
        .bytes()
        .chunks(8)
        .map(|chunk| {
            let mut number = [0_u8; 8];
            number.copy_from_slice(chunk);
            u64::from_le_bytes(number)
        })
        .collect()
}
