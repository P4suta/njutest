// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a remembered outcome is filed under, and what makes it stop answering.
//!
//! A key names the question, not the tree. Everything it is computed from is
//! something that could change what the tests say about the mutant; everything
//! else is left out, because a key over more than that throws away answers for
//! reasons that could not have changed them.

#[test]
fn a_key_over_nothing_is_not_a_key_and_remembers_nothing() {
    let keyed = rust_mutants::outcomes::Keyed {
        closure: String::new(),
        manifests: "m".to_owned(),
        toolchain: "rustc 1.98.0".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        build: Vec::new(),
    };
    assert!(
        !keyed.usable(),
        "a build whose dep-info could not be read names no closure, and a key over nothing would \
         file every mutant of every tree under one name"
    );
    let usable = rust_mutants::outcomes::Keyed {
        closure: "c".to_owned(),
        ..keyed
    };
    assert!(usable.usable());
}

#[test]
fn what_the_key_is_computed_from_is_what_could_change_the_answer() {
    let base = rust_mutants::outcomes::Keyed {
        closure: "c".to_owned(),
        manifests: "m".to_owned(),
        toolchain: "rustc 1.98.0".to_owned(),
        args: vec!["--test-threads=1".to_owned()],
        timeout: "auto".to_owned(),
        build: vec!["--all-features".to_owned()],
    };
    let key = base.key("mutant");
    for other in [
        rust_mutants::outcomes::Keyed {
            closure: "other".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            manifests: "other".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            toolchain: "rustc 1.99.0".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            args: Vec::new(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            timeout: "30s".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            build: Vec::new(),
            ..base.clone()
        },
    ] {
        assert_ne!(
            other.key("mutant"),
            key,
            "{other:?} names a different program and files under the same name"
        );
    }
    assert_ne!(base.key("another"), key, "and so does another mutant");
    assert_eq!(base.key("mutant"), key, "the same question is the same key");
}
