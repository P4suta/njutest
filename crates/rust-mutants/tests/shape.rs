// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That instrumenting a file keeps its tree: every guard stands where its site stood, so taking the guards back out gives the file that was written.
//!
//! The oracle is the test's own reading of the shapes a guard takes, not the engine's offsets: it parses the instrumented file, takes every guard back to the original it holds, drops what the engine adds around them, and compares the tree with the one the source holds.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use rust_mutants::catalog::Builder;
use rust_mutants::instrument::{
    Instrumenting, MODULE_STEM, instrument_file, module_name, plan_file,
};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};
use syn::visit_mut::VisitMut;
use syn::{Expr, Stmt};

/// The runtime function `path` names, where it names one.
fn runtime(path: &syn::Path) -> Option<String> {
    let names: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match names.as_slice() {
        [.., module, name] if module == MODULE_STEM => Some(name.clone()),
        _ => None,
    }
}

/// The runtime function `expr` calls, with its arguments.
fn called(expr: &Expr) -> Option<(String, Vec<Expr>)> {
    let Expr::Call(call) = expr else { return None };
    let Expr::Path(function) = &*call.func else {
        return None;
    };
    Some((
        runtime(&function.path)?,
        call.args.iter().cloned().collect(),
    ))
}

/// Whether `expr` asks the runtime whether a mutant is live, or says one is not.
fn asks(expr: &Expr) -> bool {
    match expr {
        Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Not(_),
            expr,
            ..
        }) => asks(expr),
        other => matches!(called(other), Some((name, _)) if name == "active"),
    }
}

/// The conjunct a chain of `&&` ends in, and the first it starts with.
fn ends(expr: &Expr) -> (&Expr, &Expr) {
    match expr {
        Expr::Binary(syn::ExprBinary {
            op: syn::BinOp::And(_),
            left,
            right,
            ..
        }) => (ends(left).0, right),
        other => (other, other),
    }
}

/// The original a selector keeps: the last conjunct of its last disjunct, which is the one every `!active` before it lets through.
fn selected(expr: &Expr) -> Option<Expr> {
    let Expr::Binary(syn::ExprBinary {
        op: syn::BinOp::Or(_),
        right,
        ..
    }) = expr
    else {
        return None;
    };
    let (first, last) = ends(right);
    match first {
        Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Not(_),
            ..
        }) if asks(first) => Some(last.clone()),
        _ => None,
    }
}

/// The block a chain of `if active(..)` ends in, which holds the original.
fn kept(chain: &syn::ExprIf) -> Option<&syn::Block> {
    match chain.else_branch.as_ref().map(|(_, branch)| &**branch) {
        Some(Expr::If(next)) if asks(&next.cond) => kept(next),
        Some(Expr::Block(block)) => Some(&block.block),
        _ => None,
    }
}

/// `block` as the one expression it holds, or as a block.
fn as_expression(block: &syn::Block) -> Expr {
    match block.stmts.as_slice() {
        [Stmt::Expr(expr, None)] => expr.clone(),
        _ => Expr::Block(syn::ExprBlock {
            attrs: Vec::new(),
            label: None,
            block: block.clone(),
        }),
    }
}

/// What the identity macro holds, which the compiler reads as an expression once it expands it.
fn held(invocation: &syn::Macro) -> Expr {
    invocation.parse_body::<Expr>().unwrap_or_else(|error| {
        panic!(
            "what the identity macro holds has to read as an expression, as the compiler reads it: {error}: {}",
            invocation.tokens
        )
    })
}

/// The original a guard holds, where `expr` is a guard.
fn original(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::Macro(invocation) if runtime(&invocation.mac.path).as_deref() == Some("value") => {
            Some(held(&invocation.mac))
        }
        Expr::If(chain) if asks(&chain.cond) => kept(chain).map(as_expression),
        Expr::Binary(_) => selected(expr),
        Expr::Paren(paren) => Some((*paren.expr).clone()),
        other => match called(other) {
            Some((name, arguments)) if name != "active" && arguments.len() >= 2 => {
                arguments.get(1).cloned()
            }
            _ => None,
        },
    }
}

/// Whether `stmt` is a call the engine adds on its own: an item's entry, a checkpoint.
fn added(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(expr, Some(_)) => {
            matches!(called(expr), Some((name, _)) if name == "item" || name == "checkpoint" || name == "body")
        }
        _ => false,
    }
}

