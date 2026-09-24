// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The operator table: families, rules, tiers, and the registry that fixes their order.

use std::fmt;

/// A profile level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Tier {
    /// The default profile: operators whose survivors almost always point at a real gap in the tests.
    Balanced,
    /// Adds operators that are valuable but noisier in code that manipulates bits for performance rather than for meaning.
    Strong,
    /// Adds statement deletion, the classic source of equivalent mutants in logging and metrics code.
    All,
}

impl Tier {
    /// The tier's canonical name, which is also its profile name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Strong => "strong",
            Self::All => "all",
        }
    }

    /// Whether a profile at `self` selects rules at `other`.
    #[must_use]
    pub fn includes(self, other: Self) -> bool {
        other <= self
    }

    /// The tier named `name`, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.name() == name)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An operator family: the unit of selection for `--operator`, and, through its position in the canonical table, the deduplication tiebreak — an earlier family is the more local edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum Family {
    /// `true` ↔ `false`.
    BooleanLiteral,
    /// Negating an `if`, `while`, or guard condition; removing a `!`.
    ConditionNegation,
    /// `&&` ↔ `||`.
    BooleanConnective,
    /// `==`, `!=`, `<`, `<=`, `>`, `>=`.
    Comparison,
    /// `a..b` ↔ `a..=b`.
    Range,
    /// `+`, `-`, `*`, `/`, `%`.
    Arithmetic,
    /// Replacing a returned value with a default.
    ReturnReplacement,
    /// `?` propagation.
    ErrorPropagation,
    /// Deleting a match arm, and removing the guard that narrows one.
    MatchArm,
    /// `break` ↔ `continue`.
    ControlFlow,
    /// Fixing an `if` condition at `true` or at `false`, so each branch is asked for on its own.
    ConditionRemoval,
    /// `&`, `|`, `^`, `<<`, `>>`.
    Bitwise,
    /// `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`.
    CompoundAssignment,
    /// A method whose name says the opposite of another the same receiver has: `is_some`, `is_ok`, `max`.
    MethodSwap,
    /// Deleting a statement, and the `else` a statement ends with.
    StatementDeletion,
    /// Moving an integer literal by one, and emptying a string.
    Literal,
    /// `saturating_add` ↔ `wrapping_add`, and its siblings: one operation, the other boundary.
    SaturatingArithmetic,
    /// Failing the call a `?` asks about, so the suite is asked whether it noticed (ADR 0032); never chosen by a tier.
    Fault,
    /// Stopping the process just after a call that writes, so the next run is asked to start over what it left (ADR 0035); never chosen by a tier.
    Durable,
}

/// What a rule changes: the program's text, or what the program is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perturbs {
    /// The program itself, which is what a mutation is.
    Program,
    /// What a call the program makes returns, which is what a fault is.
    Environment,
    /// Whether the process goes on after a call, which is what a crash is.
    Crash,
}

impl Family {
    /// What the family's rules perturb, which every question that differs between a fault and a mutation reads rather than naming the fault family again.
    #[must_use]
    pub const fn perturbs(self) -> Perturbs {
        match self {
            Self::Fault => Perturbs::Environment,
            Self::Durable => Perturbs::Crash,
            Self::BooleanLiteral
            | Self::ConditionNegation
            | Self::BooleanConnective
            | Self::Comparison
            | Self::Range
            | Self::Arithmetic
            | Self::ReturnReplacement
            | Self::ErrorPropagation
            | Self::MatchArm
            | Self::ControlFlow
            | Self::ConditionRemoval
            | Self::Bitwise
            | Self::CompoundAssignment
            | Self::MethodSwap
            | Self::StatementDeletion
            | Self::Literal
            | Self::SaturatingArithmetic => Perturbs::Program,
        }
    }

    /// Whether a proof read off the unperturbed program may remove a target from what could notice one of the family's rules: not for a fault, which changes where control goes past its site and so leaves such a proof without its premise (ADR 0032).
    #[must_use]
    pub const fn proofs_apply(self) -> bool {
        matches!(self.perturbs(), Perturbs::Program)
    }

