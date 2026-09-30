// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Composing guard forms from a site's alternatives, each offset read where its text is written.

use rust_mutants_decision::shape::opens_a_block;

/// One guard shape the instrumenter composes a dormant mutant from.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
pub enum Form {
    /// The boolean selector, for a syntactically boolean position.
    C,
    /// The expression selector, for any value position.
    E,
    /// The statement guard.
    S,
    /// The guard written onto a match arm that had none, which is the one shape that adds syntax rather than replacing it.
    M,
    /// The compile-time expression selector, whose single active mutant is baked into a separate build.
    B,
}

impl Form {
    /// The letter.
    #[must_use]
    pub const fn letter(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::E => "E",
            Self::S => "S",
            Self::M => "M",
            Self::B => "B",
        }
    }
}

impl std::fmt::Display for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.letter())
    }
}

/// The path one of the runtime's functions is called by from a site `super_depth` inline modules down.
#[must_use]
pub fn named(module: &str, super_depth: u32, function: &str) -> String {
    let mut path = String::new();
    for _ in 0..super_depth {
        path.push_str("super::");
    }
    path.push_str(module);
    path.push_str("::");
    path.push_str(function);
    path
}

/// One alternative at a site: which mutant it is, what it reads, and what the compiler vouched a run may ask about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternative {
    /// The mutant's dense catalog index.
    pub index: u32,
    /// The text the guard writes when this mutant is the live one.
    pub text: String,
    /// Whether a run may evaluate this beside the original and record whether the two ever differed.
    pub comparable: bool,
    /// The runtime function that asks the question the compiler vouched a run may ask about the value this replaces, where it vouched one.
    pub probe: Option<&'static str>,
}

/// A composed guard: its text, where each alternative's own text sits in it, and where the original branch does, each offset from the start of the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// The guard.
    pub text: String,
    /// One entry per alternative, in the order given: the mutant index and the byte range its text occupies.
    pub alternatives: Vec<(u32, std::ops::Range<usize>)>,
    /// Where the original branch's text starts.
    pub original_at: usize,
    /// Every mutant this guard evaluates beside what it replaces, ascending; a form that cannot compare reports none, whatever it was offered.
    pub compared: Vec<u32>,
}

/// How a site reaches the runtime module its guards call into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paths<'a> {
    /// The module the runtime lives in, at the top level of the file.
    pub module: &'a str,
    /// How many `super::` segments separate the site's inline module from it.
    pub depth: u32,
}

impl Paths<'_> {
    /// The path one of the runtime's functions is called by from this site.
    #[must_use]
    pub fn of(self, function: &str) -> String {
        named(self.module, self.depth, function)
    }
}

/// Composes one guard of `form` from its alternatives, in catalog order, and the original branch.
#[must_use]
pub fn compose(
    form: Form,
    paths: &Paths<'_>,
    alternatives: &[Alternative],
    original: &str,
) -> Composed {
    match form {
        Form::C => selector(paths, alternatives, original, String::new()),
        Form::E if opens_a_block(original) => chain(
            paths,
            alternatives,
            original,
            (Holds::Values, String::new()),
        ),
        Form::E => {
            let opening = format!("{}!(", paths.of("value"));
            let mut composed = chain(paths, alternatives, original, (Holds::Values, opening));
            composed.text.push(')');
            composed
        }
        Form::S => chain(
            paths,
            alternatives,
            original,
            (Holds::Statements, String::new()),
        ),
        Form::M => arm(paths, alternatives, original),
        Form::B => compiled(paths, alternatives, original),
    }
}

/// Form B: a constant selector whose branches retain their coercion site and never call a runtime probe.
fn compiled(paths: &Paths<'_>, alternatives: &[Alternative], original: &str) -> Composed {
    let mut text = String::new();
    let grouped = !alternatives.is_empty() && !opens_a_block(original);
    if grouped {
        put(&mut text, &[&paths.of("value"), "!("]);
    }
    let mut spans = Vec::with_capacity(alternatives.len());
    for (at, one) in alternatives.iter().enumerate() {
        put(
            &mut text,
            &[
                if at == 0 { "if " } else { " else if " },
                &paths.of("baked"),
                "(",
                &one.index.to_string(),
                ") { ",
            ],
        );
        spans.push((
            one.index,
            held(&mut text, &one.text, Some(&paths.of("value"))),
        ));
        text.push_str(" }");
    }
    if !alternatives.is_empty() {
        text.push_str(" else { ");
    }
    let original_at = held(&mut text, original, Some(&paths.of("value"))).start;
    if !alternatives.is_empty() {
        text.push_str(" }");
    }
    if grouped {
        text.push(')');
    }
    Composed {
        text,
        alternatives: spans,
        original_at,
        compared: Vec::new(),
    }
}

/// Form M: the guard an arm did not have, written after the pattern that did not need one, which stands where the original does.
fn arm(paths: &Paths<'_>, alternatives: &[Alternative], original: &str) -> Composed {
    Composed {
        original_at: 0,
        ..selector(paths, alternatives, "true", format!("{original} if "))
    }
}

