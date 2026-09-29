// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rust guests compiled for `wasm32-wasip1` by the pinned toolchain and run sealed: what a Rust program and libtest reach of WASI, and the laws that make a transcript a function of its inputs.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use proptest::prelude::{ProptestConfig, any, prop, prop_assert_eq, proptest};
use rust_mutants_sealed::{
    Arguments, ClockPolicy, Environment, Invocation, Limits, OverlayEntry, OverlayState, Preopen,
    Preopens, Refusal, RefusalReason, SealedModule, SealedRunner, SealedStop, Snapshot, Transcript,
    TrapKind, WasiFunction,
};

include!("support/guests.rs");

/// How long any one guest may run by the wall clock: a backstop, never the bound a test relies on.
const WATCHDOG: Duration = Duration::from_secs(120);

/// Where the test snapshot is preopened: an absolute path of the kind a host tree has.
const SANDBOX: &str = "/home/runner/work/snapshot";

/// Enough fuel for any guest here that ends by itself.
const PLENTY: u64 = 20_000_000_000;

/// A runner for one test.
fn runner() -> SealedRunner {
    SealedRunner::new(WATCHDOG).expect("the sealed runner starts")
}

/// The snapshot the filesystem tests read and change.
fn snapshot() -> Snapshot {
    Snapshot::builder()
        .file("data/hello.txt", b"hello, sealed world".to_vec())
        .and_then(|built| built.file("data/remove-me.txt", b"bye".to_vec()))
        .and_then(|built| built.file("listing/m.txt", b"m".to_vec()))
        .and_then(|built| built.file("listing/b.txt", b"b".to_vec()))
        .and_then(|built| built.directory("empty"))
        .and_then(rust_mutants_sealed::SnapshotBuilder::build)
        .expect("the test snapshot is valid")
}

/// An invocation of the program with `arguments` after its name, the snapshot preopened at [`SANDBOX`].
fn invocation(arguments: &[&str]) -> Invocation {
    let mut all = vec!["program".to_owned()];
    all.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    Invocation {
        arguments: Arguments::new(all).expect("the arguments hold no NUL"),
        environment: Environment::new(vec![
            ("SEALED_SECOND".to_owned(), "two".to_owned()),
            ("SEALED_FIRST".to_owned(), "one".to_owned()),
        ])
        .expect("the environment is valid"),
        preopens: Preopens::new(vec![tree_at(SANDBOX)]).expect("the preopen is valid"),
        seed: 7,
        fuel: PLENTY,
        limits: Limits {
            memory: 64 << 20,
            stdout: 1 << 20,
            stderr: 1 << 20,
            overlay: 16 << 20,
        },
        clock: ClockPolicy {
            realtime_origin: 1_750_000_000_000_000_000,
            monotonic_origin: 1_000_000_000_000,
            nanos_per_fuel: NonZeroU64::MIN,
        },
    }
}

/// The transcript of `invocation`, which must be an answer about the guest.
fn run(module: &SealedModule<'_>, invocation: &Invocation) -> Transcript {
    module
        .invoke(invocation, &rust_mutants_sealed::Interrupt::of(Vec::new()))
        .expect("the invocation is an answer about the guest")
}

/// What the guest wrote to standard output.
fn stdout(transcript: &Transcript) -> String {
    String::from_utf8(transcript.stdout().bytes().to_vec()).expect("the guest writes UTF-8")
}

/// What the guest wrote to standard error.
fn stderr(transcript: &Transcript) -> String {
    String::from_utf8(transcript.stderr().bytes().to_vec()).expect("the guest writes UTF-8")
}

/// The number after `label ` on the line of `text` that starts with it.
fn reading(text: &str, label: &str) -> u128 {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{label} ")))
        .unwrap_or_else(|| panic!("no {label} line in {text:?}"))
        .parse::<u128>()
        .expect("a reading is a number")
}

#[test]
fn arguments_and_environment_reach_the_guest_in_order() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["echo", "left", "right side"]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(
        stdout(&transcript),
        "argument left\nargument right side\nvariable SEALED_FIRST=one\nvariable SEALED_SECOND=two\n"
    );
}

