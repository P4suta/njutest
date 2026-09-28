// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That an operator swap is the tree it names: every operand kept, grouped as it was, whatever operators surround it.
//!
//! The oracle here is the test's own: expressions are generated as trees, printed with the fewest parentheses their grouping needs (and some more), and the file each swap writes is read back into a tree to compare with the tree swapped by hand.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads the text it wrote at offsets it computed"
)]

use proptest::prelude::*;
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

/// A binary operator, loosest first within the order the Rust reference gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
enum Op {
    Or,
    And,
    Eq,
    Lt,
    BitOr,
    BitXor,
    BitAnd,
    Shl,
    Add,
    Mul,
}

impl Op {
    const fn text(self) -> &'static str {
        match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Lt => "<",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::BitAnd => "&",
            Self::Shl => "<<",
            Self::Add => "+",
            Self::Mul => "*",
        }
    }

    /// How tightly it binds, as the reference orders the levels.
    const fn level(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Eq | Self::Lt => 3,
            Self::BitOr => 4,
            Self::BitXor => 5,
            Self::BitAnd => 6,
            Self::Shl => 7,
            Self::Add => 8,
            Self::Mul => 9,
        }
    }

    /// What the operator table says a swap makes of it, and the rule that makes it.
    const fn swapped(self) -> (&'static str, Self) {
        match self {
            Self::Or => ("or-to-and", Self::And),
            Self::And => ("and-to-or", Self::Or),
            Self::Eq => ("eq-to-neq", Self::Eq),
            Self::Lt => ("lt-to-le", Self::Lt),
            Self::BitOr => ("bor-to-band", Self::BitAnd),
            Self::BitXor => ("xor-to-band", Self::BitAnd),
            Self::BitAnd => ("band-to-bor", Self::BitOr),
            Self::Shl => ("shl-to-shr", Self::Shl),
            Self::Add => ("add-to-sub", Self::Add),
            Self::Mul => ("mul-to-div", Self::Mul),
        }
    }

    fn of(op: &syn::BinOp) -> Self {
        match op {
            syn::BinOp::Or(_) => Self::Or,
            syn::BinOp::And(_) => Self::And,
            syn::BinOp::Eq(_) | syn::BinOp::Ne(_) => Self::Eq,
            syn::BinOp::Lt(_) | syn::BinOp::Le(_) => Self::Lt,
            syn::BinOp::BitOr(_) => Self::BitOr,
            syn::BinOp::BitXor(_) => Self::BitXor,
            syn::BinOp::BitAnd(_) => Self::BitAnd,
            syn::BinOp::Shl(_) | syn::BinOp::Shr(_) => Self::Shl,
            syn::BinOp::Add(_) | syn::BinOp::Sub(_) => Self::Add,
            syn::BinOp::Mul(_) | syn::BinOp::Div(_) => Self::Mul,
            other => panic!("the generator writes no {other:?}"),
        }
    }
}

/// An expression: a name, or two expressions joined by an operator.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tree {
    Leaf(usize),
    Node(Box<Node>),
}

/// Two expressions joined by an operator, perhaps parenthesized beyond what its grouping needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    op: Op,
    left: Tree,
    right: Tree,
    wrapped: bool,
}

fn node(op: Op, left: Tree, right: Tree, wrapped: bool) -> Tree {
    Tree::Node(Box::new(Node {
        op,
        left,
        right,
        wrapped,
    }))
}

fn tree() -> impl Strategy<Value = Tree> {
    (0_usize..6)
        .prop_map(Tree::Leaf)
        .prop_recursive(5, 32, 2, |inner| {
            (
                proptest::sample::select(Op::ALL.to_vec()),
                inner.clone(),
                inner,
                proptest::bool::weighted(0.2),
            )
                .prop_map(|(op, left, right, wrapped)| node(op, left, right, wrapped))
        })
}

/// Where one operator node sits in the printed text: its own bytes, and its operator's.
#[derive(Debug, Clone)]
struct Placed {
    node: std::ops::Range<usize>,
    op: std::ops::Range<usize>,
    path: Vec<bool>,
}

