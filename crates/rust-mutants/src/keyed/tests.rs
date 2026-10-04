// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Binding, NAME_LENGTH, RECORD_NAME, bind, binding, name, names};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHARING: &str = "0123456789abcdeffedcba9876543210fedcba9876543210fedcba9876543210";

#[test]
fn a_key_is_named_by_its_first_sixteen_digits_and_nothing_else_is_named() {
    assert_eq!(
        name(KEY).expect("a full digest is a key"),
        "0123456789abcdef"
    );
    assert_eq!(
        name("0123456789abcdef0123456789abcdef").expect("an observation identity is a key"),
        "0123456789abcdef"
    );
    for refused in [
        "0123456789abcde",
        "0123456789ABCDEF0123456789abcdef",
        "0123456789abcdef/..",
        "0123456789abcdeg0123456789abcdef",
        "",
    ] {
        assert!(name(refused).is_err(), "{refused:?} names no directory");
    }
    assert!(names("0123456789abcdef"));
    assert!(!names(KEY), "a whole key is not a name a path spells");
    assert!(!names("0123456789abcde"));
    assert_eq!(NAME_LENGTH, 16);
}

#[test]
fn a_directory_keeps_the_first_key_bound_to_it_and_tells_another_apart() {
    let directory = tempfile::tempdir().expect("a directory a key's prefix names");
    assert_eq!(
        binding(directory.path(), KEY).expect("a first binding"),
        Binding::Bound
    );
    assert_eq!(
        std::fs::read(directory.path().join(RECORD_NAME)).expect("the record"),
        KEY.as_bytes(),
        "the record keeps the whole key"
    );
    assert_eq!(
        binding(directory.path(), KEY).expect("the same key again"),
        Binding::Bound
    );
    assert_eq!(
        name(KEY).expect("a key"),
        name(SHARING).expect("a key"),
        "both keys share a name"
    );
    assert_eq!(
        binding(directory.path(), SHARING).expect("another key's binding"),
        Binding::Another,
        "a key that shares the name is never taken for the one the record keeps"
    );
    assert_eq!(
        std::fs::read(directory.path().join(RECORD_NAME)).expect("the record"),
        KEY.as_bytes(),
        "another key leaves the record as it was"
    );
    let refused = bind(directory.path(), SHARING).expect_err("bind refuses another key");
    assert!(
        refused.to_string().contains("keeps another key"),
        "{refused}"
    );
}

#[test]
fn a_record_that_holds_part_of_a_key_is_another_key() {
    let directory = tempfile::tempdir().expect("a directory a key's prefix names");
    std::fs::write(
        directory.path().join(RECORD_NAME),
        KEY.get(..NAME_LENGTH).expect("a key holds its name"),
    )
    .expect("a planted record");
    assert_eq!(
        binding(directory.path(), KEY).expect("a binding against a damaged record"),
        Binding::Another
    );
}