    /// Whether a tier chooses the family's rules, which a fault's never are: it perturbs the program's environment rather than its text, and asks another question of the suite.
    #[must_use]
    pub const fn chosen_by_tiers(self) -> bool {
        matches!(self.perturbs(), Perturbs::Program)
    }

    /// Whether the family's guard is carried into every alternative of a site it nests in, so it can be active beside a mutation of that site (ADR 0032).
    #[must_use]
    pub const fn carried_beside(self) -> bool {
        matches!(self.perturbs(), Perturbs::Environment)
    }

    /// The family's canonical name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BooleanLiteral => "boolean-literal",
            Self::ConditionNegation => "condition-negation",
            Self::BooleanConnective => "boolean-connective",
            Self::Comparison => "comparison",
            Self::Range => "range",
            Self::Arithmetic => "arithmetic",
            Self::ReturnReplacement => "return-replacement",
            Self::ErrorPropagation => "error-propagation",
            Self::MatchArm => "match-arm",
            Self::ControlFlow => "control-flow",
            Self::ConditionRemoval => "condition-removal",
            Self::Bitwise => "bitwise",
            Self::CompoundAssignment => "compound-assignment",
            Self::MethodSwap => "method-swap",
            Self::StatementDeletion => "statement-deletion",
            Self::Literal => "literal",
            Self::SaturatingArithmetic => "saturating-arithmetic",
            Self::Fault => "fault",
            Self::Durable => "durable",
        }
    }

    /// The family named `name`, if any.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.name() == name)
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One mutation operator: metadata only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rule {
    /// The family the rule belongs to.
    pub family: Family,
    /// The rule's globally unique name, for example `eq-to-neq`.
    pub name: &'static str,
    /// The rule's behaviour version, starting at 1. It feeds the mutant ID.
    pub version: u32,
    /// The lowest profile that selects the rule; always the family's tier.
    pub tier: Tier,
}

/// The rules whose survivor is a statement about a path, rather than about a value nobody checked.
const UNEXECUTED_PATH_RULES: [&str; 3] = [
    "question-to-unwrap",
    "ignore-question-statement",
    "return-ok-default",
];

impl Rule {
    /// What chooses the rule: the tier it is in, or `named` for a rule no tier chooses and a run asks for by name.
    #[must_use]
    pub const fn chosen_by(&self) -> &'static str {
        if self.family.chosen_by_tiers() {
            self.tier.name()
        } else {
            "named"
        }
    }

    /// Whether a survivor of this rule says a path was never taken.
    #[must_use]
    pub fn survivor_names_an_unexecuted_path(&self) -> bool {
        UNEXECUTED_PATH_RULES.contains(&self.name)
    }
}

