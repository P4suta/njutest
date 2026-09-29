// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a claim names: which mutations its locator matches, how its line narrows them, and whether they come to the number it says.

/// What a mutation edits, as a locator reads it apart from where it sits: the file, the rule, the bytes it replaces, and the item it is in where one is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit<'a> {
    /// The workspace-relative path with forward slashes.
    pub path: &'a str,
    /// The rule's name.
    pub rule: &'a str,
    /// The bytes the edit replaces.
    pub original: &'a [u8],
    /// The item it is in, when one is known.
    pub item: Option<&'a str>,
}

/// What a locator says it names, apart from where: the file, the item as a reader writes it, the rule, and the text it replaces, which names any text where it is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted<'a> {
    /// The workspace-relative path with forward slashes.
    pub path: &'a str,
    /// The item, or a suffix of its path.
    pub item: &'a str,
    /// The rule's name.
    pub rule: &'a str,
    /// The bytes the edit replaces, as text.
    pub original: &'a str,
}

impl Wanted<'_> {
    /// Whether this names `edit`, wherever it sits: the same file and rule, the same text where it gives one, and an item whose path is the one it names.
    #[must_use]
    pub fn names(&self, edit: &Edit<'_>) -> bool {
        edit.path == self.path
            && edit.rule == self.rule
            && (self.original.is_empty() || edit.original == self.original.as_bytes())
            && edit.item.is_some_and(|item| names_item(item, self.item))
    }
}

/// Whether the item path `item` is the one a locator naming `wanted` names: itself, or a path whose last segments it is.
#[must_use]
pub fn names_item(item: &str, wanted: &str) -> bool {
    item == wanted
        || item
            .strip_suffix(wanted)
            .is_some_and(|head| head.ends_with("::"))
}

/// Whether a locator's line narrows the `matching` mutations its other fields name to the ones on that line: only where they are more than one, so a line one mutation left is a move rather than nothing.
#[must_use]
pub const fn narrows(matching: usize) -> bool {
    matching > 1
}

/// What the mutations a locator names come to, by how many are left once its line narrowed them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Located {
    /// It names them: the count it states, or exactly one where it states none.
    Named,
    /// It names none.
    Nothing,
    /// It states no count and names several, which its line did not separate.
    Several,
    /// It states a count, and names another number of them.
    Counted {
        /// How many it was written for.
        wanted: u32,
    },
}

/// What a locator that names `held` mutations comes to, where it states `count` of them.
#[must_use]
pub fn located(held: usize, count: Option<u32>) -> Located {
    match (held, count) {
        (0, _) => Located::Nothing,
        (held, Some(wanted)) if usize::try_from(wanted).is_ok_and(|wanted| held == wanted) => {
            Located::Named
        }
        (_, Some(wanted)) => Located::Counted { wanted },
        (1, None) => Located::Named,
        (_, None) => Located::Several,
    }
}

/// The line a claim holds and the line the first of what it names is on now, where it holds one and they differ.
#[must_use]
pub fn moved(line: Option<u32>, first: Option<u32>) -> Option<(u32, u32)> {
    line.zip(first).filter(|(from, to)| from != to)
}

#[cfg(test)]
mod tests;