/// Takes every guard back to the original it holds, and every call the engine added back out.
struct Undo;

impl VisitMut for Undo {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items
            .retain(|item| !matches!(item, syn::Item::Mod(module) if module.ident == MODULE_STEM));
        syn::visit_mut::visit_file_mut(self, file);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in block.stmts.drain(..) {
            match stmt {
                added_by_the_engine if added(&added_by_the_engine) => {}
                Stmt::Expr(Expr::If(chain), None) if asks(&chain.cond) => {
                    stmts.extend(
                        kept(&chain)
                            .expect("a guard chain keeps its original")
                            .stmts
                            .clone(),
                    );
                }
                Stmt::Macro(invocation)
                    if runtime(&invocation.mac.path).as_deref() == Some("value") =>
                {
                    stmts.push(Stmt::Expr(held(&invocation.mac), invocation.semi_token));
                }
                other => stmts.push(other),
            }
        }
        block.stmts = stmts;
        syn::visit_mut::visit_block_mut(self, block);
    }

    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        while let Some(held) = original(expr) {
            *expr = held;
        }
        syn::visit_mut::visit_expr_mut(self, expr);
        if let Expr::Closure(closure) = expr
            && let Expr::Block(body) = &*closure.body
            && body.label.is_none()
            && let [Stmt::Expr(only, None)] = body.block.stmts.as_slice()
        {
            let only = only.clone();
            *closure.body = only;
        }
    }

    fn visit_pat_mut(&mut self, pat: &mut syn::Pat) {
        syn::visit_mut::visit_pat_mut(self, pat);
        if let syn::Pat::Guard(guarded) = pat
            && matches!(&*guarded.guard, Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(kept), .. }) if kept.value)
        {
            *pat = (*guarded.pat).clone();
        }
    }
}

/// The tree `source` holds, read the way [`Undo`] leaves an instrumented one: no parentheses, and a closure's lone-expression block as that expression.
fn tree(file: syn::File) -> syn::File {
    let mut file = file;
    Undo.visit_file_mut(&mut file);
    file
}

/// Whether an expression begins with a block-like one, or is one, which decides where it may stand without parentheses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lead {
    Plain,
    Whole,
    Leading,
}

/// Where an expression stands, and what it may begin with there.
fn standalone((text, lead): (String, Lead)) -> String {
    match lead {
        Lead::Leading => format!("({text})"),
        Lead::Plain | Lead::Whole => text,
    }
}

fn expression() -> impl Strategy<Value = (String, Lead)> {
    let names = prop_oneof![Just("a"), Just("b"), Just("1"), Just("true")]
        .prop_map(|name| (name.to_owned(), Lead::Plain));
    names.prop_recursive(4, 32, 3, |inner| {
        let arm = prop_oneof![
            inner
                .clone()
                .prop_map(|body| format!("{},", standalone(body))),
            inner
                .clone()
                .prop_map(|body| format!("{{ {} }}", standalone(body))),
            (inner.clone(), inner.clone()).prop_map(|(then, otherwise)| {
                format!(
                    "if c {{ {} }} else {{ {} }}",
                    standalone(then),
                    standalone(otherwise)
                )
            }),
        ];
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|((left, lead), (right, _))| {
                let lead = if lead == Lead::Plain {
                    Lead::Plain
                } else {
                    Lead::Leading
                };
                (format!("{left} + {right}"), lead)
            }),
            (inner.clone(), inner.clone()).prop_map(|(left, right)| {
                (
                    format!("({} < {})", standalone(left), standalone(right)),
                    Lead::Plain,
                )
            }),
            inner
                .clone()
                .prop_map(|body| (format!("{{ {} }}", standalone(body)), Lead::Whole)),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(
                |(condition, then, otherwise)| {
                    (
                        format!(
                            "if {} {{ {} }} else {{ {} }}",
                            condition_of(condition),
                            standalone(then),
                            standalone(otherwise)
                        ),
                        Lead::Whole,
                    )
                }
            ),
            (inner.clone(), arm.clone(), arm).prop_map(|(scrutinee, first, rest)| {
                (
                    format!(
                        "match {} {{ 0 => {first} _ => {rest} }}",
                        condition_of(scrutinee)
                    ),
                    Lead::Whole,
                )
            }),
            inner
                .clone()
                .prop_map(|(argument, _)| (format!("f({argument})"), Lead::Plain)),
            (inner.clone(), inner).prop_map(|((bound, _), body)| {
                (
                    format!("{{ let x = {bound}; {} }}", standalone(body)),
                    Lead::Whole,
                )
            }),
        ]
    })
}

