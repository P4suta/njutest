// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Flattening: one line, same tokens, same literal values.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::str::FromStr;

use proptest::prelude::*;
use rust_mutants::flatten::{FlattenError, flatten};

fn flat(src: &str) -> String {
    flatten(src).unwrap_or_else(|e| panic!("{src:?}: {e}"))
}

fn lexes_the_same(a: &str, b: &str) -> bool {
    let ta = proc_macro2::TokenStream::from_str(a)
        .expect("lexes")
        .to_string();
    let tb = proc_macro2::TokenStream::from_str(b)
        .expect("lexes")
        .to_string();
    ta == tb
}

#[test]
fn a_doc_comment_expands_to_an_attribute_and_keeps_its_neighbour_apart() {
    // A doc comment is not a comment to the lexer: it is `#[doc = "…"]`.
    // Written where the comment was, its `#` would touch the token before
    // it and change that token's spacing, which is a different stream.
    let src = "a a a ==/// c\n";
    let out = flat(src);
    assert!(!out.contains('\n'), "{out:?}");
    assert!(
        out.starts_with("a a a == #"),
        "the neighbour stays apart: {out:?}"
    );
    assert!(lexes_the_same(src, &out), "{src:?} -> {out:?}");

    // An inner doc comment is the same shape.
    let inner = "a//! c\n";
    let out = flat(inner);
    assert!(lexes_the_same(inner, &out), "{inner:?} -> {out:?}");
}

#[test]
fn a_fragment_already_on_one_line_is_returned_byte_for_byte() {
    for src in [
        "a + b",
        "let x = f(a,  b);",
        "x.y::<T>()  -> i32",
        "'a: loop {}",
        "r#type",
    ] {
        assert_eq!(flat(src), src);
    }
}

#[test]
fn line_breaks_and_indentation_between_tokens_fold_to_one_space() {
    let src = "f(\n    a,\n    b,\n)";
    assert_eq!(flat(src), "f( a, b, )");
    assert_eq!(flat("a\r\n    + b"), "a + b");
    assert_eq!(
        flat("  a +\n b  "),
        "a + b",
        "leading and trailing whitespace is not part of a token"
    );
}

#[test]
fn comments_are_dropped_and_the_tokens_around_them_stay_apart() {
    assert_eq!(flat("f(a, // the second\n  b)"), "f(a, b)");
    assert_eq!(flat("a /* mid */ + b"), "a + b");
    assert_eq!(flat("a /* two\nlines */ + b"), "a + b");
    assert_eq!(flat("x// trailing"), "x");
}

#[test]
fn a_doc_comment_becomes_the_attribute_the_lexer_reads_it_as() {
    let out = flat("{\n    /// documented\n    fn inner() {}\n    inner()\n}");
    assert!(!out.contains('\n'));
    assert!(out.contains("doc") && out.contains("documented"), "{out}");
    assert!(
        lexes_the_same(&out, "{ #[doc = \" documented\"] fn inner() {} inner() }")
            || lexes_the_same(&out, "{ #[doc = r\" documented\"] fn inner() {} inner() }"),
        "{out}"
    );
}

#[test]
fn a_raw_string_spanning_lines_becomes_an_escaped_literal_with_the_same_value() {
    let out = flat("let s = r\"line one\nline two\";");
    assert_eq!(out, "let s = \"line one\\nline two\";");
    let out = flat("let s = r#\"he said \"hi\"\nagain\"#;");
    assert_eq!(out, "let s = \"he said \\\"hi\\\"\\nagain\";");
    let out = flat("let b = br\"a\nb\";");
    assert_eq!(out, "let b = b\"a\\nb\";");
    let out = flat("let c = cr\"a\nb\";");
    assert_eq!(out, "let c = c\"a\\nb\";");
}

#[test]
fn a_string_continued_with_a_backslash_becomes_its_value_on_one_line() {
    assert_eq!(flat("\"abc\\\n    def\""), "\"abcdef\"");
    assert_eq!(
        flat("\"tab\\tkept\""),
        "\"tab\\tkept\"",
        "a literal on one line is untouched"
    );
}

#[test]
fn crlf_inside_a_multi_line_literal_folds_to_the_value_the_compiler_sees() {
    assert_eq!(flat("r\"a\r\nb\""), "\"a\\nb\"");
}

#[test]
fn a_fragment_that_does_not_lex_is_refused() {
    assert!(matches!(
        flatten("\"unterminated"),
        Err(FlattenError::Untokenizable { .. })
    ));
    assert!(matches!(
        flatten("f("),
        Err(FlattenError::Untokenizable { .. })
    ));
}

#[test]
fn an_empty_or_comment_only_fragment_flattens_to_nothing() {
    assert_eq!(flat(""), "");
    assert_eq!(flat("   \n  "), "");
    assert_eq!(flat("// only\n"), "");
}

#[test]
fn nested_groups_and_punctuation_keep_their_shape() {
    let out = flat("match x {\n    Some(y) => y,\n    None => 0,\n}");
    assert_eq!(out, "match x { Some(y) => y, None => 0, }");
    let out = flat("a\n->\nb");
    assert_eq!(out, "a -> b");
    assert_eq!(
        flat("::std::mem::swap(&mut a,\n&mut b)"),
        "::std::mem::swap(&mut a, &mut b)"
    );
}

proptest! {
    #[test]
    fn the_output_has_no_line_break_and_lexes_to_the_same_stream(
        tokens in proptest::collection::vec(
            prop_oneof![
                "[a-z_][a-z0-9_]{0,4}".prop_map(|s| s),
                "[0-9]{1,3}(u8|i32|)".prop_map(|s| s),
                "(\\+|-|\\*|/|==|!=|<|<=|&&|\\|\\||->|::|=>|,|;|\\.|&|!|\\?)".prop_map(|s| s),
                "\"[a-z ]{0,5}\"".prop_map(|s| s),
                "r#\"[a-z ]{0,5}\"#".prop_map(|s| s),
                "'[a-z]'".prop_map(|s| s),
            ],
            1..12,
        ),
        gaps in proptest::collection::vec("( |\n|\r\n|  \n  |/\\* c \\*/|// c\n|)", 12),
        wrap in prop_oneof![Just(""), Just("()"), Just("{}"), Just("[]")],
    ) {
        let mut src = String::new();
        for (token, gap) in tokens.iter().zip(&gaps) {
            src.push_str(token);
            src.push_str(gap);
        }
        if !wrap.is_empty() {
            src = format!("{}{}{}", &wrap[..1], src, &wrap[1..]);
        }
        prop_assume!(proc_macro2::TokenStream::from_str(&src).is_ok());
        let out = flatten(&src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        prop_assert!(!out.contains('\n') && !out.contains('\r'), "{:?} -> {:?}", src, out);
        prop_assert!(lexes_the_same(&src, &out), "{:?} -> {:?}", src, out);
        prop_assert_eq!(flatten(&out).expect("idempotent"), out, "flattening is idempotent");
    }
}
