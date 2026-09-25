// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guards as a measurement: a guard records which of the process's threads reached it, and libtest names a thread after its test.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use rust_mutants::instrument::{
    ACTIVE_ENV, Instrumenting, STEP_NONCE_ENV, STEP_NOTICE_ENV, STEP_PROTOCOL_EXIT, STEP_STATE_ENV,
    STEP_STATE_SCHEMA, STEPS_ENV, instrument_file,
};
use rust_mutants::instrument::{
    CATALOG_ENV, MODULE_STEM, Rendering, TOUCH_ENV, WATCHED_ENV, render,
};
use rust_mutants::rule::Tier;
use rust_mutants::testkit::compile::ScriptedCompile;
use rust_mutants::touch::{self, TouchError};

const SOURCE: &str = "pub fn one(a: i32) -> i32 { a + 1 }\n\
                      pub fn two(a: i32) -> i32 { a - 1 }\n\
                      pub fn three(a: i32) -> i32 { a * 2 }\n";

const CATALOG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The item index the file's first item takes, which is not zero so that an offset is exercised.
const FIRST_ITEM: u32 = 5;

/// The watched directory every runtime here is built with, which every process here is started carrying, so none of them has anything to say there.
const WATCHED: &str = "/nowhere-a-process-of-this-test-writes";

/// How many items the file holds.
const ITEMS: u32 = 3;

fn exact_output(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("the generated fixture writes exact UTF-8")
}

/// Owns one hostile fixture process until it has been forcibly reaped.
struct FixtureChild {
    child: Option<Child>,
}

impl FixtureChild {
    fn launch(command: &mut Command) -> std::io::Result<Self> {
        Ok(Self {
            child: Some(command.spawn()?),
        })
    }

    fn terminate(mut self) -> std::io::Result<()> {
        let Some(mut child) = self.child.take() else {
            return Err(std::io::Error::other(
                "the fixture process was already reaped",
            ));
        };
        child.kill()?;
        child.wait()?;
        Ok(())
    }
}

impl Drop for FixtureChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let killed = child.kill();
        let waited = child.wait();
        if killed.is_err() || waited.is_err() {
            std::process::abort();
        }
    }
}

