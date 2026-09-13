// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that keeps `#[allow]`, `Box<dyn Trait>`, and a comment beside the code out.

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

#[test]
fn a_comment_beside_the_code_is_refused_and_documentation_is_not() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    // why\n}\n"),
        [Kind::Comment],
        "a comment beside code is a second account of it that nothing keeps true"
    );
    assert_eq!(
        kinds("//! A file.\n\n/// What it is.\npub fn f() {}\n"),
        [],
        "the one line the lint set asks for on a public item is documentation, not a comment"
    );
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    let x = 1; // here\n    drop(x);\n}\n"),
        [Kind::Comment],
        "and one at the end of a line is one too"
    );
    assert_eq!(
        kinds("//! A file.\n\n/* why */\npub fn f() {}\n"),
        [Kind::Comment],
        "whichever way it is spelled"
    );
    assert_eq!(
        kinds("//! A file.\n\n/** What it is. */\npub fn f() {}\n"),
        [],
        "and a block that documents is documentation"
    );
}

#[test]
fn the_licence_header_is_not_a_comment_this_gate_refuses() {
    assert_eq!(
        kinds(
            "// SPDX-FileCopyrightText: 2026 njutest contributors\n             // SPDX-License-Identifier: MIT OR Apache-2.0\n\n//! A file.\n"
        ),
        [],
        "every file of this repository carries it"
    );
}

#[test]
fn slashes_inside_a_literal_are_not_a_comment() {
    for source in [
        "//! A file.\npub const URL: &str = \"https://example.test/a\";\n",
        "//! A file.\npub const RAW: &str = r\"https://example.test/a\";\n",
        "//! A file.\npub const HASHED: &str = r#\"a \"//\" b\"#;\n",
        "//! A file.\npub const BYTES: &[u8] = b\"//\";\n",
        "//! A file.\npub const SLASH: char = '/';\n",
        "//! A file.\npub fn f(s: &'static str) -> &'static str {\n    s\n}\n",
        "//! A file.\npub const ESCAPED: &str = \"a\\\\\";\n",
    ] {
        assert_eq!(
            kinds(source),
            [],
            "the text a program carries is not a thing anybody said about it: {source}"
        );
    }
}

#[test]
fn the_engine_s_own_annotation_is_an_instruction_rather_than_an_account() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    // rust-mutants: skip nothing to see\n}\n"),
        [],
        "a skip marker is read by the engine, and its syntax is what says so"
    );
}

#[test]
fn the_development_page_names_every_kind_this_gate_reports() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace root")
            .join("docs/development.md"),
    )
    .expect("the page");
    for kind in Kind::ALL {
        assert!(
            page.contains(&format!("`{}`", kind.label())),
            "a finding says {} and docs/development.md does not say what it is",
            kind.label()
        );
    }
}
