// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::opens_a_block;

#[test]
fn every_way_an_expression_can_open_a_block_is_read_as_one_and_nothing_else_is() {
    for opening in [
        "{ a }",
        "#[cfg(x)] { a }",
        "'label: { break 'label a; }",
        "if a { b } else { c }",
        "if(a) { b } else { c }",
        "match a { _ => b }",
        "loop { break a; }",
        "while a { b(); }",
        "for x in a { b(x); }",
        "unsafe { a }",
        "unsafe{ a }",
        "const { 1 }",
        "async { a }",
        "async move { a }",
    ] {
        assert!(
            opens_a_block(opening),
            "{opening} ends at its own closing brace, so its guard has to as well"
        );
    }
    for plain in [
        "",
        "a",
        "iffy()",
        "if_then()",
        "for_each(a)",
        "matches!(a, b)",
        "looping()",
        "whilst",
        "fortune()",
        "unsafely()",
        "constant",
        "asyncio()",
        "f({ a })",
        "(match a { _ => b })",
        "!loop_count",
        "_",
    ] {
        assert!(
            !opens_a_block(plain),
            "{plain:?} begins with no block, so the identity macro can hold its guard"
        );
    }
}

#[test]
fn a_block_is_read_from_its_first_word_alone() {
    for (text, opens) in [
        ("match", true),
        ("while_x", false),
        ("x_while", false),
        ("r#match", false),
        ("async_fn()", false),
        ("unsafe_cell", false),
    ] {
        assert_eq!(
            opens_a_block(text),
            opens,
            "{text:?}: a keyword opens a block only as a whole word at the start"
        );
    }
}
