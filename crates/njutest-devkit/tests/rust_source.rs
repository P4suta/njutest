// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A question about this repository's Rust source is answered by its items, whatever text surrounds them.

use njutest_devkit::rust_source::{
    RustSourceError, names_listed, public_text_constants, strings_listed,
};

const REGISTER: &str = r#"
pub const FIRST: &str = "first";
pub const SECOND: &str = "second";
const PRIVATE: &str = "private";
pub const COUNT: usize = 2;
pub const ALL: [&str; 2] = [
    FIRST,
    SECOND,
];
const FLAGS: [&str; 2] = ["--one", "--two"];
"#;

const SURROUNDINGS: &str = r#"
mod __generated {
    pub const ALL: [&str; 1] = [DECOY];
    pub const DECOY: &str = "decoy";
    fn say(nonce: u64) -> String {
        format!(
            "{}\t{}",
            nonce,
            CATALOG,
        )
    }
    const FLAGS: [&str; 1] = ["--decoy"];
}
"#;

#[test]
fn a_register_is_read_by_its_items_and_not_its_lines() {
    for text in [REGISTER.to_owned(), format!("{REGISTER}{SURROUNDINGS}")] {
        assert!(
            matches!(public_text_constants(&text), Ok(names) if names == ["FIRST", "SECOND"]),
            "only the top-level public &str constants are declared ones: {:?}",
            public_text_constants(&text)
        );
        assert!(
            matches!(names_listed(&text, "ALL"), Ok(names) if names == ["FIRST", "SECOND"]),
            "a line of a nested item, however it is laid out, is not an element of the register: \
             {:?}",
            names_listed(&text, "ALL")
        );
        assert!(
            matches!(strings_listed(&text, "FLAGS"), Ok(flags) if flags == ["--one", "--two"]),
            "{:?}",
            strings_listed(&text, "FLAGS")
        );
    }
}

#[test]
fn a_question_the_source_cannot_answer_is_refused_by_name() {
    assert!(matches!(
        names_listed(REGISTER, "MISSING"),
        Err(RustSourceError::NoConstant { name }) if name == "MISSING"
    ));
    assert!(matches!(
        names_listed(REGISTER, "COUNT"),
        Err(RustSourceError::NotAnArray { name }) if name == "COUNT"
    ));
    assert!(matches!(
        names_listed(REGISTER, "FLAGS"),
        Err(RustSourceError::UnexpectedElement { name, wanted: "a bare name" }) if name == "FLAGS"
    ));
    assert!(matches!(
        strings_listed(REGISTER, "ALL"),
        Err(RustSourceError::UnexpectedElement { name, wanted: "a string literal" }) if name == "ALL"
    ));
    assert!(matches!(
        public_text_constants("pub const ALL: [&str; 1] = ["),
        Err(RustSourceError::Unparsed { .. })
    ));
}
