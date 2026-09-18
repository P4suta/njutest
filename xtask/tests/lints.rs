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

/// A recursive removal in a loop is refused, and one of a single directory is not.
#[test]
fn a_removal_in_a_loop_is_refused_and_one_of_a_named_directory_is_not() {
    let looping = r"
        fn sweep(directories: &[std::path::PathBuf]) {
            for one in directories {
                drop(std::fs::remove_dir_all(one));
            }
        }
    ";
    let single = r"
        fn close(dir: &std::path::Path) {
            drop(std::fs::remove_dir_all(dir));
        }
    ";
    let found = scan_source("crates/demo/src/lib.rs", looping).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::UnboundedRemoval),
        "a loop of removals is what runs for a day on a wedged mount while saying nothing: \
         {found:?}"
    );
    let found = scan_source("crates/demo/src/lib.rs", single).expect("it parses");
    assert!(
        found.is_empty(),
        "one directory a caller names is one removal, and a budget over one thing is a \
         bound on nothing: {found:?}"
    );
}

/// A command handed to a reader with an identity in it is refused.
#[test]
fn a_command_built_from_an_identity_is_refused() {
    let perishable = r#"
        fn said(mutant: &Mutant) -> String {
            format!("rust-mutants explain {}", mutant.display_id)
        }
    "#;
    let holding = r#"
        fn said(mutant: &Mutant) -> String {
            format!("rust-mutants explain {}:{}:{}", mutant.path, mutant.item, mutant.rule)
        }
    "#;
    let found = scan_source("crates/demo/src/lib.rs", perishable).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::PerishableHandle),
        "the edit that closes a survivor re-mints the identity naming it, so a command \
         printed with one stops working the moment it is followed: {found:?}"
    );
    let found = scan_source("crates/demo/src/lib.rs", holding).expect("it parses");
    assert!(
        found.is_empty(),
        "a locator holds through that edit, which is the whole reason it exists: {found:?}"
    );
}

/// A test that joins a layout constant is refused, and one that asks for the path is not.
#[test]
fn a_test_that_decides_the_layout_is_refused() {
    let source = "pub const LAYOUT: &str = \"reports/runs\";\n";
    let exported = xtask::lints::exported_strings(source);
    assert_eq!(
        exported,
        vec![(String::from("LAYOUT"), 1)],
        "a constant with a separator in it spells a structure rather than a name"
    );
    assert!(
        xtask::lints::exported_strings("pub const NAME: &str = \"runs\";\n").is_empty(),
        "and one without a separator is a name, which is a thing worth exporting"
    );

    let test = "use njutest_cli::app::reports::LAYOUT;\nlet path = root.join(LAYOUT);\n";
    assert!(xtask::lints::joins(test, "LAYOUT"));
    assert_eq!(
        xtask::lints::imported_from(test, "LAYOUT").as_deref(),
        Some("reports"),
        "the import is what tells four constants of the same name apart"
    );

    let asking = "use njutest_cli::app::reports::Store;\nlet path = Store::read(root).runs();\n";
    assert!(
        !xtask::lints::joins(asking, "LAYOUT"),
        "a test that asks the type that owns the layout decides nothing"
    );
    assert_eq!(
        xtask::lints::imported_from("let it = a::b::config::LAYOUT;\n", "LAYOUT").as_deref(),
        Some("config"),
        "a name written out in full at the point it is used is reached from a module too"
    );
}

#[test]
fn a_directory_the_configuration_can_move_is_spelled_in_one_place() {
    let source = "/// where reports/runs went\npub(crate) const DEFAULT_REPORTS_DIRECTORY: &str \
                  = \"artifacts\";\nconst OTHER_DIR: &str = \"vendor\";\n";
    assert_eq!(
        xtask::lints::configured_directories(source),
        vec![String::from("artifacts")],
        "a default the configuration falls back to is a directory somebody can rename, \
         and one that is not a default is not"
    );

    let directories = vec![String::from("artifacts")];
    assert_eq!(
        xtask::lints::spelled(
            "let path = root.join(\"artifacts/runs/one\");\n",
            &directories
        ),
        vec![1],
        "a path literal under a directory the configuration can move decides it"
    );
    assert_eq!(
        xtask::lints::spelled("let path = root.join(\"artifacts\");\n", &directories),
        vec![1],
        "and so does the directory itself, joined onto a root"
    );
    assert!(
        xtask::lints::spelled("let held = it.expect(\"artifacts\");\n", &directories).is_empty(),
        "while the same word said to a reader is not a path at all"
    );
    assert!(
        xtask::lints::spelled("/// under artifacts/runs\n", &directories).is_empty(),
        "and documentation may say where things are, which is what it is for"
    );
    assert!(
        xtask::lints::spelled("let path = Store::read(root).run(\"one\");\n", &directories)
            .is_empty(),
        "asking the type that owns the layout decides nothing"
    );
}

#[test]
fn a_colour_spelled_by_hand_is_refused_wherever_it_is_not_the_one_place_that_paints() {
    let by_hand = r#"
        pub const fn paint(killed: bool) -> &'static str {
            if killed { "\u{1b}[32m" } else { "\u{1b}[31m" }
        }
    "#;
    let found = scan_source("crates/demo/src/ui.rs", by_hand).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::HandPainted),
        "a second module that spells an escape decides for itself what green means, what \
         a reader's terminal can take, and whether to paint at all — and then the two \
         halves of one workspace look like two tools. The set of things that carry a \
         colour is closed; the set of places that turn one into bytes has to be one: \
         {found:?}"
    );
    let named = r"
        fn drawn(telling: Telling, word: &str) -> String {
            telling.painted(Style::Gap, word)
        }
    ";
    assert!(
        scan_source("crates/demo/src/ui.rs", named)
            .expect("it parses")
            .is_empty(),
        "asking for what a thing is rather than for a colour is the whole point, and it \
         is not what this refuses"
    );
}

#[test]
fn a_type_that_publishes_its_whole_list_may_not_also_say_the_list_is_open() {
    let both = r"
        /// Every way a thing can go.
        #[non_exhaustive]
        pub enum Outcome {
            /// One.
            Killed,
            /// Another.
            Survived,
        }

        impl Outcome {
            /// Every outcome, in declaration order.
            pub const ALL: [Self; 2] = [Self::Killed, Self::Survived];
        }
    ";
    let found = scan_source("crates/demo/src/outcome.rs", both).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::OpenAndClosed),
        "`ALL` promises this is every one of them and `#[non_exhaustive]` promises it is \
         not, so a caller outside the crate is made to write an arm for a case the list \
         says cannot exist — and the arm it writes counts the next variant as whatever \
         was nearest. A tally reached one of those and reported every future outcome as \
         a harness failure: {found:?}"
    );

    let closed = r"
        pub enum Outcome { Killed, Survived }
        impl Outcome {
            pub const ALL: [Self; 2] = [Self::Killed, Self::Survived];
        }
    ";
    assert!(
        scan_source("crates/demo/src/outcome.rs", closed)
            .expect("it parses")
            .is_empty(),
        "a closed set that says so is the whole point"
    );

    let open = r"
        #[non_exhaustive]
        pub enum RunError { Refused, Stopped }
    ";
    assert!(
        scan_source("crates/demo/src/error.rs", open)
            .expect("it parses")
            .is_empty(),
        "and an error a caller branches on must already handle one it does not know, so \
         the attribute costs nothing where no list was promised"
    );
}
