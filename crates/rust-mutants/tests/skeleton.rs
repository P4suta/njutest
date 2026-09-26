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
use rust_mutants::touch::{Item, ItemRef};

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
    let refs: Vec<ItemRef> = catalog
        .items
        .iter()
        .map(|item| {
            catalog
                .item_ref(item.index)
                .expect("every item has a reference")
        })
        .collect();
    let pairs: Vec<(&Item, &ItemRef)> = catalog.items.iter().zip(&refs).collect();
    evidence(std::slice::from_ref(unit), &pairs)
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
fn a_body_whose_only_contribution_is_its_own_execution_is_sealed() {
    for sealed in [
        "fn f() -> i32 { let x = 1; x + 1 }",
        "fn f() { println!(\"{}\", 1); }",
        "fn f() { std::println!(\"x\"); assert_eq!(1, 1); let _ = vec![1, 2]; }",
        "fn f() -> bool { matches!(Some(1), Some(_)) && cfg!(test) }",
        "fn f() -> i32 { unsafe { 1 } }",
        "#[test] fn f() { assert!(true); }",
        "#[test] #[should_panic(expected = \"x\")] #[ignore] fn f() { panic!(\"x\"); }",
        "#[inline] #[must_use] #[doc = \"x\"] fn f() -> i32 { 1 }",
        "fn f() { #[allow(unused_variables)] let x = 1; }",
        "struct S; impl S { #[track_caller] fn f(&self) {} }",
        "#[cfg(test)] mod tests { use super::*; fn f() {} }",
    ] {
        assert_eq!(verdict(sealed), None, "{sealed}");
    }
}

#[test]
fn a_body_whose_unit_file_cannot_be_read_is_never_sealed() {
    let cataloged = [("src/lib.rs", "fn f() { 1 }")];
    let missing = evidence_of(&unit(&[]), &cataloged);
    assert_eq!(
        item(&missing, "f").unsealed,
        Some(Unsealing::Unread),
        "a unit with no readable bytes for the cataloged file says that the body was unread"
    );
    let unparsable = [("src/lib.rs", "fn f() { @ }")];
    let malformed = evidence_of(&unit(&unparsable), &cataloged);
    assert_eq!(
        item(&malformed, "f").unsealed,
        Some(Unsealing::Unlocated),
        "bytes that cannot be read as the cataloged Rust file locate no body, so the body cannot be sealed"
    );
}

#[test]
fn a_body_that_contributes_anything_else_is_not_sealed_and_says_why() {
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
        ("fn f() -> impl Fn() -> i32 { || 1 }", Unsealing::OpaqueType),
        ("async fn f() -> i32 { 1 }", Unsealing::OpaqueType),
        (
            "trait T { fn f(&self) -> impl Sized { 1 } }",
            Unsealing::OpaqueType,
        ),
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
        "pub fn f() -> i32 { 7 }\npub fn g() -> i32 { 2 }\n",
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

#[test]
fn a_skeleton_is_the_fold_of_the_entries_it_names() {
    let files = [("src/lib.rs", "pub fn f() -> i32 { 1 }\nconst K: i32 = 2;\n")];
    let mut read = unit(&files);
    read.files
        .insert("$root/src/answer.txt".to_owned(), b"30\n".to_vec());
    read.env.insert("LIMIT".to_owned(), "unset".to_owned());
    read.emitted
        .insert("$emitted/$target/out".to_owned(), "b".repeat(64));
    let kept = evidence_of(&read, &files);
    let one = kept.units.first().expect("the unit");
    assert_eq!(
        one.entries.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "$emitted/$target/out",
            "$env/LIMIT",
            "$positions/$root/src/lib.rs",
            "$root/src/answer.txt",
            "$root/src/lib.rs"
        ],
        "every file, variable and build script output the unit read is an entry"
    );
    let mut folded = String::new();
    for (name, digest) in &one.entries {
        folded.push_str(name);
        folded.push('\0');
        folded.push_str(digest);
        folded.push('\n');
    }
    assert_eq!(
        one.skeleton,
        rust_mutants::id::digest(folded.as_bytes()),
        "and the skeleton is exactly their fold, so a reader re-derives it from them"
    );
    let rendered = "pub fn f() -> i32 {sealed:$root/src/lib.rs#0}\nconst K: i32 = 2;\n";
    assert_eq!(
        one.entries.get("$root/src/lib.rs"),
        Some(&rust_mutants::id::digest(rendered.as_bytes())),
        "a Rust file's entry is its bytes with each sealed body replaced by a placeholder \
         naming the file and the item's position among its items, and nothing of the body's \
         bytes or lines"
    );
    assert_eq!(
        one.entries.get("$positions/$root/src/lib.rs"),
        Some(&rust_mutants::id::digest(b"body 1 2:16")),
        "and where the compiler reads a position in it is the start of each body that is not \
         sealed, here the constant's initialiser, by its ordinal, line and column"
    );
}

