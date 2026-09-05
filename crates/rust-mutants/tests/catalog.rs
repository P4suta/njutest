// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The catalog is a pure function of the candidate set: validated,
//! identified, deduplicated, canonically ordered, densely indexed.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::catalog::{
    BuildError, Builder, CATALOG_DOMAIN, Candidate, CandidateError, DisplayCollisionError,
    DuplicateReason, PrefixError,
};
use rust_mutants::id::{IdentityError, digest};
use rust_mutants::rule::{Family, Registry, Rule, RuleError, Tier};
use rust_mutants::span::Span;

const SOURCE: &[u8] = b"pub fn f(a: i32, b: i32) -> bool { a < b && a == b }\n";

fn registered(name: &str) -> Rule {
    Registry::canonical().lookup(name).expect("registered")
}

/// `(path, rule, (start, end), original, replacement)`.
type Edit<'a> = (&'a str, &'a str, (u32, u32), &'a [u8], &'a [u8]);

fn candidate((path, rule, span, original, replacement): Edit<'_>) -> Candidate {
    Candidate {
        path: path.to_owned(),
        rule: registered(rule),
        span: Span::new(span.0, span.1).expect("well formed"),
        original: original.to_vec(),
        replacement: replacement.to_vec(),
        source_digest: digest(SOURCE),
    }
}

// Minted by the Python reference implementation of the recipe.
const ID_A: &str = "80d0bd6ede781c3eb430fd8335de916e77333b0e3c3d10f1f134cdd566379b41";
const ID_B: &str = "febc861aa0e6bd6983317697bac848217184777186171d4589b1f2bf0e343f4a";
const CATALOG_DIGEST_AB: &str = "0e6fab7d3e66510bdacf680fec254d8bfa89a7be44334b0e08b2415a6422a2cf";
const DISPLAY_A: &str = "80d0bd6ede781c3eb430";

fn a() -> Candidate {
    candidate(("crates/a/src/a.rs", "lt-to-le", (37, 38), b"<", b"<="))
}

fn b() -> Candidate {
    candidate(("crates/a/src/b.rs", "eq-to-neq", (46, 48), b"==", b"!="))
}

#[test]
fn a_candidate_hashes_to_the_recipe_identity() {
    assert_eq!(a().id().expect("valid"), ID_A);
    assert_eq!(b().id().expect("valid"), ID_B);
    let identity = a().identity();
    assert_eq!(identity.original_digest, digest(b"<"));
    assert_eq!(identity.replacement_digest, digest(b"<="));
}

#[test]
fn a_candidate_whose_original_is_not_the_span_length_is_refused() {
    let mut wrong = a();
    wrong.original = b"<<".to_vec();
    assert!(matches!(
        wrong.validate(),
        Err(CandidateError::OriginalLengthMismatch {
            span_len: 1,
            original_len: 2,
            ..
        })
    ));
    wrong.id().expect_err("an incoherent candidate mints no id");
}

#[test]
fn a_replacement_identical_to_the_original_is_not_a_mutation() {
    let mut noop = a();
    noop.replacement = noop.original.clone();
    assert!(matches!(
        noop.validate(),
        Err(CandidateError::NoOpReplacement { .. })
    ));
    let mut empty_on_empty = a();
    empty_on_empty.span = Span::new(37, 37).expect("empty");
    empty_on_empty.original.clear();
    empty_on_empty.replacement.clear();
    assert!(matches!(
        empty_on_empty.validate(),
        Err(CandidateError::NoOpReplacement { .. })
    ));
}

#[test]
fn an_invalid_identity_is_refused_before_anything_else() {
    let mut bad = a();
    bad.path = "/abs/a.rs".to_owned();
    assert!(matches!(
        bad.validate(),
        Err(CandidateError::Identity(IdentityError::Path(_)))
    ));
}

#[test]
fn the_builder_refuses_an_unregistered_or_mismatched_rule() {
    let mut builder = Builder::new();
    let mut bumped = a();
    bumped.rule = Rule {
        version: 2,
        ..bumped.rule
    };
    assert!(matches!(
        builder.add(bumped),
        Err(CandidateError::Rule(RuleError::Mismatch { .. }))
    ));
    let mut unknown = a();
    unknown.rule = Rule {
        name: "lt-to-gt",
        ..unknown.rule
    };
    assert!(matches!(
        builder.add(unknown),
        Err(CandidateError::Rule(RuleError::UnknownRule { .. }))
    ));
    assert!(builder.is_empty());
}

