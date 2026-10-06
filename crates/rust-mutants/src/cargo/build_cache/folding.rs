// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A digest folded from fields that each name the input they describe, so two foldings that differ say which inputs differ.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

/// The input one field of a folded digest describes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cargo) enum Input {
    /// The name and version of the derivation itself.
    Derivation,
    /// The versions the toolchain reports.
    Versions,
    /// The arguments the compile starts with.
    Arguments,
    /// The root of the tree the compile reads.
    Root,
    /// A file the compile may read.
    File(PathBuf),
    /// A variable the compile may read, named and never spelled.
    Variable(OsString),
    /// A variable the platform's loader searches by, named and never spelled.
    SearchVariable(&'static str),
    /// A directory the loader searches, or an entry of one.
    Searched(PathBuf),
    /// What an augmented compile is for and the arguments it starts with.
    Augmentation,
}

impl fmt::Display for Input {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Derivation => formatter.write_str("the key's derivation"),
            Self::Versions => formatter.write_str("the toolchain's versions"),
            Self::Arguments => formatter.write_str("the compile's arguments"),
            Self::Root => formatter.write_str("the source root"),
            Self::File(path) => write!(formatter, "the file {}", path.display()),
            Self::Variable(name) => write!(formatter, "the variable {}", name.display()),
            Self::SearchVariable(name) => write!(formatter, "the loader search variable {name}"),
            Self::Searched(path) => {
                write!(formatter, "the loader search entry {}", path.display())
            }
            Self::Augmentation => formatter.write_str("what the compile is augmented for"),
        }
    }
}

/// What became of one input between two foldings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::cargo) enum Change {
    /// The input was not there before.
    Appeared(Input),
    /// The input is no longer there.
    Gone(Input),
    /// The input is there with other content.
    Changed(Input),
    /// Every input is what it was, folded in another order.
    Reordered,
}

impl fmt::Display for Change {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Appeared(input) => write!(formatter, "{input} appeared"),
            Self::Gone(input) => write!(formatter, "{input} is gone"),
            Self::Changed(input) => write!(formatter, "{input} changed"),
            Self::Reordered => formatter.write_str("the inputs were folded in another order"),
        }
    }
}

/// A digest being folded, with the hash of every field under the input it describes.
pub(in crate::cargo) struct Folding {
    digest: Sha256,
    fields: Vec<(Input, Vec<u8>)>,
}

impl Folding {
    /// A folding of no field yet.
    pub(in crate::cargo) fn new() -> Self {
        Self {
            digest: Sha256::new(),
            fields: Vec::new(),
        }
    }

    /// Folds `bytes` in as one field describing `input`.
    pub(in crate::cargo) fn field(&mut self, input: &Input, bytes: &[u8]) {
        super::field(&mut self.digest, bytes);
        self.fields
            .push((input.clone(), Sha256::digest(bytes).to_vec()));
    }

    /// Folds a finished digest in as one field, keeping the fields it was folded from as this folding's own.
    pub(in crate::cargo) fn nest(&mut self, folded: &Folded) {
        super::field(&mut self.digest, folded.digest.as_bytes());
        self.fields.extend(folded.fields.iter().cloned());
    }

    /// The digest and the fields it was folded from.
    pub(in crate::cargo) fn finish(self) -> Folded {
        Folded {
            digest: hex::encode(self.digest.finalize()),
            fields: self.fields,
        }
    }
}

/// A finished digest with the hash of every field it was folded from, under the input each describes.
#[derive(Debug, Clone)]
pub(in crate::cargo) struct Folded {
    digest: String,
    fields: Vec<(Input, Vec<u8>)>,
}

impl Folded {
    /// The digest, as lowercase hexadecimal.
    pub(in crate::cargo) fn digest(&self) -> &str {
        &self.digest
    }

    /// Every input whose fields differ between this folding and `after`, in the order inputs sort in.
    pub(in crate::cargo) fn changes(&self, after: &Self) -> Vec<Change> {
        let (before, now) = (self.by_input(), after.by_input());
        let inputs: BTreeSet<&Input> = before.keys().chain(now.keys()).copied().collect();
        let mut changes: Vec<Change> = inputs
            .into_iter()
            .filter_map(|input| match (before.get(input), now.get(input)) {
                (None, Some(_)) => Some(Change::Appeared(input.clone())),
                (Some(_), None) => Some(Change::Gone(input.clone())),
                (Some(first), Some(second)) if first != second => {
                    Some(Change::Changed(input.clone()))
                }
                (Some(_), Some(_)) | (None, None) => None,
            })
            .collect();
        if changes.is_empty() && (self.fields != after.fields || self.digest != after.digest) {
            changes.push(Change::Reordered);
        }
        changes
    }

    fn by_input(&self) -> BTreeMap<&Input, Vec<&[u8]>> {
        let mut grouped: BTreeMap<&Input, Vec<&[u8]>> = BTreeMap::new();
        for (input, field) in &self.fields {
            grouped.entry(input).or_default().push(field);
        }
        grouped
    }
}

/// The most changes a refusal spells out before it counts the rest.
const SPELLED: usize = 16;

/// A key computed again after its compile that is not the key the compile started under, with every input whose fields differ.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error(
    "compiler inputs changed while preparing their products: {}",
    listed(changes)
)]
pub(in crate::cargo) struct InputsChangedError {
    changes: Vec<Change>,
}