#[test]
fn a_line_added_inside_a_sealed_body_moves_only_what_the_compiler_reads_a_position_of() {
    let below = |tail: &str| {
        [
            (
                "src/lib.rs",
                format!("pub fn f() -> i32 {{ 1 }}\npub fn g() {{ panic!() }}\n{tail}"),
            ),
            (
                "src/lib.rs",
                format!("pub fn f() -> i32 {{\n    1\n}}\npub fn g() {{ panic!() }}\n{tail}"),
            ),
        ]
    };
    let [before, taller] = below("");
    let (before, taller) = (
        [(before.0, before.1.as_str())],
        [(taller.0, taller.1.as_str())],
    );
    let (was, now) = (
        evidence_of(&unit(&before), &before),
        evidence_of(&unit(&taller), &taller),
    );
    assert_eq!(
        was.units, now.units,
        "the placeholder of a sealed body names neither its bytes nor its lines, so a line added \
         inside one moves no skeleton"
    );
    assert_ne!(
        item(&was, "g").start,
        item(&now, "g").start,
        "and moves the start of every body after it, which an execution that entered `g`, whose \
         `panic!` reports the line it is on, is held to"
    );
    for (tail, consumer) in [
        ("pub const HERE: u32 = line!();\n", "a constant initialiser"),
        ("m!{}\n", "an item-level macro invocation"),
        (
            "/// ```\n/// assert!(true);\n/// ```\npub fn h() {}\n",
            "a documentation code block",
        ),
        ("pub struct S([u8; size()]);\n", "a compile-time call"),
        ("#[derive(Debug)]\npub struct T;\n", "a derive"),
    ] {
        let [before, taller] = below(tail);
        let (before, taller) = (
            [(before.0, before.1.as_str())],
            [(taller.0, taller.1.as_str())],
        );
        assert_ne!(
            evidence_of(&unit(&before), &before).units,
            evidence_of(&unit(&taller), &taller).units,
            "{consumer} below the edit is read by the compiler where it stands, and runs where no \
             test enters, so the line it moved to is in the skeleton"
        );
    }
    for (tail, inert) in [
        (
            "/// A plain sentence.\npub fn h() {}\n",
            "a documentation line with no code",
        ),
        ("pub struct S([u8; 4]);\n", "a literal length"),
        ("#[cfg(test)]\nmod tests {}\n", "a listed attribute"),
        ("#[test]\nfn t() {}\n", "a test attribute"),
    ] {
        let [before, taller] = below(tail);
        let (before, taller) = (
            [(before.0, before.1.as_str())],
            [(taller.0, taller.1.as_str())],
        );
        assert_eq!(
            evidence_of(&unit(&before), &before).units,
            evidence_of(&unit(&taller), &taller).units,
            "{inert} below the edit reads no position, so it moves no skeleton"
        );
    }
}

#[test]
fn nothing_the_compiler_runs_while_it_builds_is_sealed() {
    let files = [("src/lib.rs", "pub fn expand(n: i32) -> i32 { n + 1 }\n")];
    let mut macros = unit(&files);
    macros.kind = "proc-macro".to_owned();
    let kept = evidence_of(&macros, &files);
    assert_eq!(
        item(&kept, "expand").unsealed,
        Some(Unsealing::CompileTime),
        "a procedural macro's body runs in the compiler, where no test enters it, and what it \
         returns is the code of every crate that uses it"
    );
}

#[test]
fn an_item_is_named_by_the_reference_every_record_joins_on() {
    let files = [(
        "src/lib.rs",
        "const K: i32 = 1;\npub fn f() -> i32 { K }\npub fn g() -> i32 { 2 }\n",
    )];
    let kept = evidence_of(&unit(&files), &files);
    let g = item(&kept, "g");
    assert_eq!(
        g.item,
        ItemRef {
            package: "demo".to_owned(),
            path: "src/lib.rs".to_owned(),
            ordinal: 2,
        },
        "an item is named by its package, its file, and its position among the file's \
         cataloged items, the constant included, which is what an entered union names it by"
    );
    let entry = kept
        .units
        .first()
        .and_then(|one| one.entries.get("$root/src/lib.rs"))
        .expect("the file's entry");
    let rendered = "const K: i32 = 1;\npub fn f() -> i32 {sealed:$root/src/lib.rs#1}\npub fn g() -> i32 {sealed:$root/src/lib.rs#2}\n";
    assert_eq!(
        entry,
        &rust_mutants::id::digest(rendered.as_bytes()),
        "and the placeholder names each sealed body by that same ordinal"
    );
}
