// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The seam ratchet refuses each shape ADR 0001 names, exempts what it exempts, and holds the tree to the ledger in both directions.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use xtask::devgates::{Seam, SeamKind, compare, parse_ledger, scan_source};

fn seams(path: &str, source: &str) -> Vec<String> {
    scan_source(path, source)
        .expect("parses")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_static_mut_is_a_seam() {
    assert_eq!(
        seams("crates/x/src/a.rs", "static mut COUNTER: u32 = 0;"),
        ["crates/x/src/a.rs:static-mut:COUNTER"]
    );
}

#[test]
fn a_static_with_interior_mutability_is_a_seam() {
    let source = r#"
        use std::sync::{Mutex, OnceLock, LazyLock, RwLock};
        use std::cell::RefCell;
        use std::sync::atomic::AtomicBool;
        static A: Mutex<u32> = Mutex::new(0);
        static B: OnceLock<String> = OnceLock::new();
        static C: LazyLock<Vec<u8>> = LazyLock::new(Vec::new);
        static D: RwLock<u8> = RwLock::new(0);
        static E: AtomicBool = AtomicBool::new(false);
        static F: std::cell::Cell<u8> = std::cell::Cell::new(0);
        static PLAIN: &str = "constant";
        const ALSO_FINE: Mutex<u32> = Mutex::new(0);
    "#;
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:static-interior-mutability:A",
            "crates/x/src/a.rs:static-interior-mutability:B",
            "crates/x/src/a.rs:static-interior-mutability:C",
            "crates/x/src/a.rs:static-interior-mutability:D",
            "crates/x/src/a.rs:static-interior-mutability:E",
            "crates/x/src/a.rs:static-interior-mutability:F",
        ]
    );
}

#[test]
fn a_thread_local_is_a_seam() {
    let source =
        "thread_local! { static SLOT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }";
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        ["crates/x/src/a.rs:thread-local:SLOT"]
    );
}

#[test]
fn cfg_test_is_allowed_only_on_a_tests_module() {
    let source = r#"
        #[cfg(test)]
        mod tests { fn helper() {} }
        #[cfg(test)]
        fn test_only_helper() {}
        fn production() { if cfg!(test) { } }
        #[cfg(any(test, feature = "testkit"))]
        pub mod testkit;
    "#;
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:cfg-test-outside-tests-module:cfg!(test)",
            "crates/x/src/a.rs:cfg-test-outside-tests-module:test_only_helper",
        ]
    );
}

#[test]
fn reading_the_process_environment_is_a_seam_outside_the_composition_root() {
    let source = r#"
        use std::env;
        fn a() -> Option<String> { std::env::var("HOME").ok() }
        fn b() -> Option<std::ffi::OsString> { env::var_os("TMPDIR") }
        fn c() { let _ = std::env::current_dir(); let _ = env::args(); let _ = option_env!("X"); }
        const V: &str = env!("CARGO_PKG_VERSION"); // compile-time, not the process
    "#;
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:process-environment-read:env::args",
            "crates/x/src/a.rs:process-environment-read:env::var_os",
            "crates/x/src/a.rs:process-environment-read:option_env!",
            "crates/x/src/a.rs:process-environment-read:std::env::current_dir",
            "crates/x/src/a.rs:process-environment-read:std::env::var",
        ]
    );
    assert_eq!(seams("crates/x/src/main.rs", source), Vec::<String>::new());
}

#[test]
fn exiting_the_process_is_a_seam_outside_the_composition_root() {
    let source = "fn a() { std::process::exit(3) } fn b() { process::exit(1) }";
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:process-exit:process::exit",
            "crates/x/src/a.rs:process-exit:std::process::exit"
        ]
    );
    assert_eq!(seams("crates/x/src/main.rs", source), Vec::<String>::new());
}

