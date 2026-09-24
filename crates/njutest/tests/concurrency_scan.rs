// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The static half of the single-threaded proof: every way a file can start a thread is found, and anything unsure is.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest::concurrency::scan::{ScanError, Starts, scanned};

fn found(source: &str) -> Vec<(Starts, String)> {
    scanned("src/lib.rs", source)
        .expect("Rust")
        .into_iter()
        .map(|one| (one.what, one.by))
        .collect()
}

fn kinds(source: &str) -> Vec<Starts> {
    found(source).into_iter().map(|(what, _)| what).collect()
}

#[test]
fn code_that_starts_nothing_is_found_to_start_nothing() {
    assert_eq!(
        found(
            "use std::sync::OnceLock;\n\
             static ONE: OnceLock<u32> = OnceLock::new();\n\
             pub fn add(a: u32, b: u32) -> u32 { a + *ONE.get_or_init(|| b) }\n\
             #[test] fn adds() { assert_eq!(add(1, 2), 3); }\n"
        ),
        [],
        "a lazily initialised global runs on the thread that touches it, which starts nothing"
    );
}

#[test]
fn a_spawn_is_found_on_any_receiver_and_under_any_import() {
    for source in [
        "fn a() { std::thread::spawn(|| {}); }",
        "use std::thread; fn a() { thread::spawn(|| {}); }",
        "use std::thread::spawn as go; fn a() { go(|| {}); }",
        "fn a() { std::thread::Builder::new().name(\"w\".into()).spawn(|| {}); }",
        "fn a() { std::thread::scope(|s| { s.spawn(|| {}); }); }",
        "fn a() { tokio::task::spawn_blocking(|| {}); }",
        "fn a() { std::process::Command::new(\"sh\").spawn(); }",
    ] {
        assert!(
            !kinds(source).is_empty(),
            "a file that can start a thread or a process is never read as one that cannot: {source}"
        );
    }
    assert!(
        kinds("fn a() { std::thread::spawn(|| {}); }").contains(&Starts::Spawn),
        "a spawn is a spawn"
    );
}

#[test]
fn a_rename_that_hides_the_word_spawn_is_still_found_by_its_import() {
    assert!(
        !kinds("use std::thread::spawn as go; fn a() { go(|| {}); }").is_empty(),
        "an import of a spawning function is a spawn, whatever it is called at the call"
    );
}

#[test]
fn a_scope_parallel_iterator_pool_or_runtime_is_found() {
    assert!(kinds("fn a() { crossbeam::scope(|s| {}); }").contains(&Starts::Scope));
    assert!(
        kinds("use rayon::prelude::*; fn a(v: &[u8]) { v.par_iter(); }")
            .contains(&Starts::Parallel)
    );
    assert!(kinds("fn a(v: Vec<u8>) { v.into_par_iter(); }").contains(&Starts::Parallel));
    assert!(
        kinds("fn a() { let _pool = threadpool::ThreadPool::new(4); }").contains(&Starts::Parallel)
    );
    assert!(kinds("#[tokio::main] async fn main() {}").contains(&Starts::Runtime));
    assert!(kinds("#[tokio::test] async fn t() {}").contains(&Starts::Runtime));
    assert!(kinds("#[async_std::test] async fn t() {}").contains(&Starts::Runtime));
    assert!(
        kinds("fn a() { tokio::runtime::Builder::new_multi_thread().build(); }")
            .contains(&Starts::Runtime)
    );
}

#[test]
fn what_a_macro_is_handed_is_read_too() {
    assert!(
        kinds("fn a() { my_macro!(std::thread::spawn(|| {})); }").contains(&Starts::Spawn),
        "a macro body is code the compiler will see, and so must this"
    );
    assert!(
        kinds("quote::quote! { ::tokio::spawn(async {}) }").contains(&Starts::Spawn),
        "a proc macro that writes a spawn into the code it expands to spawns in the binary that uses it"
    );
}

#[test]
fn code_the_compiler_cannot_see_is_native_and_proves_nothing() {
    assert!(kinds("extern \"C\" { fn work(); }").contains(&Starts::Native));
    assert!(
        kinds("fn a() { unsafe { libc::pthread_create(p, a, f, x); } }").contains(&Starts::Native)
    );
}

#[test]
fn a_file_that_is_not_rust_is_refused_rather_than_read_as_starting_nothing() {
    assert!(
        scanned("src/broken.rs", "fn a( {").is_err(),
        "a file that was not read is not a file with nothing in it"
    );
}

#[test]
fn a_raw_identifier_is_read_as_the_name_it_spells() {
    assert_eq!(
        kinds(
            "fn a() { std::thread::r#spawn(|| {}); std::thread::r#scope(|s| {}); r#rayon::join(|| 1, || 2); }"
        ),
        [Starts::Spawn, Starts::Scope, Starts::Parallel],
        "`r#spawn` is `spawn` to the compiler, so it is `spawn` to the scan"
    );
}

#[test]
fn a_file_nested_deeper_than_the_scan_reads_is_refused_on_any_stack() {
    let source = format!(
        "fn a() {{ let _ = {}1{}; }}",
        "(".repeat(5000),
        ")".repeat(5000)
    );
    let refused = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn_scoped(scope, || {
                matches!(
                    scanned("src/deep.rs", &source),
                    Err(ScanError::TooDeep { .. })
                )
            })
            .expect("a thread")
            .join()
            .expect("the scan returns rather than overflowing the stack")
    });
    assert!(
        refused,
        "a nesting the parser would recurse through is refused before it is parsed"
    );
}

#[test]
fn brackets_in_comments_and_literals_are_not_nesting() {
    let many = "(".repeat(300);
    let source = format!(
        "// {many}\n/* {many} /* {many} */ */\nfn a<'a>(x: &'a str) -> char {{\n    let _ = \"{many}\\\"\";\n    let _ = r#\"{many}\"#;\n    let _ = b\"{many}\";\n    let _ = br##\"{many}\"##;\n    let _ = b'(';\n    '('\n}}\n"
    );
    assert_eq!(
        scanned("src/lib.rs", &source).expect("read").len(),
        0,
        "a file whose brackets nest shallowly is read, whatever its comments and literals hold"
    );
}

#[test]
fn a_chain_the_parser_would_recurse_through_without_a_bracket_is_read_on_a_small_stack() {
    let source = format!(
        "fn a() -> bool {{ {}true }}\nfn b() {{ std::thread::spawn(|| {{}}); }}\ntype T = {}u8{};\n",
        "!".repeat(20_000),
        "Vec<".repeat(5_000),
        ">".repeat(5_000)
    );
    let found = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn_scoped(scope, || {
                scanned("src/chain.rs", &source).map(|found| found.len())
            })
            .expect("a thread")
            .join()
            .expect("the scan returns rather than overflowing the stack")
    });
    assert_eq!(
        found.expect("tokens are read without a parse that recurses"),
        1,
        "the spawn is found however long the chains beside it"
    );
}