/// Form C: a boolean selector with no block, so the site introduces no temporary scope of its own, written after `text`.
///
/// The outer generated macro invocation is load bearing: a nested Form C site sits inside its parent's `&&` chain, where `&&` binds tighter than the `||` this composes.
/// The original is held in one too, or in the comparison call that wraps it, for the same reason: `!active(k) && a || b` leaves `b` deciding while mutant `k` is live.
/// Unlike a generic function, the identity macro preserves the surrounding expression's coercion site.
fn selector(
    paths: &Paths<'_>,
    alternatives: &[Alternative],
    original: &str,
    mut text: String,
) -> Composed {
    let (active, value) = (paths.of("active"), paths.of("value"));
    put(&mut text, &[&value, "!("]);
    let mut spans = Vec::with_capacity(alternatives.len());
    for one in alternatives {
        put(
            &mut text,
            &[&active, "(", &one.index.to_string(), ") && ", &value, "!("],
        );
        let start = text.len();
        text.push_str(&one.text);
        spans.push((one.index, start..text.len()));
        text.push_str(") || ");
    }
    for one in alternatives {
        put(
            &mut text,
            &["!", &active, "(", &one.index.to_string(), ") && "],
        );
    }
    let comparable: Vec<&Alternative> = alternatives.iter().filter(|one| one.comparable).collect();
    let differing = paths.of("differing");
    for one in &comparable {
        put(
            &mut text,
            &[&differing, "(", &one.index.to_string(), ", ", &value, "!("],
        );
    }
    let held = comparable.is_empty();
    if held {
        put(&mut text, &[&value, "!("]);
    }
    let original_at = text.len();
    text.push_str(original);
    if held {
        text.push(')');
    }
    for one in comparable.iter().rev() {
        put(&mut text, &["), || ", &value, "!(", &one.text, "))"]);
    }
    text.push(')');
    Composed {
        text,
        alternatives: spans,
        original_at,
        compared: comparable.iter().map(|one| one.index).collect(),
    }
}

/// What the branches of a chain hold, which decides what may be written around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    /// Values: every vouched probe is written around the original, and a branch that opens a block is held in the identity macro, since it starts the block it is written into.
    Values,
    /// Statements, which neither a probe nor the identity macro can hold.
    Statements,
}

/// Forms E and S: a branch chain, written after `text`.
///
/// Both have the same branches; Form E is grouped by the generated identity macro because it stands where a value does, unless what it replaces opens a block, when the chain itself has to end where that did.
fn chain(
    paths: &Paths<'_>,
    alternatives: &[Alternative],
    original: &str,
    (holds, mut text): (Holds, String),
) -> Composed {
    if alternatives.is_empty() {
        let original_at = text.len();
        text.push_str(original);
        return Composed {
            text,
            alternatives: Vec::new(),
            original_at,
            compared: Vec::new(),
        };
    }
    let (active, value) = (paths.of("active"), paths.of("value"));
    let holding =
        |branch: &str| (holds == Holds::Values && opens_a_block(branch)).then_some(value.as_str());
    let probed: Vec<(u32, &str)> = match holds {
        Holds::Values => alternatives
            .iter()
            .filter_map(|one| one.probe.map(|runtime| (one.index, runtime)))
            .collect(),
        Holds::Statements => Vec::new(),
    };
    let mut spans = Vec::with_capacity(alternatives.len());
    for (at, one) in alternatives.iter().enumerate() {
        let keyword = if at == 0 { "if " } else { " else if " };
        put(
            &mut text,
            &[keyword, &active, "(", &one.index.to_string(), ") { "],
        );
        spans.push((one.index, held(&mut text, &one.text, holding(&one.text))));
        text.push_str(if one.text.is_empty() { "}" } else { " }" });
    }
    text.push_str(" else { ");
    for (index, runtime) in &probed {
        put(
            &mut text,
            &[&paths.of(runtime), "(", &index.to_string(), ", "],
        );
    }
    let around = if probed.is_empty() {
        holding(original)
    } else {
        None
    };
    let original_at = held(&mut text, original, around).start;
    for _ in &probed {
        text.push(')');
    }
    text.push_str(" }");
    Composed {
        text,
        alternatives: spans,
        original_at,
        compared: probed.iter().map(|(index, _)| *index).collect(),
    }
}

/// Writes every piece of `pieces` onto `text`, in order.
fn put(text: &mut String, pieces: &[&str]) {
    for piece in pieces {
        text.push_str(piece);
    }
}

/// Writes `branch` onto `text`, inside the identity macro `around` names where it names one, and returns where the branch itself landed.
fn held(text: &mut String, branch: &str, around: Option<&str>) -> std::ops::Range<usize> {
    if let Some(value) = around {
        put(text, &[value, "!("]);
    }
    let start = text.len();
    text.push_str(branch);
    let end = text.len();
    if around.is_some() {
        text.push(')');
    }
    start..end
}

#[cfg(test)]
mod tests;
