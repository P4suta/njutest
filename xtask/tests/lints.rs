// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that keeps `#[allow]` and `Box<dyn Trait>` out.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::lints::{Kind, scan_source};

fn kinds(source: &str) -> Vec<Kind> {
    scan_source("a.rs", source)
        .expect("the source parses")
        .into_iter()
        .map(|finding| finding.kind)
        .collect()
}

#[test]
fn ordinary_code_is_not_a_finding() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f(x: i32) -> i32 {\n    x + 1\n}\n"),
        []
    );
}

#[test]
fn an_item_allow_is_refused() {
    assert_eq!(
        kinds("//! A file.\n\n#[allow(dead_code)]\npub fn f() {}\n"),
        [Kind::AllowAttribute]
    );
}

#[test]
fn a_crate_level_allow_is_refused_too() {
    assert_eq!(
        kinds("//! A file.\n#![allow(clippy::pedantic)]\n"),
        [Kind::AllowAttribute]
    );
}

#[test]
fn an_expect_is_the_waiver_this_repository_writes() {
    assert_eq!(
        kinds("//! A file.\n\n#[expect(dead_code, reason = \"soon\")]\npub fn f() {}\n"),
        [],
        "an expectation the compiler retires when the lint stops firing"
    );
}

#[test]
fn a_boxed_trait_object_is_refused_wherever_it_appears() {
    for source in [
        "//! A file.\npub struct S {\n    f: Box<dyn std::fmt::Debug>,\n}\n",
        "//! A file.\npub fn f() -> Box<dyn Fn(i32) -> i32> {\n    unimplemented!()\n}\n",
        "//! A file.\npub fn f(v: Vec<Box<dyn std::error::Error>>) {\n    drop(v);\n}\n",
    ] {
        assert_eq!(kinds(source), [Kind::BoxedTraitObject], "{source}");
    }
}

#[test]
fn a_box_of_something_concrete_is_not_a_finding() {
    assert_eq!(
        kinds("//! A file.\npub struct S {\n    f: Box<str>,\n    g: Box<[u8]>,\n}\n"),
        [],
        "the cost this gate is about is the vtable, not the allocation"
    );
}

#[test]
fn a_finding_says_where_it_is_and_what_to_do_instead() {
    let findings = scan_source(
        "crates/a/src/lib.rs",
        "//! A file.\n\n#[allow(dead_code)]\npub fn f() {}\n",
    )
    .expect("the source parses");
    let rendered = findings.first().expect("one finding").to_string();
    assert!(rendered.starts_with("crates/a/src/lib.rs:3:"), "{rendered}");
    assert!(rendered.contains("#[expect("), "{rendered}");
}

#[test]
fn a_file_that_is_not_rust_is_an_error_rather_than_a_pass() {
    scan_source("a.rs", "fn (").expect_err("a file this gate cannot read is not a file it passes");
}