#[test]
fn the_builder_refuses_contradictions_about_one_file() {
    let mut builder = Builder::new();
    builder.add(a()).expect("first");
    let mut other_digest = candidate(("crates/a/src/a.rs", "eq-to-neq", (46, 48), b"==", b"!="));
    other_digest.source_digest = digest(b"a different file");
    assert!(matches!(
        builder.add(other_digest),
        Err(CandidateError::SourceDigestConflict { .. })
    ));
    let other_original = candidate(("crates/a/src/a.rs", "le-to-lt", (37, 38), b">", b"<"));
    assert!(matches!(
        builder.add(other_original),
        Err(CandidateError::OriginalConflict { .. })
    ));
    assert_eq!(builder.len(), 1);
}

#[test]
fn the_catalog_is_a_pure_function_of_the_candidate_set() {
    let mut forward = Builder::new();
    forward.add_all([a(), b()]).expect("valid");
    let mut backward = Builder::new();
    backward.add_all([b(), a()]).expect("valid");
    let catalog = forward.build().expect("builds");
    assert_eq!(
        catalog,
        backward.build().expect("builds"),
        "insertion order does not matter"
    );
    assert_eq!(catalog.len(), 2);
    let ids: Vec<&str> = catalog.mutants().iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [ID_A, ID_B], "canonical order: by path bytes");
    let indices: Vec<u32> = catalog.mutants().iter().map(|m| m.index).collect();
    assert_eq!(indices, [0, 1], "dense indices from zero");
    assert_eq!(catalog.mutants()[0].display_id, DISPLAY_A);
    assert_eq!(catalog.display_length(), 20);
    assert_eq!(CATALOG_DOMAIN, "rust-mutants-catalog-v1");
    assert_eq!(
        catalog.digest(),
        CATALOG_DIGEST_AB,
        "minted by the Python reference implementation"
    );
}

#[test]
fn canonical_order_within_a_file_is_span_then_registry_position_then_replacement() {
    let late = candidate(("crates/a/src/a.rs", "eq-to-neq", (46, 48), b"==", b"!="));
    let early = a();
    let mut builder = Builder::new();
    builder
        .add_all([late.clone(), early.clone()])
        .expect("valid");
    let catalog = builder.build().expect("builds");
    let spans: Vec<Span> = catalog.mutants().iter().map(|m| m.candidate.span).collect();
    assert_eq!(spans, [early.span, late.span]);
}

#[test]
fn accessors_find_a_mutant_by_position_index_id_and_display_id() {
    let mut builder = Builder::new();
    builder.add_all([a(), b()]).expect("valid");
    let catalog = builder.build().expect("builds");
    assert_eq!(catalog.at(1).map(|m| m.id.as_str()), Some(ID_B));
    assert_eq!(catalog.at(2), None);
    assert_eq!(catalog.by_index(0).map(|m| m.id.as_str()), Some(ID_A));
    assert_eq!(catalog.by_index(7), None);
    assert_eq!(catalog.by_id(ID_B).map(|m| m.index), Some(1));
    assert_eq!(catalog.by_display_id(DISPLAY_A).map(|m| m.index), Some(0));
    assert_eq!(catalog.by_display_id("nope"), None);
}

#[test]
fn identical_candidates_collapse_and_the_duplicate_is_recorded() {
    let mut builder = Builder::new();
    builder.add_all([a(), a()]).expect("valid");
    let catalog = builder.build().expect("builds");
    assert_eq!(catalog.len(), 1);
    let duplicates = catalog.duplicates();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].reason, DuplicateReason::Identical);
    assert_eq!(duplicates[0].dropped_id, duplicates[0].winner_id);
    assert_eq!(
        DuplicateReason::Identical.to_string(),
        "identical-candidate"
    );
}