#[test]
fn importing_test_support_from_production_code_is_a_seam() {
    let source = r#"
        use crate::testkit::Repo;
        use njutest_devkit::golden;
        #[cfg(test)]
        mod tests { use crate::testkit::ScriptedWorkspace; }
        #[cfg(any(test, feature = "testkit"))]
        pub mod testkit;
    "#;
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:testkit-import:crate::testkit::Repo",
            "crates/x/src/a.rs:testkit-import:njutest_devkit::golden",
        ]
    );
}

#[test]
fn items_inside_a_tests_module_are_not_scanned() {
    let source = r#"
        #[cfg(test)]
        mod tests {
            static mut X: u32 = 0;
            fn f() { std::env::var("A"); std::process::exit(1) }
        }
    "#;
    assert_eq!(seams("crates/x/src/a.rs", source), Vec::<String>::new());
}

#[test]
fn findings_are_sorted_by_path_kind_and_name() {
    let source = "static mut Z: u32 = 0; fn f() { std::process::exit(1) } static mut A: u32 = 0;";
    assert_eq!(
        seams("crates/x/src/a.rs", source),
        [
            "crates/x/src/a.rs:static-mut:A",
            "crates/x/src/a.rs:static-mut:Z",
            "crates/x/src/a.rs:process-exit:std::process::exit",
        ]
    );
}

fn seam(path: &str, kind: SeamKind, name: &str) -> Seam {
    Seam {
        path: path.to_owned(),
        kind,
        name: name.to_owned(),
    }
}

#[test]
fn the_ledger_parses_sorted_lines_and_ignores_comments() {
    let text = "# comment\n\ncrates/x/src/a.rs:static-mut:A\ncrates/x/src/b.rs:process-exit:std::process::exit\n";
    assert_eq!(
        parse_ledger(text).expect("parses"),
        [
            seam("crates/x/src/a.rs", SeamKind::StaticMut, "A"),
            seam(
                "crates/x/src/b.rs",
                SeamKind::ProcessExit,
                "std::process::exit"
            ),
        ]
    );
}

#[test]
fn a_malformed_ledger_line_is_refused_with_its_line_number() {
    let error = parse_ledger("# ok\ncrates/x/src/a.rs:no-such-kind:A\n").expect_err("refused");
    assert_eq!(error.line, 2);
    assert!(error.reason.contains("no-such-kind"), "{error}");
}

#[test]
fn an_unsorted_ledger_is_refused() {
    let text = "crates/x/src/b.rs:static-mut:B\ncrates/x/src/a.rs:static-mut:A\n";
    let error = parse_ledger(text).expect_err("refused");
    assert_eq!(error.line, 2);
    assert!(error.reason.contains("sorted"), "{error}");
}

#[test]
fn the_gate_is_green_only_when_scan_and_ledger_agree_exactly() {
    let recorded = seam("crates/x/src/a.rs", SeamKind::StaticMut, "A");
    let new = seam("crates/x/src/b.rs", SeamKind::ThreadLocal, "SLOT");
    compare(
        std::slice::from_ref(&recorded),
        std::slice::from_ref(&recorded),
    )
    .expect("agreement");

    let both = [recorded.clone(), new.clone()];
    let disagreement = compare(&both, std::slice::from_ref(&recorded)).expect_err("new seam");
    assert_eq!(disagreement.unrecorded, std::slice::from_ref(&new));
    assert!(disagreement.stale.is_empty());
    let message = disagreement.to_string();
    assert!(
        message.contains("crates/x/src/b.rs:thread-local:SLOT"),
        "{message}"
    );
    assert!(
        message.contains("reviewed exception, not the fix"),
        "the message teaches the rule: {message}"
    );

    let disagreement = compare(&[], std::slice::from_ref(&recorded)).expect_err("stale line");
    assert_eq!(disagreement.stale, std::slice::from_ref(&recorded));
    assert!(
        disagreement.to_string().contains("no longer"),
        "{disagreement}"
    );
}
