// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a body is sealed, as the engine audit reads it again under ADR 0041.

use xtask::engineaudit::carry::{Lists, Unsealed, sealing};

const LISTS: Lists<'static> = Lists {
    macros: &[
        "assert",
        "assert_eq",
        "format",
        "println",
        "vec",
        "matches",
        "panic",
    ],
    attributes: &[
        "inline",
        "must_use",
        "doc",
        "cfg",
        "allow",
        "expect",
        "clippy::*",
    ],
    roots: &["std", "core", "alloc"],
};

/// The sealing of the function `f` in `source`, the unit being that one file, or `None` where the source does not parse or declares no `f`.
fn sealed(source: &str) -> Option<Result<(), Unsealed>> {
    let Ok(file) = syn::parse_str::<syn::File>(source) else {
        return None;
    };
    let item = file.items.iter().find_map(|item| match item {
        syn::Item::Fn(function) if function.sig.ident == "f" => Some(function.clone()),
        _ => None,
    })?;
    Some(sealing(&item, std::slice::from_ref(&file), LISTS))
}

#[test]
fn a_body_that_contributes_only_its_own_execution_is_sealed() {
    for source in [
        "fn f(a: u32) -> u32 { a + 1 }",
        "fn f(a: u32) -> String { format!(\"{a}\") }",
        "fn f() { std::assert_eq!(1, 1); }",
        "#[inline] #[must_use] fn f() -> u32 { let v = vec![1]; v[0] }",
        "use super::*; fn f() {}",
        "fn f() { unsafe { core::hint::unreachable_unchecked() } }",
    ] {
        assert_eq!(sealed(source), Some(Ok(())), "{source}");
    }
}

#[test]
fn a_body_that_contributes_more_than_its_execution_names_why() {
    for (source, why) in [
        ("const fn f() -> u32 { 1 }", Unsealed::Evaluated),
        ("fn f() { foo!(); }", Unsealed::Macro),
        ("fn f() { other::format!(\"x\"); }", Unsealed::Macro),
        (
            "fn f() -> String { format!(\"{}\", env!(\"X\")) }",
            Unsealed::Macro,
        ),
        ("#[tokio::main] fn f() {}", Unsealed::Attribute),
        ("fn f() { fn helper() {} helper() }", Unsealed::DeclaresItem),
        ("fn f() { impl Tr for u8 {} }", Unsealed::DeclaresItem),
        ("fn f() -> u32 { const { 1 } }", Unsealed::ConstBlock),
        (
            "macro_rules! format { () => {} } fn f() {}",
            Unsealed::Shadowed,
        ),
        ("use elsewhere::format; fn f() {}", Unsealed::Shadowed),
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
fn every_reason_a_body_can_be_unsealed_has_a_word() {
    let words: std::collections::BTreeSet<&str> =
        Unsealed::ALL.iter().map(|reason| reason.name()).collect();
    assert_eq!(words.len(), Unsealed::ALL.len());
}