/// Whether `child`, written bare on one side of `op`, would be read as a different operand.
fn needs(child: &Tree, op: Op, right_side: bool) -> bool {
    match child {
        Tree::Node(inner) if !inner.wrapped => {
            inner.op.level() < op.level()
                || (inner.op.level() == op.level() && (right_side || op.level() == 3))
        }
        Tree::Node(_) | Tree::Leaf(_) => false,
    }
}

/// Prints `tree` with the parentheses its grouping needs and the ones it asks for, noting where every node and operator landed.
fn print(tree: &Tree, text: &mut String, path: &mut Vec<bool>, placed: &mut Vec<Placed>) {
    match tree {
        Tree::Leaf(name) => {
            text.push('v');
            text.push_str(&name.to_string());
        }
        Tree::Node(one) => {
            if one.wrapped {
                text.push('(');
            }
            let start = text.len();
            for (child, right_side) in [(&one.left, false), (&one.right, true)] {
                if right_side {
                    text.push(' ');
                    let op_start = text.len();
                    text.push_str(one.op.text());
                    placed.push(Placed {
                        node: start..start,
                        op: op_start..text.len(),
                        path: path.clone(),
                    });
                    text.push(' ');
                }
                let paren = needs(child, one.op, right_side);
                if paren {
                    text.push('(');
                }
                path.push(right_side);
                print(child, text, path, placed);
                path.pop();
                if paren {
                    text.push(')');
                }
            }
            if let Some(mine) = placed.iter_mut().rev().find(|placed| placed.path == *path) {
                mine.node = start..text.len();
            }
            if one.wrapped {
                text.push(')');
            }
        }
    }
}

/// The tree an expression holds, parentheses read as the grouping they are.
fn read(expr: &syn::Expr) -> Tree {
    match expr {
        syn::Expr::Paren(paren) => read(&paren.expr),
        syn::Expr::Binary(binary) => node(
            Op::of(&binary.op),
            read(&binary.left),
            read(&binary.right),
            false,
        ),
        syn::Expr::Path(path) => {
            let name = path.path.segments.last().expect("a name").ident.to_string();
            Tree::Leaf(
                name.trim_start_matches('v')
                    .parse::<usize>()
                    .expect("a generated name"),
            )
        }
        other => panic!("the generator writes no {other:?}"),
    }
}

/// `tree` with every extra parenthesis forgotten, which is all a reading can see.
fn bare(tree: &Tree) -> Tree {
    match tree {
        Tree::Leaf(name) => Tree::Leaf(*name),
        Tree::Node(one) => node(one.op, bare(&one.left), bare(&one.right), false),
    }
}

/// The operator of the node at `path`.
fn at(tree: &Tree, path: &[bool]) -> Op {
    match (tree, path.split_first()) {
        (Tree::Node(one), None) => one.op,
        (Tree::Node(one), Some((false, rest))) => at(&one.left, rest),
        (Tree::Node(one), Some((true, rest))) => at(&one.right, rest),
        (Tree::Leaf(_), _) => panic!("a path into a name"),
    }
}

/// `tree` with the node at `path` swapped as the table says.
fn swapped(tree: &Tree, path: &[bool]) -> Tree {
    match (tree, path.split_first()) {
        (Tree::Node(one), None) => node(
            one.op.swapped().1,
            one.left.clone(),
            one.right.clone(),
            one.wrapped,
        ),
        (Tree::Node(one), Some((false, rest))) => node(
            one.op,
            swapped(&one.left, rest),
            one.right.clone(),
            one.wrapped,
        ),
        (Tree::Node(one), Some((true, rest))) => node(
            one.op,
            one.left.clone(),
            swapped(&one.right, rest),
            one.wrapped,
        ),
        (Tree::Leaf(_), _) => panic!("a path into a name"),
    }
}

/// Where the generated expression stands: the text before it and after it, each an item among others that hold swaps of their own.
const PLACES: [(&str, &str); 5] = [
    ("fn f() { let probe = ", "; }\n"),
    (
        "struct S;\nimpl S {\n    fn g() { let _ = v0 && v1; }\n    fn f() { let probe = ",
        "; }\n    const K: u8 = 1 + 2;\n}\n",
    ),
    (
        "mod m {\n    fn g() { let _ = v0 || v1; }\n    mod n {\n        fn f() { let probe = ",
        "; }\n    }\n}\nfn h() { let _ = v2 & v3; }\n",
    ),
    ("trait T {\n    fn f() { let probe = ", "; }\n}\n"),
    (
        "const BEFORE: bool = v0 && v1;\nfn f() { let probe = ",
        "; }\nconst AFTER: bool = v2 || v3;\n",
    ),
];

