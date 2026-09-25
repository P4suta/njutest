// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an edit can change without an execution noticing: which bodies are sealed, each body's digest, and each unit's skeleton (ADR 0041).

#![expect(
    clippy::expect_used,
    reason = "a test that cannot catalog its own fixture has nothing to say about the evidence"
)]

use std::collections::BTreeMap;

use rust_mutants::instrument::{ItemSource, catalog_items};
use rust_mutants::skeleton::{ItemEvidence, Skeletons, UnitSource, Unsealing, evidence};

/// One library unit reading exactly these Rust files.
fn unit(files: &[(&str, &str)]) -> UnitSource {
    UnitSource {
        package: "demo".to_owned(),
        target: "demo".to_owned(),
        kind: "lib".to_owned(),
        test: false,
        files: files
            .iter()
            .map(|(path, text)| (format!("$root/{path}"), text.as_bytes().to_vec()))
            .collect(),
        env: BTreeMap::new(),
        emitted: BTreeMap::new(),
    }
}

/// The evidence for one unit of these files, every item of them cataloged.
fn evidence_of(unit: &UnitSource, files: &[(&str, &str)]) -> Skeletons {
    let sources: Vec<ItemSource<'_>> = files
        .iter()
        .map(|(path, text)| ItemSource {
            path,
            package: "demo",
            source: text.as_bytes(),
        })
        .collect();
    let catalog = catalog_items(&sources).expect("the fixture's items are cataloged");
    evidence(std::slice::from_ref(unit), &catalog.items)
}

/// The evidence of the one item called `name`, wherever it is declared.
fn item<'a>(skeletons: &'a Skeletons, name: &str) -> &'a ItemEvidence {
    let nested = format!("::{name}");
    skeletons
        .items
        .iter()
        .find(|item| item.name == name || item.name.ends_with(&nested))
        .expect("the item is cataloged")
}

/// Why the item `f` of this one file is not sealed, or nothing when it is.
fn verdict(source: &str) -> Option<Unsealing> {
    let files = [("src/lib.rs", source)];
    let skeletons = evidence_of(&unit(&files), &files);
    item(&skeletons, "f").unsealed.clone()
}

#[test]
fn a_body_is_sealed_only_when_all_it_contributes_is_its_own_execution() {
    for sealed in [
        "fn f() -> i32 { let x = 1; x + 1 }",
        "fn f() { println!(\"{}\", 1); }",
        "fn f() { std::println!(\"x\"); assert_eq!(1, 1); let _ = vec![1, 2]; }",
        "fn f() -> bool { matches!(Some(1), Some(_)) && cfg!(test) }",
        "fn f() -> i32 { unsafe { 1 } }",
        "fn f() -> impl Fn() -> i32 { || 1 }",
        "#[inline] #[must_use] #[doc = \"x\"] fn f() -> i32 { 1 }",
        "fn f() { #[allow(unused_variables)] let x = 1; }",
        "struct S; impl S { #[track_caller] fn f(&self) {} }",
        "#[cfg(test)] mod tests { use super::*; fn f() {} }",
    ] {
        assert_eq!(verdict(sealed), None, "{sealed}");
    }
    for (unsealed, why) in [
        (
            "fn f() { thing!(); }",
            Unsealing::Macro {
                name: "thing".to_owned(),
            },
        ),
        (
            "fn f() { other::println!(\"x\"); }",
            Unsealing::Macro {
                name: "other::println".to_owned(),
            },
        ),
        (
            "fn f() { println!(\"{}\", env!(\"X\")); }",
            Unsealing::Macro {
                name: "env".to_owned(),
            },
        ),
        (
            "fn f() -> &'static str { include_str!(\"a.txt\") }",
            Unsealing::Macro {
                name: "include_str".to_owned(),
            },
        ),
        (
            "#[tokio::main] fn f() {}",
            Unsealing::Attribute {
                name: "tokio::main".to_owned(),
            },
        ),
        (
            "fn f() { #[custom] let x = 1; }",
            Unsealing::Attribute {
                name: "custom".to_owned(),
            },
        ),
        (
            "struct S; #[async_trait] impl S { fn f(&self) {} }",
            Unsealing::Attribute {
                name: "async_trait".to_owned(),
            },
        ),
        (
            "fn f() { struct Inner; }",
            Unsealing::DeclaresItem {
                kind: "struct".to_owned(),
            },
        ),
        (
            "struct S; fn f() { impl S {} }",
            Unsealing::DeclaresItem {
                kind: "impl".to_owned(),
            },
        ),
        ("fn f() -> i32 { const { 1 } }", Unsealing::ConstBlock),
        ("const fn f() -> i32 { 1 }", Unsealing::Evaluated),
    ] {
        assert_eq!(verdict(unsealed), Some(why), "{unsealed}");
    }
}

