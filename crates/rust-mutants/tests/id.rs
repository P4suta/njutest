// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The identity recipe, frozen as of v1.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use proptest::prelude::*;
use rust_mutants::id::{
    DISPLAY_ID_LENGTH, ID_DOMAIN, Identity, IdentityError, PathError, digest, display_id_of,
    is_digest, is_id, normalize_path,
};
use rust_mutants::span::{Span, SpanError};

const LF_SOURCE: &[u8] = b"pub fn equal(a: i32, b: i32) -> bool {\n    a == b\n}\n";
const CRLF_SOURCE: &[u8] = b"pub fn equal(a: i32, b: i32) -> bool {\r\n    a == b\r\n}\r\n";
const UNICODE_SOURCE: &[u8] = b"pub const OK: bool = true;\n";
const DELETE_SOURCE: &[u8] = b"pub fn walk() {\n    log_traversal(\"walk\");\n}\n";

struct Vector {
    name: &'static str,
    path: &'static str,
    rule_name: &'static str,
    rule_version: u32,
    span: (u32, u32),
    source: &'static [u8],
    original: &'static [u8],
    replacement: &'static [u8],
    want_id: &'static str,
}

const VECTORS: [Vector; 5] = [
    Vector {
        name: "lf source",
        path: "crates/a/src/score.rs",
        rule_name: "eq-to-neq",
        rule_version: 1,
        span: (1024, 1026),
        source: LF_SOURCE,
        original: b"==",
        replacement: b"!=",
        want_id: "566154251b7671b0d67431bf92ae9e1821ac42893033b853fe30f065e89b391f",
    },
    Vector {
        name: "crlf source",
        path: "crates/a/src/score.rs",
        rule_name: "eq-to-neq",
        rule_version: 1,
        span: (1024, 1026),
        source: CRLF_SOURCE,
        original: b"==",
        replacement: b"!=",
        want_id: "21d8328cfe52381f13d0eac368c03ba3c1b24c178fd423c8ace31a53a74c12ff",
    },
    Vector {
        name: "unicode path",
        path: "crates/a/src/日本語/テスト.rs",
        rule_name: "true-to-false",
        rule_version: 1,
        span: (21, 25),
        source: UNICODE_SOURCE,
        original: b"true",
        replacement: b"false",
        want_id: "d829a40d00aae8e3d57ea65aaddba9a75ca7dfd41028ba658aeaeec33be4174c",
    },
    Vector {
        name: "empty replacement",
        path: "crates/a/src/walk.rs",
        rule_name: "delete-call-statement",
        rule_version: 1,
        span: (2048, 2070),
        source: DELETE_SOURCE,
        original: b"log_traversal(\"walk\");",
        replacement: b"",
        want_id: "23f8ac0600c01b56c03c1415894ea7f93fdde228178e57f7810aa8c157c11690",
    },
    Vector {
        name: "rule version bump",
        path: "crates/a/src/score.rs",
        rule_name: "eq-to-neq",
        rule_version: 2,
        span: (1024, 1026),
        source: LF_SOURCE,
        original: b"==",
        replacement: b"!=",
        want_id: "2247650ae7d56c59a74fd4ec04c6f2959f1fe4beefcbfc2fe3d54a26985afd57",
    },
];

impl Vector {
    fn identity(&self) -> Identity {
        Identity {
            path: self.path.to_owned(),
            rule_name: self.rule_name.to_owned(),
            rule_version: self.rule_version,
            span: Span::new(self.span.0, self.span.1).expect("well formed"),
            source_digest: digest(self.source),
            original_digest: digest(self.original),
            replacement_digest: digest(self.replacement),
        }
    }
}

#[test]
fn the_domain_separator_names_the_recipe_version() {
    assert_eq!(ID_DOMAIN, "rust-mutants-id-v1");
}

#[test]
fn golden_id_vectors() {
    for vector in &VECTORS {
        let got = vector.identity().id().expect("valid identity");
        assert_eq!(got, vector.want_id, "vector {:?}", vector.name);
        assert!(is_id(&got), "{got:?} is not a well-formed id");
    }
}

#[test]
fn golden_vector_ids_are_all_distinct() {
    let mut seen = std::collections::BTreeSet::new();
    for vector in &VECTORS {
        assert!(
            seen.insert(vector.want_id),
            "vector {:?} shares an id",
            vector.name
        );
    }
}

#[test]
fn crlf_and_lf_differ_only_in_the_source_digest() {
    let (lf, crlf) = (&VECTORS[0], &VECTORS[1]);
    assert_eq!(
        (lf.path, lf.rule_name, lf.span, lf.original, lf.replacement),
        (
            crlf.path,
            crlf.rule_name,
            crlf.span,
            crlf.original,
            crlf.replacement
        )
    );
    assert_ne!(lf.source, crlf.source);
    assert_ne!(
        lf.want_id, crlf.want_id,
        "a CRLF checkout must not produce the same mutant id as an LF checkout"
    );
}