#[test]
fn a_sleep_returns_at_once_having_moved_virtual_time_by_exactly_what_it_asked() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let asked = 10_000_000_000_u128;
    let started = Instant::now();
    let transcript = run(&module, &invocation(&["sleep", "10000"]));
    let took = started.elapsed();
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert!(
        took < Duration::from_secs(5),
        "a ten-second sleep took {took:?} of the wall clock, so it was not virtual"
    );
    assert_eq!(
        u128::from(transcript.waited()),
        asked,
        "the wait moved the clocks by exactly what was asked"
    );
    let output = stdout(&transcript);
    for clock in ["monotonic", "realtime"] {
        let elapsed = reading(&output, clock);
        assert!(
            elapsed >= asked && elapsed - asked < 1_000_000,
            "the {clock} clock moved {elapsed} ns across a sleep of {asked} ns: the sleep and a \
             little fuel, nothing else"
        );
    }
    let origin = u128::from(invocation(&[]).clock.realtime_origin);
    let began = reading(&output, "started");
    assert!(
        began >= origin && began - origin < 100_000_000,
        "the realtime clock starts at its origin and has moved only by the fuel spent: {began}"
    );
    assert_eq!(
        run(&module, &invocation(&["sleep", "10000"])),
        transcript,
        "the clocks read the same in every invocation"
    );
}

#[test]
fn a_spin_wait_on_time_ends_because_spending_fuel_moves_the_clock() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["spin", "5"]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(stdout(&transcript), "spun true\n");
    assert_eq!(transcript.waited(), 0, "a spin waits on nothing");
}

#[test]
fn random_bytes_are_a_function_of_the_seed() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let seeded = |seed| {
        let mut asked = invocation(&["random"]);
        asked.seed = seed;
        stdout(&run(&module, &asked))
    };
    assert_eq!(seeded(1), seeded(1), "one seed, one stream");
    assert_ne!(seeded(1), seeded(2), "another seed, another stream");
    let random = seeded(3);
    let words: Vec<&str> = random.split_whitespace().collect();
    assert_eq!(words.len(), 3, "{random}");
    assert_ne!(
        words.get(1),
        words.get(2),
        "the stream moves on as it is read"
    );
}

#[test]
fn a_snapshot_preopened_at_an_absolute_host_path_is_read_through_it() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(
        &module,
        &invocation(&["read", &format!("{SANDBOX}/data/hello.txt")]),
    );
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(stdout(&transcript), "hello, sealed world");
    assert!(transcript.overlay().is_empty(), "reading changes nothing");
}

/// Where the test snapshot is preopened when a Windows build spelled its root.
const WINDOWS_SANDBOX: &str = r"C:\runner\work\snapshot";

/// The test snapshot preopened at `root`.
fn tree_at(root: &str) -> Preopen {
    Preopen::Tree {
        path: root.to_owned(),
        snapshot: snapshot(),
    }
}

/// The working directory `directory` of the tree preopened at `root`.
fn working_in(root: &str, directory: &str) -> Preopen {
    Preopen::Working {
        tree: root.to_owned(),
        directory: directory.to_owned(),
    }
}

/// An invocation of the program with `arguments`, the snapshot preopened at `root` and its working directory at `directory`.
fn in_working_directory(arguments: &[&str], root: &str, directory: &str) -> Invocation {
    let mut asked = invocation(arguments);
    asked.preopens = Preopens::new(vec![tree_at(root), working_in(root, directory)])
        .expect("the tree and its working directory are valid");
    asked
}

/// What the program printed for `arguments` in the working directory `directory` of the snapshot at `root`, once it returned.
fn printed(
    module: &SealedModule<'_>,
    arguments: &[&str],
    (root, directory): (&str, &str),
) -> String {
    let transcript = run(module, &in_working_directory(arguments, root, directory));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{arguments:?}: {}",
        stderr(&transcript)
    );
    stdout(&transcript)
}

#[test]
fn a_relative_path_is_read_from_the_working_directory_and_an_absolute_one_through_its_tree() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let at = (SANDBOX, "data");
    assert_eq!(
        printed(&module, &["read", "hello.txt"], at),
        "hello, sealed world"
    );
    assert_eq!(printed(&module, &["read", "../listing/m.txt"], at), "m");
    assert_eq!(
        printed(&module, &["read", &format!("{SANDBOX}/listing/b.txt")], at),
        "b",
        "wasi-libc gives a path to the preopen whose name is its longest prefix, the tree's own"
    );
    assert_eq!(
        printed(
            &module,
            &[
                "relay",
                "made.txt",
                &format!("{SANDBOX}/data/made.txt"),
                "one overlay"
            ],
            at
        ),
        "one overlay",
        "what a relative path wrote, the tree's absolute path reads"
    );
    let escaped = run(
        &module,
        &in_working_directory(&["read", "../../outside.txt"], SANDBOX, "data"),
    );
    assert_eq!(escaped.stop(), SealedStop::Exited { code: 2 });
    assert_eq!(
        escaped.refusals(),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 1,
        }]
    );
}