fn remove_if_present(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn regular_file_present(path: &std::path::Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(std::io::Error::other(
            "the notice path is not a regular file",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_optional_text(path: &std::path::Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The runtime module of a file whose mutants are the ones `SOURCE` yields, named `__rm`.
fn module() -> (String, u32) {
    let scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All);
    let placements = scripted.placements();
    let count = u32::try_from(placements.len()).expect("a small catalog");
    assert!(count >= 3, "the source yields mutants to reach: {count}");
    let rendered = render(&Rendering {
        module: MODULE_STEM,
        catalog_digest: CATALOG,
        placements,
        markers: &[],
        first_item: FIRST_ITEM,
        item_count: ITEMS,
        newline: "\n",
        watched: WATCHED,
    })
    .expect("a nonempty small catalog has a representable runtime window");
    (rendered, count)
}

/// Two file-local runtimes naming the same catalog, plus one selected identity.
fn step_modules() -> (String, String, String) {
    let scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All);
    let placements = scripted.placements();
    let selected = placements
        .first()
        .expect("the source yields a mutant")
        .id
        .clone();
    let one = render(&Rendering {
        module: "__rm_one",
        catalog_digest: CATALOG,
        placements,
        markers: &[],
        first_item: 0,
        item_count: 0,
        newline: "\n",
        watched: WATCHED,
    })
    .expect("the first runtime renders");
    let two = render(&Rendering {
        module: "__rm_two",
        catalog_digest: CATALOG,
        placements,
        markers: &[],
        first_item: 0,
        item_count: 0,
        newline: "\n",
        watched: WATCHED,
    })
    .expect("the second runtime renders");
    (one, two, selected)
}

/// Builds a program around the runtime, runs it, and returns what it wrote to the touch log.
fn ran(name: &str, body: &str, touching: bool) -> String {
    let (module, _) = module();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join(format!("{name}.rs"));
    std::fs::write(&source, format!("{module}\nfn main() {{\n{body}\n}}\n")).expect("write");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let log = dir.join(format!("{name}.touch"));
    remove_if_present(&log).expect("remove old touch log");
    let mut command = Command::new(dir.join(name));
    command.env_remove("RUST_MUTANTS_ACTIVE");
    command.env(CATALOG_ENV, CATALOG);
    command.env(WATCHED_ENV, WATCHED);
    if touching {
        command.env(TOUCH_ENV, &log);
    } else {
        command.env_remove(TOUCH_ENV);
    }
    let output = command.output().expect("the program runs");
    assert!(output.status.success(), "{}", exact_output(&output.stderr));
    match read_optional_text(&log).expect("read touch log") {
        Some(text) => text,
        None => String::new(),
    }
}

#[test]
fn one_allowance_spans_file_modules_and_repeated_guard_checks_do_not_spend_twice() {
    let (one, two, selected) = step_modules();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join("global_steps.rs");
    let notice = dir.join("step.notice");
    let state = dir.join("step.state");
    let continued = dir.join("continued");
    let nonce = "0123456789abcdef0123456789abcdef";
    std::fs::write(
        &state,
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t2\tdormant\t0\n"),
    )
    .expect("initial state");
    let program = format!(
        "{one}\n{two}\nfn main() {{\n\
         \x20   let worker = std::thread::spawn(|| {{\n\
         \x20       assert!(__rm_one::active(0));\n\
         \x20       assert!(__rm_one::active(0));\n\
         \x20       __rm_two::checkpoint();\n\
         \x20       std::fs::write({continued:?}, b\"continued\").expect(\"marker\");\n\
         \x20       __rm_two::checkpoint();\n\
         \x20   }});\n\
         \x20   for _ in 0..2_000 {{\n\
         \x20       let settled = std::fs::read_to_string({state:?})\n\
         \x20           .map(|text| text.contains(\"stopping\"))\n\
         \x20           .unwrap_or(false);\n\
         \x20       if settled {{ break; }}\n\
         \x20       if worker.is_finished() {{ panic!(\"publisher stopped before settling\"); }}\n\
         \x20       std::thread::sleep(std::time::Duration::from_millis(1));\n\
         \x20   }}\n\
         \x20   assert!(std::fs::metadata({notice:?}).is_ok(), \"notice was published\");\n\
         }}\n"
    );
    std::fs::write(&source, program).expect("write program");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let run = Command::new(dir.join("global_steps"))
        .env(ACTIVE_ENV, &selected)
        .env(CATALOG_ENV, CATALOG)
        .env(WATCHED_ENV, WATCHED)
        .env(STEPS_ENV, "2")
        .env(STEP_NONCE_ENV, nonce)
        .env(STEP_NOTICE_ENV, &notice)
        .env(STEP_STATE_ENV, &state)
        .output()
        .expect("program runs");
    assert!(run.status.success(), "{}", exact_output(&run.stderr));
    assert_eq!(std::fs::read(&continued).expect("continued"), b"continued");
    assert_eq!(
        std::fs::read_to_string(&notice).expect("notice"),
        format!("rust-mutants-step-notice-v1\t{nonce}\t{CATALOG}\t{selected}\t2\t3\n")
    );
    assert_eq!(
        std::fs::read_to_string(&state).expect("state"),
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t2\tstopping\t3\n")
    );
}

/// How long a wait for a fixture process runs before it fails, sized for a machine building several workspaces at once, because a wait that decides by the clock reports the machine.
const BACKSTOP: Duration = Duration::from_secs(120);

/// Waits until `path` is a regular file, or fails with `escaped` once the backstop passes.
fn wait_until_present(path: &std::path::Path, escaped: &str) {
    let deadline = Instant::now()
        .checked_add(BACKSTOP)
        .expect("a deadline one backstop from now");
    while !regular_file_present(path).expect("inspect notice") {
        assert!(Instant::now() < deadline, "{escaped}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Waits until `state` reads exactly `wanted`, or fails saying what it read instead.
///
/// A read can fail rather than answer while the process that owns the file is writing it: a Windows lock is mandatory where a POSIX one is advisory, so unreadable here means the same as not yet.
fn wait_until_state_reads(state: &std::path::Path, wanted: &str) {
    let deadline = Instant::now()
        .checked_add(BACKSTOP)
        .expect("a deadline one backstop from now");
    loop {
        match std::fs::read_to_string(state) {
            Ok(observed) if observed == wanted => return,
            Ok(observed) => assert!(
                Instant::now() < deadline,
                "the recoverable publisher did not reach its stopping state: {observed:?}"
            ),
            Err(error) => assert!(
                Instant::now() < deadline,
                "the recoverable publisher's state stayed unreadable: {error}"
            ),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn publication_failure_never_persists_a_stopping_state_without_a_final_notice() {
    let (module, _, selected) = step_modules();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join("publication_failure.rs");
    let notice = dir.join("step.notice");
    let partial = dir.join("step.notice.partial");
    let state = dir.join("step.state");
    let nonce = "0123456789abcdef0123456789abcdef";
    std::fs::write(
        &state,
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1\tdormant\t0\n"),
    )
    .expect("initial state");
    std::fs::write(&partial, b"occupied").expect("block publication");
    std::fs::write(
        &source,
        format!(
            "{module}\nfn main() {{ assert!(__rm_one::active(0)); __rm_one::checkpoint(); }}\n"
        ),
    )
    .expect("write program");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let configured = |command: &mut Command| {
        command
            .env(ACTIVE_ENV, &selected)
            .env(CATALOG_ENV, CATALOG)
            .env(WATCHED_ENV, WATCHED)
            .env(STEPS_ENV, "1")
            .env(STEP_NONCE_ENV, nonce)
            .env(STEP_NOTICE_ENV, &notice)
            .env(STEP_STATE_ENV, &state);
    };
    let mut first = Command::new(dir.join("publication_failure"));
    configured(&mut first);
    let failed = first.output().expect("first process runs");
    assert_eq!(failed.status.code(), Some(STEP_PROTOCOL_EXIT));
    assert_eq!(
        std::fs::read_to_string(&state).expect("recoverable state"),
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1\tactive\t1\n")
    );
    assert!(matches!(
        std::fs::symlink_metadata(&notice),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ));

    std::fs::remove_file(&partial).expect("unblock publication");
    let mut second = Command::new(dir.join("publication_failure"));
    configured(&mut second);
    let child = FixtureChild::launch(&mut second).expect("second process starts");
    let wanted = format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1\tstopping\t2\n");
    wait_until_state_reads(&state, &wanted);
    assert!(
        regular_file_present(&notice).expect("the notice publication preceded the stopping state"),
    );
    child.terminate().expect("second process is reaped");
    assert_eq!(
        std::fs::read_to_string(&state).expect("stopping state"),
        wanted
    );
}

#[cfg(unix)]
#[test]
fn the_generated_runtime_refuses_a_step_state_symlink_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let (module, _, selected) = step_modules();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join("state_symlink.rs");
    let state = dir.join("step.state");
    let target = dir.join("must-not-change");
    let notice = dir.join("step.notice");
    let nonce = "0123456789abcdef0123456789abcdef";
    std::fs::write(&target, b"sentinel").expect("target");
    symlink(&target, &state).expect("state symlink");
    std::fs::write(
        &source,
        format!("{module}\nfn main() {{ assert!(__rm_one::active(0)); }}\n"),
    )
    .expect("write program");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let run = Command::new(dir.join("state_symlink"))
        .env(ACTIVE_ENV, &selected)
        .env(CATALOG_ENV, CATALOG)
        .env(WATCHED_ENV, WATCHED)
        .env(STEPS_ENV, "2")
        .env(STEP_NONCE_ENV, nonce)
        .env(STEP_NOTICE_ENV, &notice)
        .env(STEP_STATE_ENV, &state)
        .output()
        .expect("program runs");
    assert_eq!(run.status.code(), Some(STEP_PROTOCOL_EXIT));
    assert_eq!(std::fs::read(&target).expect("target"), b"sentinel");
    assert!(matches!(
        std::fs::symlink_metadata(&notice),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ));
}

#[test]
fn an_expression_closure_reentered_by_an_external_iterator_spends_the_global_allowance() {
    let source = "fn main() { let _never = std::iter::repeat(()).position(|_| true); }\n";
    let scripted = ScriptedCompile::from_source("src/main.rs", source, Tier::All);
    let selected = scripted
        .placements()
        .iter()
        .find(|placement| placement.original == b"true" && placement.replacement == b"false")
        .expect("the closure boolean yields the hostile mutant");
    let comparable = BTreeSet::new();
    let probed = BTreeMap::new();
    let instrumented = instrument_file(&Instrumenting {
        path: "src/main.rs",
        source: source.as_bytes(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &comparable,
        probed: &probed,
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: WATCHED,
    })
    .expect("instrument expression closure");
    assert!(
        instrumented.text.contains("|_| { ")
            && instrumented.text.contains("::checkpoint(); ")
            && instrumented.text.contains("::value!(if "),
        "the expression body is a charged block around the guarded expression: {}",
        instrumented.text
    );

    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let path = dir.join("expression_closure.rs");
    let notice = dir.join("step.notice");
    let state = dir.join("step.state");
    let nonce = "0123456789abcdef0123456789abcdef";
    std::fs::write(
        &state,
        format!(
            "{STEP_STATE_SCHEMA}\t{nonce}\t{}\t{}\t2\tdormant\t0\n",
            scripted.catalog().digest(),
            selected.id
        ),
    )
    .expect("initial state");
    std::fs::write(&path, &instrumented.text).expect("write instrumented source");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&path)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let mut command = Command::new(dir.join("expression_closure"));
    command
        .env(ACTIVE_ENV, &selected.id)
        .env(CATALOG_ENV, scripted.catalog().digest())
        .env(WATCHED_ENV, WATCHED)
        .env(STEPS_ENV, "2")
        .env(STEP_NONCE_ENV, nonce)
        .env(STEP_NOTICE_ENV, &notice)
        .env(STEP_STATE_ENV, &state);
    let child = FixtureChild::launch(&mut command).expect("program starts");
    wait_until_present(&notice, "the expression closure escaped the allowance");
    child.terminate().expect("fixture process is reaped");
    assert_eq!(
        std::fs::read_to_string(&notice).expect("notice"),
        format!(
            "rust-mutants-step-notice-v1\t{nonce}\t{}\t{}\t2\t3\n",
            scripted.catalog().digest(),
            selected.id
        )
    );
}

fn set(indices: &[u32]) -> BTreeSet<u32> {
    indices.iter().copied().collect()
}

#[test]
fn a_guard_records_the_thread_that_reached_it_and_libtest_names_that_thread_after_its_test() {
    let (_, count) = module();
    let text = ran(
        "named",
        "    let alpha = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); __rm::active(0); __rm::active(1); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   alpha.join().expect(\"join\");\n\
         \x20   let beta = std::thread::Builder::new().name(\"beta\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(1); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   beta.join().expect(\"join\");",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: 0,
        },
    )
    .expect("the log reads");
    assert_eq!(touches.reached.tests.get("alpha"), Some(&set(&[0, 1])));
    assert_eq!(touches.reached.tests.get("beta"), Some(&set(&[1])));
    assert!(
        touches.reached.loose.is_empty(),
        "every touch was on a named thread: {touches:?}"
    );
}

#[test]
fn a_touch_nothing_can_be_attributed_to_is_recorded_as_one_rather_than_dropped() {
    let (_, count) = module();
    let text = ran(
        "loose",
        "    std::thread::spawn(|| { __rm::active(2); }).join().expect(\"join\");\n\
         \x20   __rm::active(0);",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: 0,
        },
    )
    .expect("the log reads");
    assert!(
        touches.reached.tests.is_empty(),
        "neither the main thread nor an unnamed one is a test: {touches:?}"
    );
    assert_eq!(
        touches.reached.loose,
        set(&[0, 2]),
        "a site a run cannot attribute has to reach every test of its target"
    );
}

#[test]
fn a_marker_records_that_control_entered_the_body_a_condition_gates() {
    let (_, count) = module();
    let text = ran(
        "bodies",
        "    let alpha = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); __rm::body(0); __rm::body(0); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   alpha.join().expect(\"join\");\n\
         \x20   let beta = std::thread::Builder::new().name(\"beta\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   beta.join().expect(\"join\");",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: 0,
        },
    )
    .expect("the log reads");
    assert_eq!(
        touches.reached.tests.get("alpha"),
        Some(&set(&[0])),
        "both threads reached the condition"
    );
    assert_eq!(touches.reached.tests.get("beta"), Some(&set(&[0])));
    assert_eq!(
        touches.bodies.tests.get("alpha"),
        Some(&set(&[0])),
        "and one of them entered the body it gates"
    );
    assert_eq!(
        touches.bodies.tests.get("beta"),
        None,
        "a test that evaluated the condition and never took the branch cannot have noticed a \
         mutation that only narrows it"
    );
}

#[test]
fn a_guard_records_the_test_that_saw_its_two_branches_differ() {
    let (_, count) = module();
    let text = ran(
        "differed",
        "    let alpha = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| {\n\
         \x20           assert!(__rm::differing(0, true, || false));\n\
         \x20           assert!(!__rm::differing(1, false, || false));\n\
         \x20       })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   alpha.join().expect(\"join\");\n\
         \x20   let beta = std::thread::Builder::new().name(\"beta\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   beta.join().expect(\"join\");",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: 0,
        },
    )
    .expect("the log reads");
    assert_eq!(
        touches.infected.tests.get("alpha"),
        Some(&set(&[0])),
        "the test that saw the two branches differ is the one that could have noticed"
    );
    assert_eq!(
        touches.infected.tests.get("beta"),
        None,
        "a test that ran the site and never saw it differ cannot have noticed the mutation"
    );
    assert_eq!(
        touches.reached.tests.get("beta"),
        Some(&set(&[0])),
        "though it did reach it"
    );
    assert_eq!(
        touches.infected.tests.get("alpha").map(BTreeSet::len),
        Some(1),
        "a guard whose two branches answered the same records nothing about that mutant"
    );
}

#[test]
fn a_run_with_nothing_to_record_never_evaluates_the_branch_it_would_have_compared() {
    let text = ran(
        "uncompared",
        "    let mut evaluated = false;\n\
         \x20   assert!(__rm::differing(0, true, || { evaluated = true; false }));\n\
         \x20   assert!(!evaluated, \"a run that records nothing pays nothing for the comparison\");",
        false,
    );
    assert!(text.is_empty(), "and writes no record at all: {text:?}");
}

#[test]
fn a_process_built_from_another_catalog_writes_nothing_into_this_run_s_record() {
    let (module, _) = module();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join("foreign.rs");
    std::fs::write(
        &source,
        format!("{module}\nfn main() {{ __rm::active(0); __rm::body(0); }}\n"),
    )
    .expect("write");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let log = dir.join("foreign.touch");
    remove_if_present(&log).expect("remove old touch log");
    let output = Command::new(dir.join("foreign"))
        .env_remove("RUST_MUTANTS_ACTIVE")
        .env(TOUCH_ENV, &log)
        .env(CATALOG_ENV, "b".repeat(64))
        .env(WATCHED_ENV, WATCHED)
        .output()
        .expect("the program runs");
    assert!(
        output.status.success(),
        "a binary of another catalog is not this run's to refuse, only its record to stay out \
         of: {}",
        exact_output(&output.stderr)
    );
    assert!(
        matches!(
            std::fs::symlink_metadata(&log),
            Err(ref error) if error.kind() == std::io::ErrorKind::NotFound
        ),
        "a record is about one catalog, and a process built from another writing into it is what \
         makes the whole of it unreadable: {:?}",
        std::fs::read_to_string(&log)
    );
}

#[test]
fn a_process_nothing_asked_to_record_writes_no_log_at_all() {
    let text = ran(
        "silent",
        "    let one = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   one.join().expect(\"join\");",
        false,
    );
    assert!(text.is_empty(), "{text}");
}

#[test]
fn the_header_the_runtime_writes_is_the_one_the_reader_expects() {
    let text = ran(
        "header",
        "    let one = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::active(0); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   one.join().expect(\"join\");",
        true,
    );
    assert!(
        text.starts_with(&touch::header_line(CATALOG)),
        "the generated runtime and the reader are two halves of one format, and this is where \
         they are held to each other: {text}"
    );
}

#[test]
fn a_log_about_another_catalog_says_nothing_rather_than_something_wrong() {
    let text = format!("{} {}\nt\talpha\t0\n", touch::SCHEMA, "b".repeat(64));
    assert!(matches!(
        touch::read(
            &text,
            CATALOG,
            touch::Bounds {
                mutants: 4,
                items: 0
            }
        ),
        Err(TouchError::OtherCatalog { .. })
    ));
}

#[test]
fn a_line_naming_a_site_the_catalog_does_not_hold_says_nothing_at_all() {
    let text = format!(
        "{touch_schema} {CATALOG}\nt\talpha\t0,9\n",
        touch_schema = touch::SCHEMA
    );
    assert!(matches!(
        touch::read(
            &text,
            CATALOG,
            touch::Bounds {
                mutants: 4,
                items: 0
            }
        ),
        Err(TouchError::BeyondCatalog { index: 9, .. })
    ));
}

#[test]
fn a_record_before_any_header_says_nothing_because_nothing_says_which_catalog_it_is_about() {
    assert!(matches!(
        touch::read(
            "t\talpha\t0\n",
            CATALOG,
            touch::Bounds {
                mutants: 4,
                items: 0
            }
        ),
        Err(TouchError::Headless { .. })
    ));
}

#[test]
fn a_line_of_a_kind_this_reader_does_not_know_says_nothing_at_all() {
    let text = format!("{schema} {CATALOG}\nz\talpha\t0\n", schema = touch::SCHEMA);
    assert!(matches!(
        touch::read(
            &text,
            CATALOG,
            touch::Bounds {
                mutants: 4,
                items: 0
            }
        ),
        Err(TouchError::Malformed { .. })
    ));
}

#[test]
fn the_same_thread_named_twice_is_one_test_that_reached_both_lines_worth_of_sites() {
    let text = format!(
        "{schema} {CATALOG}\nt\talpha\t0,1\nt\talpha\t2\n",
        schema = touch::SCHEMA
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: 4,
            items: 0,
        },
    )
    .expect("the log reads");
    assert_eq!(
        touches.reached.tests.get("alpha"),
        Some(&set(&[0, 1, 2])),
        "a thread that reached more sites than one line holds is still one test"
    );
}

/// Compiles a library around the runtime, returning what rustc said.
fn built(name: &str, prefix: &str) -> std::process::Output {
    let (module, _) = module();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join(format!("{name}.rs"));
    std::fs::write(&source, format!("{prefix}{module}")).expect("write");
    Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "lib"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs")
}

#[test]
fn recording_costs_a_crate_neither_its_prelude_nor_its_ban_on_unsafe_code() {
    for (name, prefix) in [
        ("freestanding", "#![no_std]\n"),
        ("unsafeless", "#![forbid(unsafe_code)]\n"),
        ("both", "#![no_std]\n#![forbid(unsafe_code)]\n"),
    ] {
        let output = built(name, prefix);
        assert!(
            output.status.success(),
            "{prefix}: {}",
            exact_output(&output.stderr)
        );
    }
}

#[test]
fn an_entry_marker_records_the_thread_that_entered_the_item_by_its_index_in_the_whole_tree() {
    let (_, count) = module();
    let text = ran(
        "entered",
        "    let alpha = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::item(5); __rm::item(5); __rm::item(7); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   alpha.join().expect(\"join\");\n\
         \x20   __rm::item(6);",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: FIRST_ITEM + ITEMS,
        },
    )
    .expect("the log reads");
    assert_eq!(touches.entered.tests.get("alpha"), Some(&set(&[5, 7])));
    assert_eq!(
        touches.entered.loose,
        set(&[6]),
        "the main thread is no test's, so what it entered every test entered"
    );
    assert!(
        touches.reached.tests.is_empty() && touches.reached.loose.is_empty(),
        "an entry is not a site: {touches:?}"
    );
}

#[test]
fn an_entry_marker_says_nothing_when_nobody_asked() {
    let text = ran("entered_quietly", "    __rm::item(5);", false);
    assert!(text.is_empty(), "{text}");
}

#[test]
fn an_item_entered_while_its_thread_is_being_torn_down_is_recorded_rather_than_fatal() {
    let (_, count) = module();
    let text = ran(
        "torn_down",
        "    struct Late;\n\
         \x20   impl Drop for Late { fn drop(&mut self) { __rm::item(6); } }\n\
         \x20   thread_local! { static LATE: Late = const { Late }; }\n\
         \x20   let alpha = std::thread::Builder::new().name(\"alpha\".to_owned())\n\
         \x20       .spawn(|| { __rm::item(5); LATE.with(|_| {}); })\n\
         \x20       .expect(\"spawn\");\n\
         \x20   alpha.join().expect(\"join\");",
        true,
    );
    let touches = touch::read(
        &text,
        CATALOG,
        touch::Bounds {
            mutants: count,
            items: FIRST_ITEM + ITEMS,
        },
    )
    .expect("the log reads");
    assert!(
        touches.entered.by("alpha", 6),
        "whichever of the two thread-locals goes first, the entry is kept, on alpha or on \
         every test: {touches:?}"
    );
}

#[test]
fn a_process_that_lost_the_runs_environment_says_so_where_the_run_looks() {
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let watched = dir.join("watched");
    let watched_text = watched.to_str().expect("a UTF-8 temporary path");
    let scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All);
    let rendered = render(&Rendering {
        module: MODULE_STEM,
        catalog_digest: CATALOG,
        placements: scripted.placements(),
        markers: &[],
        first_item: FIRST_ITEM,
        item_count: ITEMS,
        newline: "\n",
        watched: watched_text,
    })
    .expect("the runtime renders");
    let source = dir.join("orphaned.rs");
    std::fs::write(
        &source,
        format!("{rendered}\nfn main() {{\n    {MODULE_STEM}::item({FIRST_ITEM});\n}}\n"),
    )
    .expect("write");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let left = || rust_mutants::orphan::left(&watched).expect("the watched directory reads");
    let carried = Command::new(dir.join("orphaned"))
        .env(WATCHED_ENV, watched_text)
        .output()
        .expect("the program runs");
    assert!(
        carried.status.success(),
        "{}",
        exact_output(&carried.stderr)
    );
    assert!(
        left().is_empty(),
        "a process carrying what the run gave it has nothing to say: {:?}",
        left()
    );
    let cleared = Command::new(dir.join("orphaned"))
        .env_clear()
        .output()
        .expect("the program runs");
    assert!(
        cleared.status.success(),
        "{}",
        exact_output(&cleared.stderr)
    );
    let said = left();
    assert_eq!(
        said.len(),
        1,
        "a process started with a cleared environment is one no mutant can be active in and \
         whose entries nothing records, and it says so once: {said:?}"
    );
    #[cfg(unix)]
    assert!(
        said.iter()
            .all(|orphan| orphan.parent == std::process::id()),
        "the parent is the process that started it, which is what the run maps it back by: \
         {said:?}"
    );
}

#[test]
fn a_child_that_lost_the_environment_belongs_to_the_execution_that_started_it() {
    use rust_mutants::orphan::{Known, Orphan, ours};
    let child = |parent: u32| Orphan {
        pid: 4_000_000,
        parent,
        at: None,
    };
    let others = Known {
        leaders: BTreeSet::from([4_100_000_u32]),
        members: BTreeMap::new(),
    };
    assert!(
        ours(&child(4_200_000), Some(4_200_000), &others),
        "a child whose parent led this execution is this execution's"
    );
    assert!(
        !ours(&child(4_100_000), Some(4_200_000), &others),
        "a child whose parent led another execution is that one's, however close in time"
    );
    #[cfg(unix)]
    assert!(
        !ours(&child(std::process::id()), Some(4_200_000), &others),
        "a child whose parent is still running belongs to whatever is running"
    );
    #[cfg(not(unix))]
    assert!(
        ours(&child(std::process::id()), Some(4_200_000), &others),
        "a platform that is not asked whether a process still runs cannot rule out that it \
         left this execution's child, so the survival is not read past it"
    );
    assert!(
        ours(&child(0), Some(4_200_000), &others)
            && ours(&child(4_300_000), Some(4_200_000), &others),
        "a parent the platform does not name, or one that has gone and led nothing known, cannot \
         be told apart, so the survival is not read past it"
    );
    let named = Known {
        leaders: BTreeSet::from([4_100_000_u32, 4_200_000]),
        members: BTreeMap::from([
            (4_100_000, BTreeSet::from([4_000_000_u32])),
            (4_200_000, BTreeSet::from([4_000_001_u32])),
        ]),
    };
    assert!(
        !ours(&child(0), Some(4_200_000), &named),
        "a child another execution's container named as its own is that execution's, even \
         where the platform names no parent"
    );
    assert!(
        ours(
            &Orphan {
                pid: 4_000_001,
                parent: 4_100_000,
                at: None
            },
            Some(4_200_000),
            &named
        ),
        "and one this execution's container named is this execution's, whatever parent it \
         names"
    );
}

#[cfg(unix)]
#[test]
fn a_run_binds_its_step_state_once_rather_than_reopening_it_at_every_boundary() {
    let (one, two, selected) = step_modules();
    let temporary = tempfile::tempdir().expect("a place to build");
    let dir = temporary.path();
    let source = dir.join("bound_steps.rs");
    let state = dir.join("step.state");
    let held = dir.join("held.state");
    let replacement = dir.join("replacement.state");
    let nonce = "0123456789abcdef0123456789abcdef";
    let dormant =
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1000000\tdormant\t0\n");
    let elsewhere =
        format!("{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1000000\tactive\t500\n");
    std::fs::write(&state, &dormant).expect("initial state");
    std::fs::hard_link(&state, &held).expect("a second name for the file the run opens");
    std::fs::write(&replacement, &elsewhere).expect("a replacement");
    let program = format!(
        "{one}\n{two}\nfn main() {{\n\
         \x20   assert!(__rm_one::active(0));\n\
         \x20   __rm_two::checkpoint();\n\
         \x20   std::fs::rename({replacement:?}, {state:?}).expect(\"replace the name\");\n\
         \x20   for _ in 0..1_000 {{ __rm_two::checkpoint(); }}\n\
         }}\n"
    );
    std::fs::write(&source, program).expect("write program");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(built.status.success(), "{}", exact_output(&built.stderr));
    let run = Command::new(dir.join("bound_steps"))
        .env(WATCHED_ENV, WATCHED)
        .env(ACTIVE_ENV, &selected)
        .env(CATALOG_ENV, CATALOG)
        .env(STEPS_ENV, "1000000")
        .env(STEP_NONCE_ENV, nonce)
        .env(STEP_NOTICE_ENV, dir.join("step.notice"))
        .env(STEP_STATE_ENV, &state)
        .output()
        .expect("program runs");
    assert!(run.status.success(), "{}", exact_output(&run.stderr));
    let counted = std::fs::read_to_string(&held).expect("the file the run opened");
    let count = counted
        .trim_end()
        .strip_prefix(&format!(
            "{STEP_STATE_SCHEMA}\t{nonce}\t{CATALOG}\t{selected}\t1000000\tactive\t"
        ))
        .unwrap_or_else(|| panic!("the file the run opened is active: {counted}"));
    let charged = count
        .parse::<usize>()
        .expect("the charged count is a number");
    assert!(
        charged >= 1002,
        "every one of the 1002 boundaries is charged to the file the run opened, the reserved \
         ones included: {counted}"
    );
    assert_eq!(
        std::fs::read_to_string(&state).expect("the file now at the name"),
        elsewhere,
        "a run opens its step state once, per runtime copy, and never counts in whatever the \
         name is pointed at later: reopening the name at every boundary paid an open, a check \
         and a close per function entry and loop turn"
    );
}