impl fmt::Display for Rule {
    /// `name@version`, as it appears in reports and console output.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// The counts of the canonical v1 table, asserted by the registry tests.
pub const CANONICAL_FAMILY_COUNT: usize = Family::ALL.len();
/// The number of rules in the canonical v1 table.
pub const CANONICAL_RULE_COUNT: usize = 76;

const fn v1(family: Family, name: &'static str, tier: Tier) -> Rule {
    Rule {
        family,
        name,
        version: 1,
        tier,
    }
}

/// The v1 operator table, in the exact order of `docs/engine/operators.md`.
pub const CANONICAL_TABLE: [Rule; CANONICAL_RULE_COUNT] = [
    v1(Family::BooleanLiteral, "true-to-false", Tier::Balanced),
    v1(Family::BooleanLiteral, "false-to-true", Tier::Balanced),
    v1(
        Family::ConditionNegation,
        "negate-condition",
        Tier::Balanced,
    ),
    v1(
        Family::ConditionNegation,
        "negate-loop-condition",
        Tier::Balanced,
    ),
    v1(Family::ConditionNegation, "remove-not", Tier::Balanced),
    v1(
        Family::ConditionNegation,
        "negate-bool-method",
        Tier::Balanced,
    ),
    v1(Family::BooleanConnective, "and-to-or", Tier::Balanced),
    v1(Family::BooleanConnective, "or-to-and", Tier::Balanced),
    v1(Family::Comparison, "eq-to-neq", Tier::Balanced),
    v1(Family::Comparison, "neq-to-eq", Tier::Balanced),
    v1(Family::Comparison, "lt-to-le", Tier::Balanced),
    v1(Family::Comparison, "le-to-lt", Tier::Balanced),
    v1(Family::Comparison, "gt-to-ge", Tier::Balanced),
    v1(Family::Comparison, "ge-to-gt", Tier::Balanced),
    v1(Family::Range, "range-to-inclusive", Tier::Balanced),
    v1(Family::Range, "inclusive-to-range", Tier::Balanced),
    v1(Family::Arithmetic, "add-to-sub", Tier::Balanced),
    v1(Family::Arithmetic, "sub-to-add", Tier::Balanced),
    v1(Family::Arithmetic, "mul-to-div", Tier::Balanced),
    v1(Family::Arithmetic, "div-to-mul", Tier::Balanced),
    v1(Family::Arithmetic, "rem-to-mul", Tier::Balanced),
    v1(Family::Arithmetic, "remove-unary-minus", Tier::Balanced),
    v1(Family::ReturnReplacement, "return-default", Tier::Balanced),
    v1(
        Family::ReturnReplacement,
        "return-ok-default",
        Tier::Balanced,
    ),
    v1(
        Family::ReturnReplacement,
        "return-some-default",
        Tier::Balanced,
    ),
    v1(Family::ReturnReplacement, "return-true", Tier::Balanced),
    v1(
        Family::ReturnReplacement,
        "return-err-default",
        Tier::Balanced,
    ),
    v1(
        Family::ErrorPropagation,
        "question-to-unwrap",
        Tier::Balanced,
    ),
    v1(
        Family::ErrorPropagation,
        "ignore-question-statement",
        Tier::Balanced,
    ),
    v1(Family::MatchArm, "delete-match-arm", Tier::Balanced),
    v1(Family::MatchArm, "remove-match-guard", Tier::Balanced),
    v1(Family::ControlFlow, "break-to-continue", Tier::Balanced),
    v1(Family::ControlFlow, "continue-to-break", Tier::Balanced),
    v1(
        Family::ConditionRemoval,
        "condition-to-true",
        Tier::Balanced,
    ),
    v1(
        Family::ConditionRemoval,
        "condition-to-false",
        Tier::Balanced,
    ),
    v1(Family::Bitwise, "band-to-bor", Tier::Strong),
    v1(Family::Bitwise, "bor-to-band", Tier::Strong),
    v1(Family::Bitwise, "xor-to-band", Tier::Strong),
    v1(Family::Bitwise, "shl-to-shr", Tier::Strong),
    v1(Family::Bitwise, "shr-to-shl", Tier::Strong),
    v1(
        Family::CompoundAssignment,
        "add-assign-to-sub-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "sub-assign-to-add-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "mul-assign-to-div-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "div-assign-to-mul-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "rem-assign-to-mul-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "band-assign-to-bor-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "bor-assign-to-band-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "xor-assign-to-band-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "shl-assign-to-shr-assign",
        Tier::Strong,
    ),
    v1(
        Family::CompoundAssignment,
        "shr-assign-to-shl-assign",
        Tier::Strong,
    ),
    v1(Family::MethodSwap, "is-some-to-is-none", Tier::Strong),
    v1(Family::MethodSwap, "is-none-to-is-some", Tier::Strong),
    v1(Family::MethodSwap, "is-ok-to-is-err", Tier::Strong),
    v1(Family::MethodSwap, "is-err-to-is-ok", Tier::Strong),
    v1(Family::MethodSwap, "max-to-min", Tier::Strong),
    v1(Family::MethodSwap, "min-to-max", Tier::Strong),
    v1(Family::MethodSwap, "all-to-any", Tier::Strong),
    v1(Family::MethodSwap, "any-to-all", Tier::Strong),
    v1(Family::MethodSwap, "first-to-last", Tier::Strong),
    v1(Family::MethodSwap, "last-to-first", Tier::Strong),
    v1(Family::MethodSwap, "skip-to-take", Tier::Strong),
    v1(Family::MethodSwap, "take-to-skip", Tier::Strong),
    v1(Family::MethodSwap, "sum-to-product", Tier::Strong),
    v1(Family::MethodSwap, "product-to-sum", Tier::Strong),
    v1(
        Family::StatementDeletion,
        "delete-call-statement",
        Tier::All,
    ),
    v1(Family::StatementDeletion, "delete-assignment", Tier::All),
    v1(
        Family::StatementDeletion,
        "delete-compound-assignment",
        Tier::All,
    ),
    v1(Family::StatementDeletion, "delete-else-branch", Tier::All),
    v1(Family::Literal, "int-increment", Tier::All),
    v1(Family::Literal, "int-decrement", Tier::All),
    v1(Family::Literal, "string-to-empty", Tier::All),
    v1(
        Family::SaturatingArithmetic,
        "saturating-add-to-wrapping-add",
        Tier::All,
    ),
    v1(
        Family::SaturatingArithmetic,
        "saturating-sub-to-wrapping-sub",
        Tier::All,
    ),
    v1(
        Family::SaturatingArithmetic,
        "saturating-mul-to-wrapping-mul",
        Tier::All,
    ),
    v1(Family::Fault, "inject-error", Tier::All),
    v1(Family::Durable, "crash-after-write", Tier::All),
];

/// Whether a rule name is well formed: non-empty, and free of whitespace and of the `@` that separates the version in the rendered form.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains([' ', '\t', '\r', '\n', '@'])
}

