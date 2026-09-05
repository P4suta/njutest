// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The interval forest: the four relations two spans can stand in, and the
//! two traversal orders.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use proptest::prelude::*;
use rust_mutants::interval::{Conflict, Forest, Item, Node, Reason, build};
use rust_mutants::span::Span;

fn span(start: u32, end: u32) -> Span {
    Span::new(start, end).expect("well formed")
}

fn item(start: u32, end: u32, payload: &str) -> Item<String> {
    Item {
        span: span(start, end),
        payload: payload.to_owned(),
    }
}

fn node(start: u32, end: u32, alternatives: &[&str], children: Vec<Node<String>>) -> Node<String> {
    Node {
        span: span(start, end),
        alternatives: alternatives.iter().map(|s| (*s).to_owned()).collect(),
        children,
    }
}

#[test]
fn no_items_yield_an_empty_forest() {
    let (forest, conflicts) = build::<String>(Vec::new());
    assert!(forest.roots().is_empty());
    assert!(conflicts.is_empty());
    assert_eq!(forest, Forest::default());
}

#[test]
fn disjoint_spans_become_siblings_ordered_by_start_offset() {
    let (forest, conflicts) = build(vec![
        item(20, 25, "third"),
        item(0, 5, "first"),
        item(10, 15, "second"),
    ]);
    assert!(conflicts.is_empty());
    assert_eq!(
        forest.roots(),
        [
            node(0, 5, &["first"], vec![]),
            node(10, 15, &["second"], vec![]),
            node(20, 25, &["third"], vec![])
        ]
    );
}

#[test]
fn spans_that_merely_touch_are_disjoint_not_nested() {
    let (forest, _) = build(vec![item(0, 5, "left"), item(5, 9, "right")]);
    assert_eq!(
        forest.roots(),
        [
            node(0, 5, &["left"], vec![]),
            node(5, 9, &["right"], vec![])
        ]
    );
}

#[test]
fn identical_spans_become_alternatives_in_insertion_order() {
    let (forest, _) = build(vec![
        item(4, 9, "eq-to-neq"),
        item(20, 24, "true-to-false"),
        item(4, 9, "lt-to-le"),
        item(4, 9, "gt-to-ge"),
    ]);
    assert_eq!(
        forest.roots(),
        [
            node(4, 9, &["eq-to-neq", "lt-to-le", "gt-to-ge"], vec![]),
            node(20, 24, &["true-to-false"], vec![])
        ]
    );
}

#[test]
fn a_nested_span_attaches_to_the_smallest_enclosing_span() {
    // Deliberately scrambled: the innermost candidate is discovered first.
    let (forest, conflicts) = build(vec![
        item(4, 8, "inner"),
        item(22, 28, "tail"),
        item(2, 20, "mid"),
        item(0, 30, "outer"),
    ]);
    assert!(conflicts.is_empty());
    assert_eq!(
        forest.roots(),
        [node(
            0,
            30,
            &["outer"],
            vec![
                node(2, 20, &["mid"], vec![node(4, 8, &["inner"], vec![])]),
                node(22, 28, &["tail"], vec![])
            ]
        )]
    );
}

#[test]
fn nesting_that_shares_a_boundary_is_still_nesting() {
    let (forest, _) = build(vec![
        item(0, 10, "statement"),
        item(0, 4, "leading"),
        item(6, 10, "trailing"),
    ]);
    assert_eq!(
        forest.roots(),
        [node(
            0,
            10,
            &["statement"],
            vec![
                node(0, 4, &["leading"], vec![]),
                node(6, 10, &["trailing"], vec![])
            ]
        )]
    );
}

#[test]
fn alternatives_and_children_coexist_on_one_site() {
    let (forest, _) = build(vec![
        item(0, 10, "delete-statement"),
        item(2, 5, "add-to-sub"),
        item(0, 10, "delete-assignment"),
    ]);
    assert_eq!(
        forest.roots(),
        [node(
            0,
            10,
            &["delete-statement", "delete-assignment"],
            vec![node(2, 5, &["add-to-sub"], vec![])]
        )]
    );
}

#[test]
fn partial_overlap_evicts_the_later_span_whichever_order_they_arrive_in() {
    for order in [
        vec![item(0, 10, "left"), item(5, 15, "right")],
        vec![item(5, 15, "right"), item(0, 10, "left")],
    ] {
        let (forest, conflicts) = build(order);
        assert_eq!(forest.roots(), [node(0, 10, &["left"], vec![])]);
        assert_eq!(
            conflicts,
            [Conflict {
                item: item(5, 15, "right"),
                reason: Reason::PartialOverlap,
                against: span(0, 10)
            }]
        );
    }
    assert_eq!(Reason::PartialOverlap.to_string(), "partial-overlap");
}

