// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That reading Rust leaves no location behind on the caller's thread, where proc-macro2 would keep a copy of every text for as long as the thread lives.

#![expect(
    clippy::panic,
    reason = "a test reports a setup failure and a failed law by panicking"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

/// How many texts this thread's location map holds, read from the file a fresh token's location names.
fn texts_on_this_thread() -> usize {
    let probe: proc_macro2::TokenStream = match "probe".parse::<proc_macro2::TokenStream>() {
        Ok(tokens) => tokens,
        Err(error) => panic!("a probe token lexes: {error}"),
    };
    let Some(token) = probe.into_iter().next() else {
        panic!("the probe is one token")
    };
    let named = token.span().file();
    match named
        .strip_prefix("<parsed string ")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(str::parse::<usize>)
    {
        Some(Ok(index)) => index,
        Some(Err(_)) | None => panic!("a lexed token names the text it came from: {named}"),
    }
}

const SOURCE: &str = "fn f(a: bool, b: bool, c: bool) -> bool { a || b && c }\n";

#[test]
fn the_probe_sees_a_text_read_on_this_thread() {
    let before = texts_on_this_thread();
    let read = syn::parse_file(SOURCE);
    assert!(read.is_ok(), "the source parses");
    assert_eq!(
        texts_on_this_thread(),
        before + 2,
        "a text read here and the probe's own are both counted, so the law below can see growth"
    );
}

#[test]
fn discovering_files_leaves_the_callers_locations_as_they_were() {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let before = texts_on_this_thread();
    for _ in 0..10 {
        let found = discover_file("src/lib.rs", SOURCE.as_bytes(), &selection);
        assert!(found.is_ok(), "the source discovers");
    }
    assert_eq!(
        texts_on_this_thread(),
        before + 1,
        "ten discoveries read their text on a thread that ends with them, so the caller's map \
         holds only the probe's: a map that keeps every text is a leak for as long as the \
         thread lives and a wrap of every location past 4 GiB"
    );
}

#[test]
fn a_file_whose_read_backs_would_spend_the_threads_locations_is_refused_by_name() {
    let source = "fn f(a: bool, b: bool, c: bool) -> bool { a || b && c || a && b }\n";
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let whole = discover_file("src/lib.rs", source.as_bytes(), &selection);
    assert!(
        whole.is_ok(),
        "with the ceiling a thread has, the file discovers"
    );
    let two_readings = (source.len() * 3 + 1) * 2;
    let refused = rust_mutants::testkit::source::discover_within(two_readings, source);
    let Err(error) = refused else {
        panic!(
            "a thread that can read the file twice and no more cannot read back its swaps, and \
             a file whose swaps went missing would read as one with fewer: {refused:?}"
        )
    };
    assert_eq!(
        error.code().code,
        "RM0018",
        "the refusal names the ceiling it met, not a parse error or a smaller discovery: {error}"
    );
}

#[test]
fn a_chain_of_operators_deeper_than_a_default_stack_holds_is_read() {
    let terms = 6_000;
    let mut source = String::from("fn f(a: u64) -> u64 { a");
    for _ in 1..terms {
        source.push_str(" + a");
    }
    source.push_str(" }\n");
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let found = discover_file("src/lib.rs", source.as_bytes(), &selection);
    assert!(
        found.is_ok(),
        "a left-deep chain recurses once per term through the walk, the clone and the drop, and \
         the reading thread's stack is sized for it"
    );
}

#[test]
fn a_reading_runs_on_a_thread_of_its_own_and_a_reading_inside_it_on_another() {
    let caller = std::thread::current().id();
    let threads = rust_mutants::parsing::apart(|_| {
        let outer = std::thread::current().id();
        let inner = rust_mutants::parsing::apart(|_| std::thread::current().id());
        (outer, inner)
    });
    let Ok((outer, Ok(inner))) = threads else {
        panic!("both readings ran: {threads:?}")
    };
    assert_ne!(outer, caller, "a reading runs on a thread of its own");
    assert_ne!(
        inner, outer,
        "a reading holds its own budget, so one inside another is a thread and a budget of its own \
         rather than a share of the outer one's"
    );
}