/// A rule or a table that breaks a registry invariant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RuleError {
    /// A rule name is empty or malformed.
    #[error("rule name {name:?} is invalid")]
    InvalidName {
        /// The offending name.
        name: String,
    },
    /// A rule version is below 1.
    #[error("rule {name} has version {version}; versions start at 1")]
    InvalidVersion {
        /// The rule.
        name: String,
        /// The offending version.
        version: u32,
    },
    /// Two rules share one name.
    #[error("duplicate rule name {name:?} at positions {first} and {second}")]
    DuplicateRule {
        /// The name.
        name: String,
        /// The first position.
        first: usize,
        /// The second position.
        second: usize,
    },
    /// A family's rules disagree on tier.
    #[error("family {family} has rules at tiers {first} and {second}")]
    FamilyTierConflict {
        /// The family.
        family: Family,
        /// The family's tier.
        first: Tier,
        /// The disagreeing tier.
        second: Tier,
    },
    /// The table's tiers decrease, so one profile's rules are not a prefix of the next one's.
    #[error("position {position} is at tier {second}, below the {first} above it")]
    TierOutOfOrder {
        /// Where the tier dropped.
        position: usize,
        /// The tier of the rule above.
        first: Tier,
        /// The lower tier.
        second: Tier,
    },
    /// A family's rules are not contiguous in table order.
    #[error("family {family} resumes at position {position}; families are contiguous")]
    FamilySplit {
        /// The family.
        family: Family,
        /// Where it resumed.
        position: usize,
    },
    /// A rule is not in the registry.
    #[error("unknown rule {name:?}")]
    UnknownRule {
        /// The name.
        name: String,
    },
    /// A rule's metadata disagrees with the registered rule of that name.
    #[error("rule {rule} is registered as {registered}")]
    Mismatch {
        /// The rule as given.
        rule: Rule,
        /// The registered rule.
        registered: Rule,
    },
}

/// An ordered, immutable set of rules.
/// Position in the registry is table order, family-major, and is the deterministic tiebreak the catalog uses when two families produce the same byte edit.
#[derive(Debug, Clone, Copy)]
pub struct Registry {
    rules: &'static [Rule],
}

impl Registry {
    /// The frozen v1 operator registry.
    #[must_use]
    pub const fn canonical() -> Self {
        Self {
            rules: &CANONICAL_TABLE,
        }
    }

