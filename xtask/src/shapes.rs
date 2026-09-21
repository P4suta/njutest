// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A second opinion about every arm the ledger still waives, read from the shape of its body and from nothing else.

use std::collections::BTreeMap;

use syn::visit::Visit;

/// What a catch-all arm's body does with everything the named arms did not take.
///
/// Read off the syntax, so it knows nothing about why the arm is there.
/// That is the point: a reading derived from the one that put the line in the ledger would agree with it for free, and agreement bought that way is worth nothing (ADR 0023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Shape {
    /// The rest is the answer: a literal, a unit, a `continue`, nothing at all.
    Selects,
    /// The rest gets no answer: the arm stops rather than replying on its behalf.
    Refuses,
    /// The rest is handed an answer that was written for something else.
    Decides,
}

/// One line of the ledger, as much as reading the syntax can say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waived {
    /// The enum the arms of that match name, which is how a waiver is held against what that enum promises.
    pub over: String,
    /// What the arm's body does with everything left.
    pub shape: Shape,
}

impl Shape {
    /// What this shape is worth saying to somebody deciding whether the line stays.
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Selects => {
                "by body shape: the body is the answer for the rest, so a new variant joining it is the reading you meant"
            }
            Self::Refuses => {
                "by body shape: the body stops rather than answering. Not a second opinion but a let-else nobody has written yet, and the ledger line goes with it"
            }
            Self::Decides => {
                "by body shape: the body is an answer written for something else, so a new variant would inherit it in silence"
            }
        }
    }

    /// The word on its own, for a line somebody scans down.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Selects => "selects",
            Self::Refuses => "refuses",
            Self::Decides => "decides",
        }
    }
}

/// What the body of one arm does, judged by its shape alone.
fn of(body: &syn::Expr, ours: &[String]) -> Shape {
    match body {
        syn::Expr::Macro(called) => stopping(&called.mac.path),
        syn::Expr::Lit(_) | syn::Expr::Continue(_) | syn::Expr::Break(_) => Shape::Selects,
        syn::Expr::Tuple(tuple) if tuple.elems.is_empty() => Shape::Selects,
        syn::Expr::Return(returned) => returned
            .expr
            .as_ref()
            .map_or(Shape::Selects, |inner| of(inner, ours)),
        syn::Expr::Path(path) => named(&path.path, ours),
        syn::Expr::Block(block) if block.block.stmts.is_empty() => Shape::Selects,
        syn::Expr::Block(block) => shaped(&block.block, ours),
        _ => Shape::Decides,
    }
}

/// Whether a macro call is one that stops rather than one that answers.
fn stopping(path: &syn::Path) -> Shape {
    let Some(last) = path.segments.last() else {
        return Shape::Decides;
    };
    let name = last.ident.to_string();
    if matches!(
        name.as_str(),
        "panic" | "unreachable" | "todo" | "unimplemented"
    ) {
        return Shape::Refuses;
    }
    Shape::Decides
}

/// Whether a path used as an answer names a value of a set this repository closes.
///
/// `None` and `Ordering::Equal` are the rest, said once.
/// A variant of one of our own enums is a verdict, and a verdict handed to everything nobody named is the arm somebody's next variant falls into.
fn named(path: &syn::Path, ours: &[String]) -> Shape {
    let head = path
        .segments
        .iter()
        .rev()
        .nth(1)
        .map(|segment| segment.ident.to_string());
    match head {
        Some(owner) if ours.contains(&owner) => Shape::Decides,
        _ => Shape::Selects,
    }
}

/// A block, judged by the one thing it ends with when that is all it does.
fn shaped(block: &syn::Block, ours: &[String]) -> Shape {
    let [syn::Stmt::Expr(only, _)] = block.stmts.as_slice() else {
        return Shape::Decides;
    };
    of(only, ours)
}

/// Every arm that catches everything left of a set this repository closes, by its line.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn shapes(source: &str, ours: &[String]) -> Result<BTreeMap<usize, Waived>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut found = BTreeMap::new();
    let mut scan = Shaping {
        ours,
        found: &mut found,
    };
    scan.visit_file(&parsed);
    Ok(found)
}

/// The walk that collects a shape for every arm the wildcard gate would name.
struct Shaping<'a> {
    ours: &'a [String],
    found: &'a mut BTreeMap<usize, Waived>,
}

impl<'ast> Visit<'ast> for Shaping<'_> {
    fn visit_expr_match(&mut self, matching: &'ast syn::ExprMatch) {
        let over = matching
            .arms
            .iter()
            .find_map(|arm| crate::lints::named_variant(&arm.pat))
            .filter(|name| self.ours.contains(name));
        if let Some(over) = over {
            for arm in &matching.arms {
                if let Some(span) = crate::lints::catches_everything(&arm.pat) {
                    self.found.insert(
                        span.start().line,
                        Waived {
                            over: over.clone(),
                            shape: of(&arm.body, self.ours),
                        },
                    );
                }
            }
        }
        syn::visit::visit_expr_match(self, matching);
    }
}