#[test]
fn an_evicted_span_leaves_the_forest_open_for_the_spans_it_would_have_enclosed() {
    // "right" straddles "left" and is evicted; "tail" sits inside where "right"
    // would have been and still finds its place as a root.
    let (forest, conflicts) = build(vec![
        item(0, 10, "left"),
        item(5, 15, "right"),
        item(11, 14, "tail"),
    ]);
    assert_eq!(
        forest.roots(),
        [
            node(0, 10, &["left"], vec![]),
            node(11, 14, &["tail"], vec![])
        ]
    );
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn an_empty_span_cannot_be_placed_and_says_so() {
    let (forest, conflicts) = build(vec![item(3, 5, "site"), item(3, 3, "insertion")]);
    assert_eq!(forest.roots(), [node(3, 5, &["site"], vec![])]);
    assert_eq!(
        conflicts,
        [Conflict {
            item: item(3, 3, "insertion"),
            reason: Reason::EmptySpan,
            against: Span::default()
        }]
    );
    assert_eq!(Reason::EmptySpan.to_string(), "empty-span");
}

#[test]
fn inner_first_visits_children_before_parents_and_walk_the_reverse() {
    let (forest, _) = build(vec![
        item(0, 30, "outer"),
        item(2, 20, "mid"),
        item(4, 8, "inner"),
        item(22, 28, "tail"),
        item(40, 50, "second-root"),
    ]);
    let mut inner_first = Vec::new();
    forest.inner_first(|node| inner_first.push(node.alternatives[0].clone()));
    assert_eq!(
        inner_first,
        ["inner", "mid", "tail", "outer", "second-root"]
    );
    let mut walk = Vec::new();
    forest.walk(|node| walk.push(node.alternatives[0].clone()));
    assert_eq!(walk, ["outer", "mid", "inner", "tail", "second-root"]);
}

fn check_invariants(nodes: &[Node<u32>], enclosing: Option<Span>) {
    for (index, node) in nodes.iter().enumerate() {
        assert!(!node.span.is_empty());
        assert!(!node.alternatives.is_empty());
        if let Some(parent) = enclosing {
            assert!(
                parent.strictly_contains(node.span),
                "{parent} must strictly contain {}",
                node.span
            );
        }
        if let Some(next) = nodes.get(index + 1) {
            assert!(
                node.span.end <= next.span.start,
                "siblings are disjoint and sorted: {} then {}",
                node.span,
                next.span
            );
        }
        check_invariants(&node.children, Some(node.span));
    }
}

fn count(nodes: &[Node<u32>]) -> usize {
    nodes
        .iter()
        .map(|n| n.alternatives.len() + count(&n.children))
        .sum()
}

proptest! {
    #[test]
    fn every_item_lands_in_the_forest_or_in_the_conflicts_and_the_forest_is_well_formed(
        raw in proptest::collection::vec((0u32..40, 0u32..12), 0..20)
    ) {
        let items: Vec<Item<u32>> = raw.iter().enumerate().map(|(i, (start, len))| Item {
            span: span(*start, start + len),
            payload: u32::try_from(i).expect("small"),
        }).collect();
        let (forest, conflicts) = build(items.clone());
        check_invariants(forest.roots(), None);
        prop_assert_eq!(count(forest.roots()) + conflicts.len(), items.len());
        for conflict in &conflicts {
            match conflict.reason {
                Reason::EmptySpan => prop_assert!(conflict.item.span.is_empty()),
                Reason::PartialOverlap => {
                    prop_assert!(conflict.item.span.overlaps(conflict.against));
                    prop_assert!(!conflict.against.contains(conflict.item.span));
                    prop_assert!(!conflict.item.span.contains(conflict.against));
                }
                _ => prop_assert!(false, "unexpected reason"),
            }
        }
        // The forest is a function of the multiset, not of the order.
        let mut reversed = items;
        reversed.reverse();
        let (again, _) = build(reversed);
        let mut spans = Vec::new();
        forest.walk(|n| spans.push(n.span));
        let mut spans_again = Vec::new();
        again.walk(|n| spans_again.push(n.span));
        prop_assert_eq!(spans, spans_again);
    }
}