/// An expression where a condition or a scrutinee stands, which a block may not begin.
fn condition_of((text, lead): (String, Lead)) -> String {
    match lead {
        Lead::Plain => text,
        Lead::Whole | Lead::Leading => format!("({text})"),
    }
}

fn statement() -> impl Strategy<Value = String> {
    prop_oneof![
        expression().prop_map(|(bound, _)| format!("let x = {bound};")),
        expression().prop_map(|(added, _)| format!("x += {added};")),
        expression().prop_map(|body| format!("let g = |y| {};", standalone(body))),
        expression().prop_map(|said| format!("{};", standalone(said))),
        expression().prop_map(|body| format!("{{ {} }}", standalone(body))),
        (expression(), expression()).prop_map(|(condition, body)| {
            let body = standalone(body);
            format!(
                "if {} {{ {body} }} else {{ {body} }}",
                condition_of(condition)
            )
        }),
    ]
}

fn function() -> impl Strategy<Value = String> {
    (proptest::collection::vec(statement(), 0..4), expression()).prop_map(|(statements, tail)| {
        format!(
            "pub fn f() -> i32 {{\n    {}\n    {}\n}}\n",
            statements.join("\n    "),
            standalone(tail)
        )
    })
}

/// Every mutation of `source` at every tier, instrumented into one file whose runtime module is named `__rm`.
fn instrumented(source: &str) -> String {
    let registry = Registry::canonical();
    let discovery = discover_file(
        "src/lib.rs",
        source.as_bytes(),
        &Selection::tier(&registry, Tier::All),
    )
    .expect("the generated source is discovered");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder
            .add(found.candidate.clone())
            .expect("a candidate the walk found joins the catalog");
    }
    let catalog = builder.build().expect("the catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("the plan");
    let comparable: BTreeSet<u32> = discovery
        .candidates
        .iter()
        .filter(|found| found.comparable.is_some())
        .map(|found| {
            found
                .candidate
                .id()
                .expect("a discovered candidate has an identity")
        })
        .filter_map(|id| catalog.by_id(id.as_str()))
        .map(|mutant| mutant.index)
        .collect();
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &comparable,
        probed: &BTreeMap::new(),
        catalog_digest: catalog.digest(),
        first_item: 0,
        watched: "/watched",
    })
    .unwrap_or_else(|error| panic!("the source is instrumented: {error}\n{source}"));
    file.text.replace(
        &module_name("src/lib.rs", source).expect("the generated source has valid tokens"),
        MODULE_STEM,
    )
}

/// Instruments `source` and says what taking every guard back out of it leaves, beside what the source holds.
fn undone(source: &str) -> (syn::File, syn::File, String) {
    let written = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("the generated source parses: {error}\n{source}"));
    let text = instrumented(source);
    let instrumented = syn::parse_file(&text)
        .unwrap_or_else(|error| panic!("the instrumented file parses: {error}\n{source}\n{text}"));
    (tree(written), tree(instrumented), text)
}

#[test]
fn a_block_arm_without_a_comma_keeps_standing_as_a_block() {
    let source = "pub enum Reach { Subtree, Exact }\n\npub fn reaches(reach: Reach, empty: bool) -> bool {\n    match reach {\n        Reach::Subtree => { true }\n        Reach::Exact => empty,\n    }\n}\n";
    let (written, instrumented, text) = undone(source);
    assert!(
        written == instrumented,
        "a guard over a block arm has to stand where a block stood, or the next arm is read as \
         part of it: {text}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn instrumenting_keeps_the_tree_of_every_file(source in function()) {
        let (written, instrumented, text) = undone(&source);
        prop_assert!(
            written == instrumented,
            "taking every guard back out has to give the tree that was written:\n{}\n{}",
            source,
            text
        );
    }
}
