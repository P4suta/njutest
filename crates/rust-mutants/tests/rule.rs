// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The operator table is a contract: its order is the deduplication
//! tiebreak, its names are parts of mutant identities, and its tiers nest.

#![expect(
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use rust_mutants::rule::{
    CANONICAL_FAMILY_COUNT, CANONICAL_RULE_COUNT, Family, Registry, Rule, RuleError, Tier,
};

#[test]
fn tiers_nest_monotonically_and_round_trip_through_their_names() {
    assert_eq!(Tier::ALL, [Tier::Balanced, Tier::Strong, Tier::All]);
    assert!(Tier::Balanced < Tier::Strong && Tier::Strong < Tier::All);
    for (outer, inner, want) in [
        (Tier::All, Tier::Balanced, true),
        (Tier::Strong, Tier::Balanced, true),
        (Tier::Balanced, Tier::Balanced, true),
        (Tier::Balanced, Tier::Strong, false),
        (Tier::Strong, Tier::All, false),
    ] {
        assert_eq!(outer.includes(inner), want, "{outer} includes {inner}");
    }
    for tier in Tier::ALL {
        assert_eq!(Tier::parse(tier.name()), Some(tier));
        assert_eq!(tier.to_string(), tier.name());
    }
    assert_eq!(Tier::parse("Balanced"), None);
    assert_eq!(Tier::parse("everything"), None);
}

#[test]
fn families_round_trip_through_their_names() {
    let registry = Registry::canonical();
    for family in registry.families() {
        assert_eq!(Family::parse(family.name()), Some(family));
        assert_eq!(family.to_string(), family.name());
    }
    assert_eq!(Family::parse("comparison"), Some(Family::Comparison));
    assert_eq!(
        Family::parse("float-arithmetic"),
        None,
        "type-directed families do not exist in Rust"
    );
}

#[test]
fn the_canonical_table_has_the_documented_shape() {
    let registry = Registry::canonical();
    registry
        .validate()
        .expect("the canonical table satisfies every registry invariant");
    assert_eq!(registry.len(), CANONICAL_RULE_COUNT);
    assert_eq!(CANONICAL_RULE_COUNT, 36);
    assert_eq!(registry.families().len(), CANONICAL_FAMILY_COUNT);
    assert_eq!(CANONICAL_FAMILY_COUNT, 11);
    let expected: Vec<(Family, Tier, Vec<&str>)> = vec![
        (
            Family::BooleanLiteral,
            Tier::Balanced,
            vec!["true-to-false", "false-to-true"],
        ),
        (
            Family::ConditionNegation,
            Tier::Balanced,
            vec!["negate-condition", "negate-loop-condition", "remove-not"],
        ),
        (
            Family::BooleanConnective,
            Tier::Balanced,
            vec!["and-to-or", "or-to-and"],
        ),
        (
            Family::Comparison,
            Tier::Balanced,
            vec![
                "eq-to-neq",
                "neq-to-eq",
                "lt-to-le",
                "le-to-lt",
                "gt-to-ge",
                "ge-to-gt",
            ],
        ),
        (
            Family::Range,
            Tier::Balanced,
            vec!["range-to-inclusive", "inclusive-to-range"],
        ),
        (
            Family::Arithmetic,
            Tier::Balanced,
            vec![
                "add-to-sub",
                "sub-to-add",
                "mul-to-div",
                "div-to-mul",
                "rem-to-mul",
            ],
        ),
        (
            Family::ReturnReplacement,
            Tier::Balanced,
            vec![
                "return-default",
                "return-ok-default",
                "return-some-default",
                "return-true",
            ],
        ),
        (
            Family::ErrorPropagation,
            Tier::Balanced,
            vec!["question-to-unwrap", "ignore-question-statement"],
        ),
        (
            Family::Bitwise,
            Tier::Strong,
            vec![
                "band-to-bor",
                "bor-to-band",
                "xor-to-band",
                "shl-to-shr",
                "shr-to-shl",
            ],
        ),
        (
            Family::CompoundAssignment,
            Tier::Strong,
            vec!["add-assign-to-sub-assign", "sub-assign-to-add-assign"],
        ),
        (
            Family::StatementDeletion,
            Tier::All,
            vec![
                "delete-call-statement",
                "delete-assignment",
                "delete-compound-assignment",
            ],
        ),
    ];
    let mut position = 0;
    for (index, (family, tier, names)) in expected.iter().enumerate() {
        assert_eq!(registry.family_position(*family), Some(index), "{family}");
        let rules = registry.family_rules(*family);
        assert_eq!(
            rules.iter().map(|r| r.name).collect::<Vec<_>>(),
            *names,
            "{family}"
        );
        for rule in rules {
            assert_eq!(rule.tier, *tier, "{rule}");
            assert_eq!(rule.version, 1, "every v1 rule is at version 1");
            assert_eq!(registry.position(rule.name), Some(position), "{rule}");
            assert_eq!(registry.lookup(rule.name), Some(rule));
            position += 1;
        }
    }
    assert_eq!(position, CANONICAL_RULE_COUNT);
}