#[test]
fn a_unit_that_can_rename_a_listed_macro_seals_none_of_its_bodies() {
    for (elsewhere, why) in [
        (
            "macro_rules! format { () => {} }",
            Unsealing::Shadowed {
                name: "format".to_owned(),
            },
        ),
        (
            "use other::vec;",
            Unsealing::Shadowed {
                name: "vec".to_owned(),
            },
        ),
        (
            "use other::thing as println;",
            Unsealing::Shadowed {
                name: "println".to_owned(),
            },
        ),
        (
            "use other::*;",
            Unsealing::ForeignGlob {
                path: "other".to_owned(),
            },
        ),
        (
            "#[macro_use] extern crate other;",
            Unsealing::ForeignGlob {
                path: "other".to_owned(),
            },
        ),
    ] {
        let files = [
            ("src/lib.rs", "mod elsewhere;\nfn f() -> i32 { 1 }\n"),
            ("src/elsewhere.rs", elsewhere),
        ];
        let skeletons = evidence_of(&unit(&files), &files);
        assert_eq!(
            item(&skeletons, "f").unsealed,
            Some(why),
            "{elsewhere} anywhere in the unit can change what a listed macro in `f` expands to"
        );
    }
    let files = [
        ("src/lib.rs", "mod elsewhere;\nfn f() -> i32 { 1 }\n"),
        (
            "src/elsewhere.rs",
            "use super::*;\nuse crate::f as g;\nuse std::collections::*;\n",
        ),
    ];
    let skeletons = evidence_of(&unit(&files), &files);
    assert_eq!(
        item(&skeletons, "f").unsealed,
        None,
        "a glob from this crate or from the standard library names no macro the unit did not declare"
    );
}

#[test]
fn an_edit_inside_a_sealed_body_changes_its_digest_and_no_skeleton() {
    let before = [(
        "src/lib.rs",
        "pub fn f() -> i32 { 1 }\npub fn g() -> i32 { 2 }\n",
    )];
    let after = [(
        "src/lib.rs",
        "pub fn f() -> i32 { 1 + 2 + 3 }\npub fn g() -> i32 { 2 }\n",
    )];
    let (then, now) = (
        evidence_of(&unit(&before), &before),
        evidence_of(&unit(&after), &after),
    );
    assert!(
        item(&then, "f").sealed && item(&now, "f").sealed,
        "both bodies only compute: {then:?}"
    );
    assert_ne!(
        item(&then, "f").body_digest,
        item(&now, "f").body_digest,
        "the edited body is a different body"
    );
    assert_eq!(
        item(&then, "g").body_digest,
        item(&now, "g").body_digest,
        "and the one beside it is the same one"
    );
    assert_eq!(
        then.units, now.units,
        "while nothing outside a sealed body moved, so no unit's skeleton did"
    );
}

#[test]
fn an_edit_outside_a_sealed_body_changes_the_skeleton() {
    let base = "const K: i32 = 1;\npub fn f() -> i32 { K }\npub fn g() { thing!(1) }\n";
    let first = [("src/lib.rs", base)];
    let then = evidence_of(&unit(&first), &first);
    for (what, edited) in [
        (
            "a constant",
            "const K: i32 = 2;\npub fn f() -> i32 { K }\npub fn g() { thing!(1) }\n",
        ),
        (
            "a signature",
            "const K: i32 = 1;\npub fn f() -> i32 where i32: Copy { K }\npub fn g() { thing!(1) }\n",
        ),
        (
            "a body that invokes a macro off the list",
            "const K: i32 = 1;\npub fn f() -> i32 { K }\npub fn g() { thing!(2) }\n",
        ),
    ] {
        let files = [("src/lib.rs", edited)];
        let now = evidence_of(&unit(&files), &files);
        assert_ne!(
            then.units, now.units,
            "{what} is outside every sealed body, so the skeleton covers it"
        );
    }
    let mut read = unit(&first);
    read.files
        .insert("$root/src/answer.txt".to_owned(), b"30\n".to_vec());
    let mut reread = read.clone();
    reread
        .files
        .insert("$root/src/answer.txt".to_owned(), b"31\n".to_vec());
    let mut variable = read.clone();
    variable.env.insert("LIMIT".to_owned(), "set:9".to_owned());
    let mut emitted = read.clone();
    emitted
        .emitted
        .insert("$emitted/$target/out".to_owned(), "a".repeat(64));
    let skeleton = |unit: &UnitSource| evidence_of(unit, &first).units;
    let read_skeleton = skeleton(&read);
    for (what, other) in [
        ("a file that is not Rust", &reread),
        ("a variable the compilation read", &variable),
        ("what a build script emitted", &emitted),
    ] {
        assert_ne!(
            read_skeleton,
            skeleton(other),
            "{what} is part of what the unit compiled"
        );
    }
}

/// The lines of the fenced block the page opens with `info`.
fn listed(page: &str, info: &str) -> Vec<String> {
    let opening = format!("```{info}\n");
    page.split_once(&opening)
        .and_then(|(_before, rest)| rest.split_once("```"))
        .map(|(block, _after)| block.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn the_page_the_audit_implements_names_exactly_the_lists_the_evidence_applies() {
    let at = njutest_devkit::paths::workspace_root().join("docs/engine/carry.md");
    let page = std::fs::read_to_string(&at).expect("the carry page");
    for (info, applied) in [
        (
            "sealable-macros",
            &rust_mutants::skeleton::SEALABLE_MACROS[..],
        ),
        (
            "standard-roots",
            &rust_mutants::skeleton::STANDARD_ROOTS[..],
        ),
        ("local-roots", &rust_mutants::skeleton::LOCAL_ROOTS[..]),
        (
            "sealable-attributes",
            &rust_mutants::skeleton::SEALABLE_ATTRIBUTES[..],
        ),
        (
            "tool-namespaces",
            &rust_mutants::skeleton::TOOL_NAMESPACES[..],
        ),
    ] {
        assert_eq!(
            listed(&page, info),
            applied,
            "docs/engine/carry.md's `{info}` is what the audit reimplements, so it is this \
             list and nothing else"
        );
    }
}
