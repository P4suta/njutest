// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a body is sealed, as the engine audit reads it again from `docs/engine/carry.md`.

use xtask::engineaudit::carry::{PageLists, Unsealed, line_column, sealing};

/// The lists the page states.
fn page() -> Option<PageLists> {
    let path = njutest_devkit::paths::workspace_root().join("docs/engine/carry.md");
    let Ok(text) = std::fs::read_to_string(path) else {
        return None;
    };
    let Ok(lists) = PageLists::read(&text) else {
        return None;
    };
    Some(lists)
}

/// The sealing of the function `f` in `source`, the unit being that one file, or `None` where the source, the page, or `f`'s body cannot be read.
fn sealed(source: &str) -> Option<Result<(), Unsealed>> {
    let lists = page()?;
    let Ok(file) = syn::parse_str::<syn::File>(source) else {
        return None;
    };
    let named = source.find("fn f")?;
    let brace = named.checked_add(source.get(named..)?.find('{')?)?;
    let start = line_column(source, brace)?;
    Some(sealing(
        &file,
        start,
        std::slice::from_ref(&file),
        lists.lists(),
    ))
}

#[test]
fn the_page_states_every_list_the_audit_reads() {
    let lists = page();
    assert!(
        lists
            .as_ref()
            .is_some_and(|lists| lists.macros.contains(&"format".to_owned())
                && lists.roots.contains(&"std".to_owned())
                && lists.local.contains(&"crate".to_owned())
                && lists.attributes.contains(&"inline".to_owned())
                && lists.tools.contains(&"clippy".to_owned())),
        "{lists:?}"
    );
}

#[test]
fn a_body_that_contributes_only_its_own_execution_is_sealed() {
    for source in [
        "fn f(a: u32) -> u32 { a + 1 }",
        "fn f(a: u32) -> String { format!(\"{a}\") }",
        "fn f() { std::assert_eq!(1, 1); }",
        "#[inline] #[must_use] fn f() -> u32 { let v = vec![1]; v[0] }",
        "#[clippy::cognitive_complexity = \"9\"] fn f() {}",
        "#[cfg_attr(test, inline)] fn f() {}",
        "use super::*; fn f() {}",
        "impl S { fn f(&self) -> bool { matches!(self, S) } }",
        "trait T { fn f(&self) -> u32 { 1 } }",
        "mod m { fn f() { if !true { panic!(\"no\") } } }",
    ] {
        assert_eq!(sealed(source), Some(Ok(())), "{source}");
    }
}

#[test]
fn a_body_that_contributes_more_than_its_execution_names_why() {
    for (source, why) in [
        ("const fn f() -> u32 { 1 }", Unsealed::Evaluated),
        ("#[tokio::main] fn f() {}", Unsealed::Attribute),
        ("#![feature(x)] fn f() {}", Unsealed::Attribute),
        (
            "#[cfg_attr(test, tokio::main)] fn f() {}",
            Unsealed::Attribute,
        ),
        ("#[derive_more] impl S { fn f() {} }", Unsealed::Attribute),
        ("fn f() { foo!(); }", Unsealed::Macro),
        ("fn f() { other::format!(\"x\"); }", Unsealed::Macro),
        ("fn f() { ::std::format!(\"x\"); }", Unsealed::Macro),
        (
            "fn f() -> String { format!(\"{}\", env!(\"X\")) }",
            Unsealed::Macro,
        ),
        ("fn f() { fn helper() {} helper() }", Unsealed::DeclaresItem),
        ("fn f() { impl Tr for u8 {} }", Unsealed::DeclaresItem),
        ("fn f() -> u32 { const { 1 } }", Unsealed::ConstBlock),
        (
            "macro_rules! format { () => {} } fn f() {}",
            Unsealed::Shadowed,
        ),
        ("use elsewhere::format; fn f() {}", Unsealed::Shadowed),
        (
            "use elsewhere::{other as vec}; fn f() {}",
            Unsealed::Shadowed,
        ),
        ("use elsewhere::*; fn f() {}", Unsealed::ForeignGlob),
        (
            "#[macro_use] extern crate other; fn f() {}",
            Unsealed::ForeignGlob,
        ),
    ] {
        assert_eq!(sealed(source), Some(Err(why)), "{source}");
    }
}

#[test]
fn a_span_the_parser_cannot_find_a_body_at_is_unlocated() {
    let source = "fn f() {}\nfn g() {}";
    let Ok(file) = syn::parse_str::<syn::File>(source) else {
        return;
    };
    let lists = page();
    assert!(lists.is_some(), "the page");
    let Some(lists) = lists else { return };
    let Some(inside) = line_column(source, 3) else {
        return;
    };
    assert_eq!(
        sealing(&file, inside, std::slice::from_ref(&file), lists.lists()),
        Err(Unsealed::Unlocated)
    );
}

#[test]
fn every_reason_a_body_can_be_unsealed_has_a_word() {
    let words: std::collections::BTreeSet<&str> =
        Unsealed::ALL.iter().map(|reason| reason.name()).collect();
    assert_eq!(words.len(), Unsealed::ALL.len());
}