#[test]
fn rule_names_are_unique_kebab_case_and_render_with_their_version() {
    let registry = Registry::canonical();
    let names: BTreeSet<&str> = registry.rules().iter().map(|r| r.name).collect();
    assert_eq!(names.len(), registry.len());
    for rule in registry.rules() {
        assert!(
            rule.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()),
            "{rule}"
        );
        assert!(
            !rule.name.starts_with('-') && !rule.name.ends_with('-'),
            "{rule}"
        );
    }
    assert_eq!(
        registry
            .lookup("eq-to-neq")
            .expect("registered")
            .to_string(),
        "eq-to-neq@1"
    );
}

#[test]
fn select_tier_returns_every_rule_at_or_below_the_tier_in_table_order() {
    let registry = Registry::canonical();
    let balanced = registry.select_tier(Tier::Balanced);
    let strong = registry.select_tier(Tier::Strong);
    let all = registry.select_tier(Tier::All);
    assert_eq!(balanced.len(), 26);
    assert_eq!(strong.len(), 33);
    assert_eq!(all.len(), 36);
    assert!(balanced.iter().all(|r| r.tier == Tier::Balanced));
    assert_eq!(
        &strong[..balanced.len()],
        &balanced[..],
        "balanced ⊂ strong, in table order"
    );
    assert_eq!(
        &all[..strong.len()],
        &strong[..],
        "strong ⊂ all, in table order"
    );
    assert_eq!(all, registry.rules().to_vec());
}

#[test]
fn verify_accepts_exactly_the_registered_metadata() {
    let registry = Registry::canonical();
    let registered = registry.lookup("lt-to-le").expect("registered");
    registry.verify(registered).expect("exact match");
    let bumped = Rule {
        version: 2,
        ..registered
    };
    assert!(matches!(
        registry.verify(bumped),
        Err(RuleError::Mismatch { .. })
    ));
    let moved = Rule {
        family: Family::Bitwise,
        ..registered
    };
    assert!(matches!(
        registry.verify(moved),
        Err(RuleError::Mismatch { .. })
    ));
    let unknown = Rule {
        name: "lt-to-gt",
        ..registered
    };
    assert!(matches!(
        registry.verify(unknown),
        Err(RuleError::UnknownRule { .. })
    ));
}

const fn rule(family: Family, name: &'static str, version: u32, tier: Tier) -> Rule {
    Rule {
        family,
        name,
        version,
        tier,
    }
}

const DUPLICATE: [Rule; 2] = [
    rule(Family::Comparison, "eq-to-neq", 1, Tier::Balanced),
    rule(Family::Comparison, "eq-to-neq", 1, Tier::Balanced),
];
const TIER_CONFLICT: [Rule; 2] = [
    rule(Family::Comparison, "eq-to-neq", 1, Tier::Balanced),
    rule(Family::Comparison, "neq-to-eq", 1, Tier::Strong),
];
const SPLIT: [Rule; 3] = [
    rule(Family::Comparison, "eq-to-neq", 1, Tier::Balanced),
    rule(Family::Bitwise, "band-to-bor", 1, Tier::Strong),
    rule(Family::Comparison, "neq-to-eq", 1, Tier::Balanced),
];
const ZERO_VERSION: [Rule; 1] = [rule(Family::Comparison, "eq-to-neq", 0, Tier::Balanced)];
const BAD_NAME: [Rule; 1] = [rule(Family::Comparison, "eq to neq@1", 1, Tier::Balanced)];
const FINE: [Rule; 2] = [
    rule(Family::Comparison, "eq-to-neq", 1, Tier::Balanced),
    rule(Family::Bitwise, "band-to-bor", 3, Tier::Strong),
];

#[test]
fn a_registry_refuses_a_table_that_breaks_an_invariant() {
    assert!(matches!(
        Registry::new(&DUPLICATE),
        Err(RuleError::DuplicateRule {
            first: 0,
            second: 1,
            ..
        })
    ));
    assert!(matches!(
        Registry::new(&TIER_CONFLICT),
        Err(RuleError::FamilyTierConflict {
            family: Family::Comparison,
            ..
        })
    ));
    assert!(matches!(
        Registry::new(&SPLIT),
        Err(RuleError::FamilySplit {
            family: Family::Comparison,
            position: 2
        })
    ));
    assert!(matches!(
        Registry::new(&ZERO_VERSION),
        Err(RuleError::InvalidVersion { version: 0, .. })
    ));
    assert!(matches!(
        Registry::new(&BAD_NAME),
        Err(RuleError::InvalidName { .. })
    ));
    let registry = Registry::new(&FINE).expect("consistent");
    assert_eq!(registry.families(), [Family::Comparison, Family::Bitwise]);
    assert_eq!(registry.select_tier(Tier::Balanced), [FINE[0]]);
}