    /// A registry from rules in table order, validating the invariants the catalog relies on: valid metadata, unique names, one tier per family, contiguous families, and tiers that never decrease.
    ///
    /// # Errors
    /// Returns the first invariant the table breaks.
    pub fn new(rules: &'static [Rule]) -> Result<Self, RuleError> {
        let registry = Self { rules };
        registry.validate()?;
        Ok(registry)
    }

    /// Whether the registry's table satisfies every invariant: valid metadata, unique names, one tier per family, contiguous families, tiers that never decrease.
    ///
    /// # Errors
    /// Returns the first invariant the table breaks.
    pub fn validate(&self) -> Result<(), RuleError> {
        for (index, rule) in self.rules.iter().enumerate() {
            if !valid_name(rule.name) {
                return Err(RuleError::InvalidName {
                    name: rule.name.to_owned(),
                });
            }
            if rule.version < 1 {
                return Err(RuleError::InvalidVersion {
                    name: rule.name.to_owned(),
                    version: rule.version,
                });
            }
            let earlier = self.rules.get(..index).unwrap_or_default();
            if let Some(previous) = earlier.last()
                && rule.tier < previous.tier
            {
                return Err(RuleError::TierOutOfOrder {
                    position: index,
                    first: previous.tier,
                    second: rule.tier,
                });
            }
            if let Some(first) = earlier.iter().position(|seen| seen.name == rule.name) {
                return Err(RuleError::DuplicateRule {
                    name: rule.name.to_owned(),
                    first,
                    second: index,
                });
            }
            if let Some(seen) = earlier.iter().find(|seen| seen.family == rule.family) {
                if seen.tier != rule.tier {
                    return Err(RuleError::FamilyTierConflict {
                        family: rule.family,
                        first: seen.tier,
                        second: rule.tier,
                    });
                }
                if !matches!(earlier.last(), Some(previous) if previous.family == rule.family) {
                    return Err(RuleError::FamilySplit {
                        family: rule.family,
                        position: index,
                    });
                }
            }
        }
        Ok(())
    }

    /// The number of registered rules.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the registry holds no rules.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Every rule in table order.
    #[must_use]
    pub const fn rules(&self) -> &'static [Rule] {
        self.rules
    }

    /// Every family in table order.
    #[must_use]
    pub fn families(&self) -> Vec<Family> {
        let mut families: Vec<Family> = Vec::new();
        for rule in self.rules {
            if !families.contains(&rule.family) {
                families.push(rule.family);
            }
        }
        families
    }

    /// The registered rule with the given name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<Rule> {
        self.rules.iter().copied().find(|rule| rule.name == name)
    }

    /// The rule's index in table order: the deduplication tiebreak.
    #[must_use]
    pub fn position(&self, name: &str) -> Option<usize> {
        self.rules.iter().position(|rule| rule.name == name)
    }

    /// The family's index in table order.
    #[must_use]
    pub fn family_position(&self, family: Family) -> Option<usize> {
        self.families().iter().position(|seen| *seen == family)
    }

    /// The rules of one family in table order.
    #[must_use]
    pub fn family_rules(&self, family: Family) -> Vec<Rule> {
        self.rules
            .iter()
            .copied()
            .filter(|rule| rule.family == family)
            .collect()
    }

    /// Every rule a profile at `tier` selects, in table order.
    #[must_use]
    pub fn select_tier(&self, tier: Tier) -> Vec<Rule> {
        self.rules
            .iter()
            .copied()
            .filter(|rule| tier.includes(rule.tier) && rule.family.chosen_by_tiers())
            .collect()
    }

    /// Whether `rule` is registered with exactly this metadata.
    /// A name match with a different version or family is an error, never a near miss.
    ///
    /// # Errors
    /// Returns [`RuleError::UnknownRule`] or [`RuleError::Mismatch`].
    pub fn verify(&self, rule: Rule) -> Result<(), RuleError> {
        let registered = self
            .lookup(rule.name)
            .ok_or_else(|| RuleError::UnknownRule {
                name: rule.name.to_owned(),
            })?;
        if registered != rule {
            return Err(RuleError::Mismatch { rule, registered });
        }
        Ok(())
    }
}