/// The expression the one `let probe` in `file` is initialized with.
fn probe(file: &syn::File) -> syn::Expr {
    struct Find(Vec<syn::Expr>);
    impl<'a> syn::visit::Visit<'a> for Find {
        fn visit_local(&mut self, local: &'a syn::Local) {
            if let (syn::Pat::Ident(name), Some(init)) = (&local.pat, &local.init)
                && name.ident == "probe"
            {
                self.0.push((*init.expr).clone());
            }
            syn::visit::visit_local(self, local);
        }
    }
    let mut find = Find(Vec::new());
    syn::visit::Visit::visit_file(&mut find, file);
    let [expression] = find.0.as_slice() else {
        panic!(
            "the file holds one `let probe`, and it holds {}",
            find.0.len()
        )
    };
    expression.clone()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn every_operator_swap_keeps_the_operands_and_their_grouping(
        tree in tree(),
        place in proptest::sample::select(PLACES.to_vec()),
    ) {
        let mut expression = String::new();
        let mut placed = Vec::new();
        print(&tree, &mut expression, &mut Vec::new(), &mut placed);
        let (before, after) = place;
        let source = format!("{before}{expression}{after}");
        let registry = Registry::canonical();
        let discovered = discover_file("src/lib.rs", source.as_bytes(), &Selection::tier(&registry, Tier::All))
            .expect("the generated file parses");
        for node in &placed {
            let op_at = before.len() + node.op.start;
            let node_at = before.len() + node.node.start;
            let (rule, _) = at(&tree, &node.path).swapped();
            let op_span = (op_at, before.len() + node.op.end);
            let node_span = (node_at, before.len() + node.node.end);
            let found: Vec<_> = discovered
                .candidates
                .iter()
                .filter(|one| one.candidate.rule.name == rule)
                .filter(|one| {
                    let span = (
                        usize::try_from(one.candidate.span.start).expect("an offset"),
                        usize::try_from(one.candidate.span.end).expect("an offset"),
                    );
                    span == op_span || span == node_span
                })
                .collect();
            let declined = discovered.decisions.iter().any(|decision| {
                decision.rule == rule
                    && decision.skip.is_some()
                    && usize::try_from(decision.offset).is_ok_and(|offset| offset == op_at)
            });
            prop_assert!(
                found.len() == 1 && !declined,
                "an operand joined by operators is always one some writing keeps, parenthesized \
                 whole if nothing less does, so every operator a rule swaps is exactly one \
                 candidate and never declined: {rule} at {op_at} in {source}"
            );
            for one in found {
                let (start, end) = (
                    usize::try_from(one.candidate.span.start).expect("an offset"),
                    usize::try_from(one.candidate.span.end).expect("an offset"),
                );
                let written = format!(
                    "{}{}{}",
                    &source[..start],
                    String::from_utf8(one.candidate.replacement.clone()).expect("UTF-8"),
                    &source[end..]
                );
                let file: syn::File = syn::parse_str(&written).expect("a swap writes a file that parses");
                let read_back = read(&probe(&file));
                prop_assert_eq!(
                    read_back,
                    bare(&swapped(&tree, &node.path)),
                    "{} at {} must write the same operands grouped the same way: {} became {}",
                    rule, op_at, source.trim_end(), written.trim_end()
                );
            }
        }
    }
}