#[test]
fn a_name_the_snapshot_holds_only_in_another_case_is_refused_however_the_build_spelled_it() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    for (root, path) in [
        (SANDBOX, "Hello.txt".to_owned()),
        (SANDBOX, format!("{SANDBOX}/LISTING/m.txt")),
        (WINDOWS_SANDBOX, r"..\Listing\M.TXT".to_owned()),
        (
            WINDOWS_SANDBOX,
            format!(r"{WINDOWS_SANDBOX}\DATA\hello.txt"),
        ),
    ] {
        let transcript = run(
            &module,
            &in_working_directory(&["read", &path], root, "data"),
        );
        assert_eq!(
            transcript.stop(),
            SealedStop::Exited { code: 2 },
            "{path}: {}",
            stdout(&transcript)
        );
        assert_eq!(
            transcript.refusals(),
            [Refusal {
                function: WasiFunction::PathOpen,
                reason: RefusalReason::CaseOnly,
                count: 1,
            }],
            "{path}"
        );
    }
}

#[test]
fn the_temp_dir_of_std_panics_in_its_platform_layer_whatever_tmpdir_names() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let mut asked = invocation(&["temp-dir"]);
    asked.environment = Environment::new(vec![("TMPDIR".to_owned(), format!("{SANDBOX}/empty"))])
        .expect("the environment is valid");
    let transcript = run(&module, &asked);
    assert_eq!(
        transcript.stop(),
        SealedStop::Trapped {
            kind: TrapKind::Unreachable
        }
    );
    let said = stderr(&transcript);
    assert!(
        said.contains("/library/std/src/sys/"),
        "the panic is located in the standard library's platform layer: {said}"
    );
}

#[test]
fn a_file_made_where_a_variable_names_is_written_read_and_removed_leaving_nothing() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let mut asked = invocation(&["scratch", "TMPDIR"]);
    asked.environment = Environment::new(vec![("TMPDIR".to_owned(), format!("{SANDBOX}/empty"))])
        .expect("the environment is valid");
    let transcript = run(&module, &asked);
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(
        stdout(&transcript),
        "read \"kept for a moment\"\npresent afterwards false\n"
    );
    assert!(
        transcript.overlay().is_empty(),
        "a file made and removed leaves nothing: {:?}",
        transcript.overlay()
    );
}

#[test]
fn an_absolute_path_no_other_preopen_names_is_read_from_the_working_directory() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    assert_eq!(
        printed(&module, &["read", "/listing/m.txt"], (SANDBOX, "")),
        "m",
        "wasi-libc strips the root off a path before it matches a preopen, so `/listing/m.txt` \
         is `listing/m.txt` from the working directory"
    );
}

#[test]
fn a_path_a_windows_build_baked_in_reaches_the_tree_however_it_goes_on() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let at = (WINDOWS_SANDBOX, "data");
    assert_eq!(
        printed(
            &module,
            &[
                "read-joined",
                &format!(r"{WINDOWS_SANDBOX}\data"),
                "hello.txt"
            ],
            at
        ),
        format!("{WINDOWS_SANDBOX}\\data/hello.txt\nhello, sealed world"),
        "the guest's std joins with `/` after a root it does not know is one"
    );
    assert_eq!(
        printed(
            &module,
            &["read-joined", WINDOWS_SANDBOX, "listing/m.txt"],
            at
        ),
        format!("{WINDOWS_SANDBOX}/listing/m.txt\nm")
    );
    assert_eq!(
        printed(
            &module,
            &["read", r"c:\runner\work\snapshot\listing\b.txt"],
            at
        ),
        "b"
    );
    assert_eq!(printed(&module, &["read", r"..\listing\m.txt"], at), "m");
    assert_eq!(
        printed(
            &module,
            &[
                "relay",
                "made.txt",
                &format!(r"{WINDOWS_SANDBOX}\data\made.txt"),
                "one overlay"
            ],
            at
        ),
        "one overlay"
    );
    let elsewhere = run(
        &module,
        &in_working_directory(
            &["read", r"C:\runner\work\outside.txt"],
            WINDOWS_SANDBOX,
            "data",
        ),
    );
    assert_eq!(elsewhere.stop(), SealedStop::Exited { code: 2 });
    assert_eq!(
        elsewhere.refusals(),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 1,
        }]
    );
}