#[test]
fn the_empty_replacement_digest_is_the_empty_sha256() {
    assert_eq!(
        digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn fields_are_unambiguously_framed() {
    let base = VECTORS[0].identity();
    let mut a = base.clone();
    a.path = "ab/c.rs".to_owned();
    a.rule_name = "de".to_owned();
    let mut b = base;
    b.path = "ab/c.rsd".to_owned();
    b.rule_name = "e".to_owned();
    assert_ne!(
        a.id().expect("valid"),
        b.id().expect("valid"),
        "two different field splits collided"
    );
}

#[test]
fn every_identity_field_changes_the_id() {
    let base = VECTORS[0].identity();
    let base_id = base.id().expect("valid");
    let variants: [(&str, fn(&mut Identity)); 8] = [
        (
            "path",
            (|id: &mut Identity| id.path = "crates/a/src/other.rs".to_owned()),
        ),
        (
            "rule name",
            (|id: &mut Identity| id.rule_name = "neq-to-eq".to_owned()),
        ),
        ("rule version", (|id: &mut Identity| id.rule_version = 3)),
        ("start byte", (|id: &mut Identity| id.span.start = 1025)),
        ("end byte", (|id: &mut Identity| id.span.end = 1027)),
        (
            "source digest",
            (|id: &mut Identity| id.source_digest = digest(b"other")),
        ),
        (
            "original digest",
            (|id: &mut Identity| id.original_digest = digest(b"!=")),
        ),
        (
            "replacement digest",
            (|id: &mut Identity| id.replacement_digest = digest(b"==")),
        ),
    ];
    for (name, mutate) in &variants {
        let mut changed = VECTORS[0].identity();
        mutate(&mut changed);
        assert_ne!(
            changed.id().expect("valid"),
            base_id,
            "changing the {name} did not change the id"
        );
    }
}

#[test]
fn an_identity_that_does_not_validate_never_produces_an_id() {
    let valid = VECTORS[0].identity();
    valid.validate().expect("the golden identity is valid");
    let cases: [(&str, fn(&mut Identity), fn(&IdentityError) -> bool); 12] = [
        (
            "empty path",
            (|id: &mut Identity| id.path.clear()),
            (|e: &_| matches!(e, IdentityError::Path(PathError::Empty))),
        ),
        (
            "absolute path",
            (|id: &mut Identity| id.path = "/etc/passwd".to_owned()),
            (|e: &_| matches!(e, IdentityError::Path(PathError::Absolute { .. }))),
        ),
        (
            "escaping path",
            (|id: &mut Identity| id.path = "../outside.rs".to_owned()),
            (|e: &_| matches!(e, IdentityError::Path(PathError::Escaping { .. }))),
        ),
        (
            "backslash path is not normalized",
            (|id: &mut Identity| id.path = r"crates\a\src\score.rs".to_owned()),
            (|e: &_| matches!(e, IdentityError::UnnormalizedPath { .. })),
        ),
        (
            "dot-slash path is not normalized",
            (|id: &mut Identity| id.path = "./crates/a/src/score.rs".to_owned()),
            (|e: &_| matches!(e, IdentityError::UnnormalizedPath { .. })),
        ),
        (
            "empty rule name",
            (|id: &mut Identity| id.rule_name.clear()),
            (|e: &_| matches!(e, IdentityError::InvalidRuleName { .. })),
        ),
        (
            "rule name with a version suffix",
            (|id: &mut Identity| id.rule_name = "eq-to-neq@1".to_owned()),
            (|e: &_| matches!(e, IdentityError::InvalidRuleName { .. })),
        ),
        (
            "zero rule version",
            (|id: &mut Identity| id.rule_version = 0),
            (|e: &_| matches!(e, IdentityError::InvalidRuleVersion { version: 0 })),
        ),
        (
            "reversed span",
            (|id: &mut Identity| id.span = Span { start: 9, end: 4 }),
            (|e: &_| {
                matches!(
                    e,
                    IdentityError::Span(SpanError::Reversed { start: 9, end: 4 })
                )
            }),
        ),
        (
            "short source digest",
            (|id: &mut Identity| id.source_digest = "abc".to_owned()),
            (|e: &_| {
                matches!(
                    e,
                    IdentityError::InvalidDigest {
                        field: "source",
                        ..
                    }
                )
            }),
        ),
        (
            "uppercase original digest",
            (|id: &mut Identity| id.original_digest = id.original_digest.to_uppercase()),
            (|e: &_| {
                matches!(
                    e,
                    IdentityError::InvalidDigest {
                        field: "original",
                        ..
                    }
                )
            }),
        ),
        (
            "non-hex replacement digest",
            (|id: &mut Identity| id.replacement_digest = "z".repeat(64)),
            (|e: &_| {
                matches!(
                    e,
                    IdentityError::InvalidDigest {
                        field: "replacement",
                        ..
                    }
                )
            }),
        ),
    ];
    for (name, mutate, expected) in &cases {
        let mut id = VECTORS[0].identity();
        mutate(&mut id);
        let error = id.validate().expect_err(name);
        assert!(expected(&error), "{name}: unexpected error {error:?}");
        assert!(
            id.id().is_err(),
            "{name}: an invalid identity produced an id"
        );
    }
}

#[test]
fn normalize_path_canonicalizes_and_refuses_paths_outside_the_workspace() {
    let ok = [
        ("crates/a/src/score.rs", "crates/a/src/score.rs"),
        (r"crates\a\src\score.rs", "crates/a/src/score.rs"),
        ("./crates/score.rs", "crates/score.rs"),
        ("crates//a/./score.rs", "crates/a/score.rs"),
        ("crates/b/../a/score.rs", "crates/a/score.rs"),
        ("crates/日本語/テスト.rs", "crates/日本語/テスト.rs"),
        ("main.rs", "main.rs"),
        ("1:/repo/score.rs", "1:/repo/score.rs"),
    ];
    for (input, want) in ok {
        let got = normalize_path(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(got, want, "{input:?}");
        assert_eq!(
            normalize_path(&got).expect("idempotent"),
            got,
            "normalization is idempotent"
        );
    }
    let refused: [(&str, fn(&PathError) -> bool); 12] = [
        ("", (|e: &_| matches!(e, PathError::Empty))),
        (
            "crates/sco\0re.rs",
            (|e: &_| matches!(e, PathError::NulByte)),
        ),
        (
            "/crates/score.rs",
            (|e: &_| matches!(e, PathError::Absolute { .. })),
        ),
        (
            r"C:\repo\score.rs",
            (|e: &_| matches!(e, PathError::VolumeName { .. })),
        ),
        ("c:", (|e: &_| matches!(e, PathError::VolumeName { .. }))),
        ("./A:", (|e: &_| matches!(e, PathError::VolumeName { .. }))),
        (
            r"a:\repo\score.rs",
            (|e: &_| matches!(e, PathError::VolumeName { .. })),
        ),
        (
            r"Z:\repo\score.rs",
            (|e: &_| matches!(e, PathError::VolumeName { .. })),
        ),
        (
            "../score.rs",
            (|e: &_| matches!(e, PathError::Escaping { .. })),
        ),
        (
            "crates/../../score.rs",
            (|e: &_| matches!(e, PathError::Escaping { .. })),
        ),
        (".", (|e: &_| matches!(e, PathError::Escaping { .. }))),
        ("..", (|e: &_| matches!(e, PathError::Escaping { .. }))),
    ];
    for (input, expected) in &refused {
        let error = normalize_path(input).expect_err(input);
        assert!(expected(&error), "{input:?}: unexpected error {error:?}");
    }
}

#[test]
fn display_id_is_the_first_twenty_hex_digits_of_a_full_id() {
    let full = VECTORS[0].want_id;
    let short = display_id_of(full).expect("full id");
    assert_eq!(short.len(), DISPLAY_ID_LENGTH);
    assert_eq!(short, "566154251b7671b0d674");
    for bad in [
        "",
        "abc",
        &full.to_uppercase(),
        &format!("{full}0"),
        &"z".repeat(64),
    ] {
        assert!(
            matches!(display_id_of(bad), Err(IdentityError::InvalidId { .. })),
            "{bad:?}"
        );
    }
}

#[test]
fn is_id_and_is_digest_accept_exactly_64_lowercase_hex_characters() {
    let full = VECTORS[0].want_id;
    assert!(is_id(full));
    assert!(is_digest(full));
    for bad in [
        "",
        &full[..63],
        &format!("{full}a"),
        &full.to_uppercase(),
        &"g".repeat(64),
    ] {
        assert!(!is_id(bad), "{bad:?}");
        assert!(!is_digest(bad), "{bad:?}");
    }
}

proptest! {
    #[test]
    fn the_id_is_a_deterministic_function_of_the_identity(
        path in "[a-z][a-z0-9_]{0,8}(/[a-z][a-z0-9_]{0,8}){0,3}\\.rs",
        rule in "[a-z]+(-[a-z]+){1,3}",
        version in 1u32..5,
        start in 0u32..10_000,
        len in 0u32..50,
        source in proptest::collection::vec(any::<u8>(), 0..64),
        original in proptest::collection::vec(any::<u8>(), 0..16),
        replacement in proptest::collection::vec(any::<u8>(), 0..16),
    ) {
        let identity = Identity {
            path,
            rule_name: rule,
            rule_version: version,
            span: Span::new(start, start + len).expect("well formed"),
            source_digest: digest(&source),
            original_digest: digest(&original),
            replacement_digest: digest(&replacement),
        };
        let first = identity.id().expect("valid");
        prop_assert!(is_id(&first));
        prop_assert_eq!(identity.id().expect("valid"), first);
    }
}
