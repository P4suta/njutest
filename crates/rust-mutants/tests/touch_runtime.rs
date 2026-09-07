// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guards as a measurement: a guard records which of the process's threads reached it, and libtest names a thread after its test.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;
use std::process::Command;

use rust_mutants::instrument::{MODULE_STEM, TOUCH_ENV, render};
use rust_mutants::rule::Tier;
use rust_mutants::testkit::compile::ScriptedCompile;
use rust_mutants::touch::{self, TouchError};

const SOURCE: &str = "pub fn one(a: i32) -> i32 { a + 1 }\n\
                      pub fn two(a: i32) -> i32 { a - 1 }\n\
                      pub fn three(a: i32) -> i32 { a * 2 }\n";

const CATALOG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The runtime module of a file whose mutants are the ones `SOURCE` yields, named `__rm`.
fn module() -> (String, u32) {
    let scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All);
    let placements = scripted.placements();
    let count = u32::try_from(placements.len()).expect("a small catalog");
    assert!(count >= 3, "the source yields mutants to reach: {count}");
    (render(MODULE_STEM, CATALOG, placements, "\n"), count)
}

/// Builds a program around the runtime, runs it, and returns what it wrote to the touch log.
fn ran(name: &str, body: &str, touching: bool) -> String {
    let (module, _) = module();
    let dir = mjutest_devkit::paths::workspace_root()
        .join("target/touch-runtime")
        .join(format!("{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a place to build");
    let source = dir.join(format!("{name}.rs"));
    std::fs::write(&source, format!("{module}\nfn main() {{\n{body}\n}}\n")).expect("write");
    let built = Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "bin"])
        .arg("--out-dir")
        .arg(&dir)
        .arg(&source)
        .output()
        .expect("rustc runs");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let log = dir.join(format!("{name}.touch"));
    drop(std::fs::remove_file(&log));
    let mut command = Command::new(dir.join(name));
    command.env_remove("RUST_MUTANTS_ACTIVE");
    command.env_remove("RUST_MUTANTS_CATALOG");
    if touching {
        command.env(TOUCH_ENV, &log);
    } else {
        command.env_remove(TOUCH_ENV);
    }
    let output = command.output().expect("the program runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(&log).unwrap_or_default()
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
    let touches = touch::read(&text, CATALOG, count).expect("the log reads");
    assert_eq!(touches.tests.get("alpha"), Some(&set(&[0, 1])));
    assert_eq!(touches.tests.get("beta"), Some(&set(&[1])));
    assert!(
        touches.loose.is_empty(),
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
    let touches = touch::read(&text, CATALOG, count).expect("the log reads");
    assert!(
        touches.tests.is_empty(),
        "neither the main thread nor an unnamed one is a test: {touches:?}"
    );
    assert_eq!(
        touches.loose,
        set(&[0, 2]),
        "a site a run cannot attribute has to reach every test of its target"
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
fn a_log_about_another_catalog_says_nothing_rather_than_something_wrong() {
    let text = format!("{} {}\nt\talpha\t0\n", touch::SCHEMA, "b".repeat(64));
    assert!(matches!(
        touch::read(&text, CATALOG, 4),
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
        touch::read(&text, CATALOG, 4),
        Err(TouchError::BeyondCatalog { index: 9, .. })
    ));
}

#[test]
fn a_record_before_any_header_says_nothing_because_nothing_says_which_catalog_it_is_about() {
    assert!(matches!(
        touch::read("t\talpha\t0\n", CATALOG, 4),
        Err(TouchError::Headless { .. })
    ));
}

#[test]
fn the_same_thread_named_twice_is_one_test_that_reached_both_lines_worth_of_sites() {
    let text = format!(
        "{schema} {CATALOG}\nt\talpha\t0,1\nt\talpha\t2\n",
        schema = touch::SCHEMA
    );
    let touches = touch::read(&text, CATALOG, 4).expect("the log reads");
    assert_eq!(
        touches.tests.get("alpha"),
        Some(&set(&[0, 1, 2])),
        "a thread that reached more sites than one line holds is still one test"
    );
}

/// Compiles a library around the runtime, returning what rustc said.
fn built(name: &str, prefix: &str) -> std::process::Output {
    let (module, _) = module();
    let dir = mjutest_devkit::paths::workspace_root()
        .join("target/touch-runtime")
        .join(format!("{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a place to build");
    let source = dir.join(format!("{name}.rs"));
    std::fs::write(&source, format!("{prefix}{module}")).expect("write");
    Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "lib"])
        .arg("--out-dir")
        .arg(&dir)
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
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