#[test]
fn the_same_edit_from_two_rules_is_won_by_the_earlier_table_row() {
    const TABLE: [Rule; 2] = [
        Rule {
            family: Family::BooleanLiteral,
            name: "true-to-false",
            version: 1,
            tier: Tier::Balanced,
        },
        Rule {
            family: Family::StatementDeletion,
            name: "literal-to-false",
            version: 1,
            tier: Tier::All,
        },
    ];
    let registry = Registry::new(&TABLE).expect("consistent");
    let mut local = a();
    local.rule = TABLE[0];
    local.original = b"t".to_vec();
    local.replacement = b"f".to_vec();
    let mut broad = local.clone();
    broad.rule = TABLE[1];
    for order in [[local.clone(), broad.clone()], [broad, local]] {
        let mut builder = Builder::with_registry(registry);
        builder.add_all(order).expect("valid");
        let catalog = builder.build().expect("builds");
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog.mutants()[0].candidate.rule,
            TABLE[0],
            "the more local rule wins"
        );
        let duplicate = &catalog.duplicates()[0];
        assert_eq!(duplicate.reason, DuplicateReason::Shadowed);
        assert_eq!(duplicate.dropped.rule, TABLE[1]);
        assert_eq!(duplicate.winner_rule, TABLE[0]);
        assert_ne!(
            duplicate.dropped_id, duplicate.winner_id,
            "different rules, different identities"
        );
    }
    assert_eq!(
        DuplicateReason::Shadowed.to_string(),
        "shadowed-by-more-local-rule"
    );
}

#[test]
fn a_display_id_collision_is_a_diagnosable_error_not_a_panic() {
    let mut builder = Builder::new().with_display_length(1);
    for start in 0..40u32 {
        builder
            .add(candidate((
                "crates/a/src/a.rs",
                "true-to-false",
                (start, start + 4),
                b"true",
                b"false",
            )))
            .expect("valid");
    }
    let error = builder
        .build()
        .expect_err("forty ids cannot share sixteen one-character prefixes");
    match error {
        BuildError::DisplayCollision(DisplayCollisionError { length, collisions }) => {
            assert_eq!(length, 1);
            assert!(!collisions.is_empty());
            let shorts: Vec<&str> = collisions.iter().map(|c| c.display_id.as_str()).collect();
            let mut sorted = shorts.clone();
            sorted.sort_unstable();
            assert_eq!(shorts, sorted, "collisions are sorted by short form");
            for collision in &collisions {
                assert!(collision.ids.len() >= 2);
                assert!(
                    collision.ids.windows(2).all(|w| w[0] < w[1]),
                    "ids are sorted"
                );
            }
        }
        other => panic!("expected a display collision, got {other:?}"),
    }
    let out_of_range = Builder::new().with_display_length(0);
    assert_eq!(
        out_of_range.build().expect("empty").display_length(),
        20,
        "falls back to the default"
    );
}

#[test]
fn an_empty_catalog_has_a_digest_and_nothing_else() {
    let catalog = Builder::new().build().expect("empty");
    assert!(catalog.is_empty());
    assert_eq!(catalog.digest().len(), 64);
    assert_ne!(catalog.digest(), CATALOG_DIGEST_AB);
}

#[test]
fn resolve_prefix_refuses_to_guess() {
    // Spans 39 and 200 of this file share the first four hex digits of their
    // identities (found by search with the reference implementation).
    let first = candidate((
        "crates/a/src/a.rs",
        "true-to-false",
        (39, 43),
        b"true",
        b"false",
    ));
    let second = candidate((
        "crates/a/src/a.rs",
        "true-to-false",
        (200, 204),
        b"true",
        b"false",
    ));
    assert_eq!(&first.id().expect("valid")[..4], "6d62");
    assert_eq!(&second.id().expect("valid")[..4], "6d62");
    let mut builder = Builder::new();
    builder
        .add_all([first.clone(), second, b()])
        .expect("valid");
    let catalog = builder.build().expect("builds");

    assert_eq!(catalog.resolve_prefix(ID_B).expect("full id").id, ID_B);
    assert_eq!(
        catalog
            .resolve_prefix(&ID_B[..4])
            .expect("unique prefix")
            .id,
        ID_B
    );
    assert_eq!(
        catalog
            .resolve_prefix(&first.id().expect("valid")[..5])
            .expect("five characters disambiguate")
            .candidate,
        first
    );

    for bad in ["", "6d6", &format!("{ID_B}0"), "6D62", "zzzz"] {
        assert!(
            matches!(
                catalog.resolve_prefix(bad),
                Err(PrefixError::Invalid {
                    min: 4,
                    max: 64,
                    ..
                })
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        catalog.resolve_prefix("ffff"),
        Err(PrefixError::NotFound { .. })
    ));
    match catalog.resolve_prefix("6d62") {
        Err(PrefixError::Ambiguous { matches, .. }) => {
            assert_eq!(matches.len(), 2);
            assert!(matches.iter().all(|m| m.starts_with("6d62")));
        }
        other => panic!("expected an ambiguous prefix, got {other:?}"),
    }
}
