// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Nested rewrite sites.

use std::fmt;

use crate::span::Span;

/// One candidate rewrite: the bytes it replaces, plus whatever the caller needs to recognise it again.
/// The payload is opaque here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<T> {
    /// The byte range this candidate rewrites.
    pub span: Span,
    /// Identifies the candidate to the caller.
    pub payload: T,
}

/// One rewrite site: a byte range plus every candidate that rewrites exactly that range, and the sites nested strictly inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<T> {
    /// The byte range this site rewrites.
    pub span: Span,
    /// The payload of every item with exactly this span, in the order the caller supplied them.
    /// They are mutually exclusive rewrites of the same bytes: the instrumenter emits them as one guard chain.
    pub alternatives: Vec<T>,
    /// The sites nested strictly inside this one, ordered by start offset, pairwise disjoint, each hanging off the smallest site that encloses it.
    pub children: Vec<Self>,
}

/// A set of rewrite sites arranged by containment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forest<T> {
    roots: Vec<Node<T>>,
}

impl<T> Default for Forest<T> {
    fn default() -> Self {
        Self { roots: Vec::new() }
    }
}

/// Why an item could not be placed in the forest.
/// Surfaced verbatim as a skip reason, beside the reasons discovery produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The item straddles the boundary of a site already in the forest.
    PartialOverlap,
    /// The item covers no bytes.
    /// An empty span is a legal catalog span, but the forest is precisely the structure that cannot hold one: `[3,3)` is at once enclosed by an open `[3,5)` and disjoint from it.
    EmptySpan,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PartialOverlap => "partial-overlap",
            Self::EmptySpan => "empty-span",
        })
    }
}

/// An item [`build`] refused to place, together with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict<T> {
    /// The evicted candidate, exactly as supplied.
    pub item: Item<T>,
    /// Why it was evicted.
    pub reason: Reason,
    /// The innermost forest span the item partially overlaps; the default span when the reason is not a partial overlap.
    pub against: Span,
}

/// Arranges items into a forest of nested rewrite sites and returns the items it could not place.
#[must_use]
pub fn build<T>(items: Vec<Item<T>>) -> (Forest<T>, Vec<Conflict<T>>) {
    let mut order: Vec<(Span, usize)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.span, index))
        .collect();
    order.sort_by(|(a, x), (b, y)| a.start.cmp(&b.start).then(b.end.cmp(&a.end)).then(x.cmp(y)));
    let mut slots: Vec<Option<Item<T>>> = items.into_iter().map(Some).collect();

    let mut arena: Vec<Site<T>> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut conflicts = Vec::new();

    for (span, index) in order {
        let Some(item) = slots.get_mut(index).and_then(Option::take) else {
            continue;
        };
        if span.is_empty() {
            conflicts.push(Conflict {
                item,
                reason: Reason::EmptySpan,
                against: Span::default(),
            });
            continue;
        }
        while let Some(&top) = stack.last()
            && arena
                .get(top)
                .is_some_and(|site| site.span.end <= span.start)
        {
            stack.pop();
        }
        if let Some(&top) = stack.last()
            && let Some(enclosing) = arena.get_mut(top)
        {
            if span.end > enclosing.span.end {
                conflicts.push(Conflict {
                    item,
                    reason: Reason::PartialOverlap,
                    against: enclosing.span,
                });
                continue;
            }
            if enclosing.span == span {
                enclosing.alternatives.push(item.payload);
                continue;
            }
        }
        let id = arena.len();
        arena.push(Site {
            span,
            alternatives: vec![item.payload],
            children: Vec::new(),
        });
        match stack.last() {
            None => roots.push(id),
            Some(&parent) => {
                if let Some(site) = arena.get_mut(parent) {
                    site.children.push(id);
                }
            }
        }
        stack.push(id);
    }

    (
        Forest {
            roots: assemble(arena, &roots),
        },
        conflicts,
    )
}

struct Site<T> {
    span: Span,
    alternatives: Vec<T>,
    children: Vec<usize>,
}

/// Turns the arena into owned nodes.
/// Children have greater ids than their parents, so popping from the end builds every child before its parent.
fn assemble<T>(mut arena: Vec<Site<T>>, roots: &[usize]) -> Vec<Node<T>> {
    let mut built: Vec<Option<Node<T>>> = arena.iter().map(|_| None).collect();
    while let Some(site) = arena.pop() {
        let id = arena.len();
        let children = site
            .children
            .iter()
            .filter_map(|child| built.get_mut(*child).and_then(Option::take))
            .collect();
        if let Some(slot) = built.get_mut(id) {
            *slot = Some(Node {
                span: site.span,
                alternatives: site.alternatives,
                children,
            });
        }
    }
    roots
        .iter()
        .filter_map(|root| built.get_mut(*root).and_then(Option::take))
        .collect()
}

impl<T> Forest<T> {
    /// The outermost sites, ordered by start offset and pairwise disjoint.
    #[must_use]
    pub fn roots(&self) -> &[Node<T>] {
        &self.roots
    }

    /// Visits every node children before parents, left to right within each level: the order the splicer composes in, since a site's replacement is built from its own bytes with each child's already-rendered text substituted in.
    pub fn inner_first(&self, mut visit: impl FnMut(&Node<T>)) {
        for root in &self.roots {
            let mut frames: Vec<(&Node<T>, usize)> = vec![(root, 0)];
            while let Some((node, next)) = frames.pop() {
                if let Some(child) = node.children.get(next) {
                    frames.push((node, next.saturating_add(1)));
                    frames.push((child, 0));
                } else {
                    visit(node);
                }
            }
        }
    }

    /// Visits every node parents before children, left to right: the order an indented dump wants.
    pub fn walk(&self, mut visit: impl FnMut(&Node<T>)) {
        let mut pending: Vec<&Node<T>> = self.roots.iter().rev().collect();
        while let Some(node) = pending.pop() {
            visit(node);
            pending.extend(node.children.iter().rev());
        }
    }
}