#[test]
fn swapping_the_outer_or_of_a_chain_keeps_its_left_operand_whole() {
    let source = "fn f() { let _ = a || b || c; }\n";
    let registry = Registry::canonical();
    let discovered = discover_file(
        "src/lib.rs",
        source.as_bytes(),
        &Selection::tier(&registry, Tier::All),
    )
    .expect("the file parses");
    let outer = source.rfind("||").expect("the outer operator");
    let written: Vec<String> = discovered
        .candidates
        .iter()
        .filter(|one| one.candidate.rule.name == "or-to-and")
        .filter(|one| {
            let start = usize::try_from(one.candidate.span.start).expect("an offset");
            let end = usize::try_from(one.candidate.span.end).expect("an offset");
            start <= outer && outer < end
        })
        .map(|one| {
            let start = usize::try_from(one.candidate.span.start).expect("an offset");
            let end = usize::try_from(one.candidate.span.end).expect("an offset");
            format!(
                "{}{}{}",
                &source[..start],
                String::from_utf8(one.candidate.replacement.clone()).expect("UTF-8"),
                &source[end..]
            )
        })
        .collect();
    assert!(
        written.iter().any(|text| text.contains("(a || b) && c")),
        "`a || b || c` is `(a || b) || c`, so swapping its outer operator makes `(a || b) && c`; \
         a token swap writes `a || b && c`, which reads as `a || (b && c)`, a different mutant \
         than the one it names: {written:?}"
    );
}

#[test]
fn a_planted_wrong_grouping_is_caught_by_the_tree_oracle() {
    let source = "fn f() { let probe = v0 || v1 || v2; }\n";
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let expected = node(
        Op::And,
        node(Op::Or, Tree::Leaf(0), Tree::Leaf(1), false),
        Tree::Leaf(2),
        false,
    );
    let outer = source.rfind("||").expect("the outer operator");
    let read_candidate = |discovered: rust_mutants::syntax::FileDiscovery| {
        let candidate = discovered
            .candidates
            .iter()
            .find(|one| {
                one.candidate.rule.name == "or-to-and"
                    && usize::try_from(one.candidate.span.start).is_ok_and(|start| start <= outer)
                    && usize::try_from(one.candidate.span.end).is_ok_and(|end| outer < end)
            })
            .expect("the outer operator has a candidate");
        let start = usize::try_from(candidate.candidate.span.start).expect("an offset");
        let end = usize::try_from(candidate.candidate.span.end).expect("an offset");
        let written = format!(
            "{}{}{}",
            &source[..start],
            String::from_utf8(candidate.candidate.replacement.clone()).expect("UTF-8"),
            &source[end..]
        );
        let file: syn::File = syn::parse_str(&written).expect("the swap writes Rust");
        read(&probe(&file))
    };
    let held = read_candidate(
        discover_file("src/lib.rs", source.as_bytes(), &selection).expect("the file parses"),
    );
    assert_eq!(held, expected);
    let planted = read_candidate(
        rust_mutants::testkit::source::discover_with_planted_grouping(source)
            .expect("the file parses"),
    );
    assert_eq!(
        planted,
        node(
            Op::Or,
            Tree::Leaf(0),
            node(Op::And, Tree::Leaf(1), Tree::Leaf(2), false),
            false,
        )
    );
    assert_ne!(planted, expected);
}

#[test]
fn a_swap_is_read_back_at_the_size_of_its_edit_not_of_its_item() {
    let statements = |count: usize| -> String {
        format!(
            "fn f(v0: bool, v1: bool) -> bool {{\n{}    v0\n}}\n",
            "    let _ = v0 || v1;\n".repeat(count)
        )
    };
    let read = |source: &str| {
        rust_mutants::testkit::source::read_back(source)
            .expect("the file parses")
            .expect("what one small file reads back fits")
    };
    let (half, whole) = (read(&statements(64)), read(&statements(128)));
    assert!(
        whole <= 2 * half,
        "twice the swaps in one item read back at most twice as much: each is held to the item \
         it stands in by reading its own edit again, not the whole item once per swap, which \
         grew with the square of the item: {half} bytes for 64 swaps, {whole} for 128"
    );
}

#[test]
fn a_swap_is_read_back_at_the_size_of_its_own_item_not_of_its_file() {
    let functions = |count: usize| -> String {
        (0..count)
            .map(|n| {
                format!("fn f{n:02}(v0: bool, v1: bool, v2: bool) -> bool {{ v0 || v1 && v2 }}\n")
            })
            .collect::<Vec<String>>()
            .concat()
    };
    let read = |source: &str| {
        rust_mutants::testkit::source::read_back(source)
            .expect("the file parses")
            .expect("what one small file reads back fits")
    };
    let one = read(&functions(1));
    assert!(
        one > 0,
        "a swap that changes how tightly its operator binds is read back"
    );
    assert_eq!(
        read(&functions(16)),
        16 * one,
        "sixteen functions read back sixteen times what one does: a swap is held to the item it \
         stands in, and a file that also holds fifteen others is no more to read for it"
    );
}