#[test]
fn a_directory_lists_in_name_order_whatever_order_its_entries_were_made_in() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let listing = format!("{SANDBOX}/listing");
    let transcript = run(
        &module,
        &invocation(&["populate", &listing, "z.txt", "a.txt", "k.txt"]),
    );
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(stdout(&transcript), "a.txt\nb.txt\nk.txt\nm.txt\nz.txt\n");
}

#[test]
fn writes_are_seen_by_later_reads_and_by_no_other_invocation() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let fresh = format!("{SANDBOX}/fresh.txt");
    let asked = invocation(&["write", &fresh, "written in the overlay"]);
    let first = run(&module, &asked);
    let second = run(&module, &asked);
    for transcript in [&first, &second] {
        assert_eq!(
            transcript.stop(),
            SealedStop::Returned,
            "{}",
            stderr(transcript)
        );
        assert_eq!(
            stdout(transcript),
            "absent\nwritten in the overlay",
            "each invocation starts from the snapshot, and reads back what it wrote"
        );
    }
    assert_eq!(
        first.overlay(),
        [OverlayEntry {
            path: fresh,
            state: OverlayState::File {
                contents: b"written in the overlay".to_vec(),
                accessed: 0,
                modified: 0,
            },
        }]
    );
    assert_eq!(first, second, "two invocations share nothing");
}

#[test]
fn the_overlay_holds_exactly_what_the_guest_changed() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["rearrange", SANDBOX]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert_eq!(
        stdout(&transcript),
        "out/b.txt holds \"moved\"\ndata/hello.txt holds \"hello\"\ndata/remove-me.txt is \
         NotFound\nout/c.txt is NotFound\n"
    );
    let at = |relative: &str| format!("{SANDBOX}/{relative}");
    assert_eq!(
        transcript.overlay(),
        [
            OverlayEntry {
                path: at("data/hello.txt"),
                state: OverlayState::File {
                    contents: b"hello".to_vec(),
                    accessed: 0,
                    modified: 0,
                },
            },
            OverlayEntry {
                path: at("data/remove-me.txt"),
                state: OverlayState::Removed,
            },
            OverlayEntry {
                path: at("out"),
                state: OverlayState::Directory {
                    accessed: 0,
                    modified: 0,
                },
            },
            OverlayEntry {
                path: at("out/b.txt"),
                state: OverlayState::File {
                    contents: b"moved".to_vec(),
                    accessed: 0,
                    modified: 1_000_000_000_000,
                },
            },
        ]
    );
}

#[test]
fn exit_3_is_exited_3() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["exit", "3"]));
    assert_eq!(transcript.stop(), SealedStop::Exited { code: 3 });
}

#[test]
fn a_panic_traps_unreachable_with_the_message_on_stderr() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["panic"]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Trapped {
            kind: TrapKind::Unreachable
        }
    );
    let said = stderr(&transcript);
    assert!(
        said.contains("panicked at") && said.contains("the guest panicked on purpose"),
        "{said}"
    );
}

#[test]
fn an_infinite_loop_spends_the_whole_budget() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let mut asked = invocation(&["spin-forever"]);
    asked.fuel = 50_000_000;
    let transcript = run(&module, &asked);
    assert_eq!(transcript.stop(), SealedStop::FuelExhausted);
    assert_eq!(transcript.fuel_spent(), asked.fuel);
}

#[test]
fn an_allocation_past_the_memory_limit_is_memory_exhausted() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let mut asked = invocation(&["allocate", "67108864"]);
    asked.limits.memory = 16 << 20;
    let transcript = run(&module, &asked);
    assert_eq!(
        transcript.stop(),
        SealedStop::MemoryExhausted,
        "{}",
        stderr(&transcript)
    );
    assert!(
        transcript.denials().memory() >= 1,
        "{:?}",
        transcript.denials()
    );
    assert!(
        transcript
            .denials()
            .memory_requests()
            .iter()
            .all(|request| *request > asked.limits.memory),
        "every refused growth asked for more than the limit: {:?}",
        transcript.denials()
    );
    assert!(transcript.peak_memory() <= asked.limits.memory);
    assert!(
        stderr(&transcript).contains("memory allocation of 67108864 bytes failed"),
        "{}",
        stderr(&transcript)
    );
}