impl InputsChangedError {
    /// The refusal of a folding `after` that is not the folding `before`, or nothing when it is.
    pub(in crate::cargo) fn between(before: &Folded, after: &Folded) -> Option<Self> {
        let changes = before.changes(after);
        (!changes.is_empty()).then_some(Self { changes })
    }

    /// Every input that differs, in the order inputs sort in.
    #[cfg(test)]
    pub(in crate::cargo) fn changes(&self) -> &[Change] {
        &self.changes
    }
}

fn listed(changes: &[Change]) -> String {
    let mut spelled: Vec<String> = changes
        .iter()
        .take(SPELLED)
        .map(ToString::to_string)
        .collect();
    if let Some(rest) = changes.len().checked_sub(SPELLED).filter(|rest| *rest > 0) {
        spelled.push(format!("and {rest} more"));
    }
    spelled.join("; ")
}

#[cfg(test)]
mod tests {
    use super::{Change, Folding, Input, InputsChangedError};
    use std::path::PathBuf;

    fn folded(fields: &[(Input, &[u8])]) -> super::Folded {
        let mut folding = Folding::new();
        for (input, bytes) in fields {
            folding.field(input, bytes);
        }
        folding.finish()
    }

    #[test]
    fn a_folding_names_each_input_that_appeared_went_or_changed_and_nothing_else() {
        let kept = Input::File(PathBuf::from("kept.rs"));
        let changed = Input::File(PathBuf::from("changed.rs"));
        let gone = Input::Searched(PathBuf::from("gone.dll"));
        let appeared = Input::Searched(PathBuf::from("appeared.profraw"));
        let before = folded(&[
            (kept.clone(), b"same"),
            (changed.clone(), b"first"),
            (gone.clone(), b"there"),
        ]);
        let after = folded(&[
            (kept, b"same"),
            (changed.clone(), b"second"),
            (appeared.clone(), b"new"),
        ]);
        let refused = InputsChangedError::between(&before, &after).expect("three inputs differ");
        assert_eq!(
            refused.changes(),
            [
                Change::Changed(changed),
                Change::Appeared(appeared),
                Change::Gone(gone),
            ]
        );
        assert_eq!(
            refused.to_string(),
            "compiler inputs changed while preparing their products: the file changed.rs \
             changed; the loader search entry appeared.profraw appeared; the loader search \
             entry gone.dll is gone"
        );
        assert!(
            InputsChangedError::between(&before, &before).is_none(),
            "a folding is no change from itself"
        );
    }

    #[test]
    fn the_same_fields_in_another_order_are_a_change_a_refusal_still_states() {
        let one = (Input::Root, b"one".as_slice());
        let two = (Input::Root, b"two".as_slice());
        let before = folded(&[one.clone(), two.clone()]);
        let after = folded(&[two, one]);
        assert_ne!(before.digest(), after.digest());
        assert_eq!(
            InputsChangedError::between(&before, &after)
                .expect("the digests differ")
                .changes(),
            [Change::Changed(Input::Root)]
        );
        let first = (Input::Versions, b"same".as_slice());
        let second = (Input::Arguments, b"same".as_slice());
        let before = folded(&[first.clone(), second.clone()]);
        let after = folded(&[second, first]);
        assert_eq!(
            InputsChangedError::between(&before, &after)
                .expect("the digests differ")
                .changes(),
            [Change::Reordered]
        );
    }

    #[test]
    fn a_refusal_spells_out_a_bounded_number_of_changes_and_counts_the_rest() {
        let before = folded(&[]);
        let paths: Vec<Input> = (0..20)
            .map(|index| Input::File(PathBuf::from(format!("{index:02}.rs"))))
            .collect();
        let after = folded(
            &paths
                .iter()
                .map(|input| (input.clone(), b"new".as_slice()))
                .collect::<Vec<_>>(),
        );
        let message = InputsChangedError::between(&before, &after)
            .expect("twenty inputs appeared")
            .to_string();
        assert!(message.contains("the file 15.rs appeared"), "{message}");
        assert!(!message.contains("16.rs"), "{message}");
        assert!(message.ends_with("; and 4 more"), "{message}");
    }

    #[test]
    fn a_nested_folding_keeps_the_digest_it_folded_and_names_its_own_fields() {
        let inner = |bytes: &[u8]| folded(&[(Input::Searched(PathBuf::from("entry")), bytes)]);
        let outer = |nested: &super::Folded| {
            let mut folding = Folding::new();
            folding.field(&Input::Derivation, b"schema");
            folding.nest(nested);
            folding.finish()
        };
        let mut plain = sha2::Sha256::default();
        super::super::field(&mut plain, b"schema");
        super::super::field(&mut plain, inner(b"one").digest().as_bytes());
        assert_eq!(
            outer(&inner(b"one")).digest(),
            hex::encode(sha2::Digest::finalize(plain)),
            "a nested folding is folded in as the one field its digest is"
        );
        assert_eq!(
            InputsChangedError::between(&outer(&inner(b"one")), &outer(&inner(b"two")))
                .expect("the nested entry changed")
                .changes(),
            [Change::Changed(Input::Searched(PathBuf::from("entry")))]
        );
    }
}