/// The binary operators of `file` in the order a walk meets them, each named, and the file with every operator made `+` and every parenthesis taken out, which is all two readings share when they differ by one operator.
fn shape(mut file: syn::File) -> (Vec<&'static str>, syn::File) {
    struct Flattened(Vec<&'static str>);
    impl syn::visit_mut::VisitMut for Flattened {
        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            while let syn::Expr::Paren(paren) = expr {
                let inner = (*paren.expr).clone();
                *expr = inner;
            }
            if let syn::Expr::Binary(binary) = expr {
                self.0.push(match binary.op {
                    syn::BinOp::Or(_) => "||",
                    syn::BinOp::And(_) => "&&",
                    syn::BinOp::BitOr(_) => "|",
                    syn::BinOp::BitXor(_) => "^",
                    syn::BinOp::BitAnd(_) => "&",
                    _ => "another",
                });
                binary.op = syn::BinOp::Add(syn::token::Plus::default());
            }
            syn::visit_mut::visit_expr_mut(self, expr);
        }
    }
    let mut flattened = Flattened(Vec::new());
    syn::visit_mut::VisitMut::visit_file_mut(&mut flattened, &mut file);
    (flattened.0, file)
}

#[test]
fn a_swap_where_tokens_touch_reads_back_as_the_swap_it_names() {
    let swaps = [
        ("or-to-and", "||", "&&"),
        ("and-to-or", "&&", "||"),
        ("bor-to-band", "|", "&"),
        ("xor-to-band", "^", "&"),
        ("band-to-bor", "&", "|"),
    ];
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let mut checked = 0;
    for source in [
        "fn f() { let probe = v0|&v1; }\n",
        "fn f() { let probe = v0|&&v1; }\n",
        "fn f() { let probe = v0||!v1&&v2; }\n",
        "fn f() { let probe = v0&&-v1||v2; }\n",
        "fn f() { let probe = v0|*v1^v2&v3; }\n",
        "fn f() { let probe = v0^&v1|v2; }\n",
        "fn f() { let probe = (v0||v1)&&(v2||v3); }\n",
        "fn f() { let probe = v0&&v1||v2&&v3; }\n",
        "fn f() { let probe = v0|v1&v2^v3; }\n",
        "fn f() { let probe = v0&!v1|v2; }\n",
        "fn f() { let probe = v0||v1|v2; }\n",
        "fn f() { let probe = v0?||v1?; }\n",
    ] {
        let original = shape(syn::parse_file(source).expect("the source parses"));
        let discovered = discover_file("src/lib.rs", source.as_bytes(), &selection)
            .expect("the source discovers");
        for found in &discovered.candidates {
            let Some((rule, from, to)) = swaps
                .iter()
                .find(|(rule, _, _)| *rule == found.candidate.rule.name)
            else {
                continue;
            };
            let (start, end) = (
                usize::try_from(found.candidate.span.start).expect("an offset"),
                usize::try_from(found.candidate.span.end).expect("an offset"),
            );
            let written = format!(
                "{}{}{}",
                &source[..start],
                String::from_utf8(found.candidate.replacement.clone()).expect("UTF-8"),
                &source[end..]
            );
            let read = shape(syn::parse_file(&written).unwrap_or_else(|error| {
                panic!("{rule} writes a file that parses: {written:?}: {error}")
            }));
            let changed: Vec<(&str, &str)> = original
                .0
                .iter()
                .zip(&read.0)
                .filter(|(was, now)| was != now)
                .map(|(was, now)| (*was, *now))
                .collect();
            assert!(
                read.1 == original.1 && changed == [(*from, *to)],
                "{rule} must write the same operands grouped the same way with the one operator \
                 swapped, however tightly the tokens around it sit: {source:?} became {written:?}"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 20,
        "the law reads back the swaps it was written for: {checked}"
    );
}