#[test]
fn deep_recursion_overflows_the_stack() {
    let runner = runner();
    let module = runner
        .prepare(&program_bytes())
        .expect("the program is a WASI command");
    let transcript = run(&module, &invocation(&["recurse"]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Trapped {
            kind: TrapKind::StackOverflow
        },
        "{}",
        stderr(&transcript)
    );
}

/// An invocation of the libtest harness with `arguments` after its name, and nothing preopened.
fn harness(arguments: &[&str]) -> Invocation {
    let mut asked = invocation(&[]);
    let mut all = vec!["sealed-libtest-guest.wasm".to_owned()];
    all.extend(arguments.iter().map(|argument| (*argument).to_owned()));
    asked.arguments = Arguments::new(all).expect("the arguments hold no NUL");
    asked.preopens = Preopens::new(Vec::new()).expect("no preopens");
    asked
}

/// The arguments that run exactly one test, on the harness's only thread, its output uncaptured.
fn one_test(name: &str) -> Invocation {
    harness(&["--exact", name, "--test-threads", "1", "--nocapture"])
}

#[test]
fn libtest_lists_its_tests() {
    let runner = runner();
    let module = runner
        .prepare(&libtest_bytes())
        .expect("the harness is a WASI command");
    let transcript = run(&module, &harness(&["--list"]));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    let listed = stdout(&transcript);
    for test in ["tests::passes", "tests::panics", "tests::returns_an_error"] {
        assert!(listed.contains(&format!("{test}: test")), "{listed}");
    }
}

#[test]
fn a_passing_test_returns() {
    let runner = runner();
    let module = runner
        .prepare(&libtest_bytes())
        .expect("the harness is a WASI command");
    let transcript = run(&module, &one_test("tests::passes"));
    assert_eq!(
        transcript.stop(),
        SealedStop::Returned,
        "{}",
        stderr(&transcript)
    );
    assert!(
        stdout(&transcript).contains("test tests::passes ... ok"),
        "{}",
        stdout(&transcript)
    );
}

#[test]
fn a_panicking_test_traps_with_its_message() {
    let runner = runner();
    let module = runner
        .prepare(&libtest_bytes())
        .expect("the harness is a WASI command");
    let transcript = run(&module, &one_test("tests::panics"));
    assert_eq!(
        transcript.stop(),
        SealedStop::Trapped {
            kind: TrapKind::Unreachable
        }
    );
    assert!(
        stderr(&transcript).contains("the failing test panicked on purpose"),
        "{}",
        stderr(&transcript)
    );
}

#[test]
fn a_test_returning_err_exits_101() {
    let runner = runner();
    let module = runner
        .prepare(&libtest_bytes())
        .expect("the harness is a WASI command");
    let transcript = run(&module, &one_test("tests::returns_an_error"));
    assert_eq!(transcript.stop(), SealedStop::Exited { code: 101 });
    let said = format!("{}{}", stdout(&transcript), stderr(&transcript));
    assert!(
        said.contains("the failing test returned an error on purpose"),
        "{said}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn the_same_invocation_twice_is_the_same_transcript(
        seed in any::<u64>(),
        words in prop::collection::vec(0_u32..10_000, 0..4),
    ) {
        let runner = runner();
        let module = runner.prepare(&program_bytes()).expect("the program is a WASI command");
        for mode in ["echo", "random", "sleep"] {
            let mut arguments = vec![mode.to_owned()];
            if mode == "sleep" {
                arguments.push(words.first().copied().unwrap_or(1).to_string());
            } else {
                arguments.extend(words.iter().map(|word| format!("word{word}")));
            }
            let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
            let mut asked = invocation(&borrowed);
            asked.seed = seed;
            let first = run(&module, &asked);
            let second = run(&module, &asked);
            prop_assert_eq!(first.digest(), second.digest());
            prop_assert_eq!(first.invocation(), second.invocation());
            prop_assert_eq!(&first, &second);
        }
    }
}
