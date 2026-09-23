// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which targets a change can be noticed by, decided from the items each target entered on a measured tree.

use std::collections::{BTreeMap, BTreeSet};

use crate::span::Span;
use crate::touch::{Steadiness, Touched};

/// One file a change touched, as it differs from the tree that was measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// The file is still there and these byte ranges of the measured version were replaced; an insertion is an empty range where it went.
    Edited {
        /// The workspace-relative path with forward slashes.
        path: String,
        /// The ranges of the measured file the change replaced.
        ranges: Vec<Span>,
    },
    /// The file was added, deleted, or renamed, so no range of the measured file names what changed.
    Whole {
        /// The workspace-relative path with forward slashes.
        path: String,
    },
}

impl Changed {
    /// The file the change is to.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Edited { path, .. } | Self::Whole { path } => path,
        }
    }
}

/// Why every target runs, whatever it entered: a change the measurement cannot place in an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Everything {
    /// A manifest, lock file, build script, toolchain file, or cargo configuration changed, which can change what every target compiles to.
    Build {
        /// The file.
        path: String,
    },
    /// A file changed whose measured version held no item: a file the run did not instrument, so nothing says who ran it.
    Unitemized {
        /// The file.
        path: String,
    },
    /// A file was added, deleted, or renamed.
    Whole {
        /// The file.
        path: String,
    },
    /// A change lies outside every item's body: a signature, an attribute, a `use`, a type, a `static`, which every caller may depend on.
    OutsideItems {
        /// The file.
        path: String,
        /// The range of the measured file it replaced.
        range: Span,
    },
    /// A change lies in the body of an item whose entry the guards cannot record, which a body the compiler may evaluate at compile time is.
    Unmeasurable {
        /// The item, as a reader writes it.
        item: String,
    },
}

/// What a selection decided for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// The target's tests entered none of the changed items on the measured tree, and its measurement held on a second run, so running it would ask nothing the change could answer.
    Skip,
    /// The target runs, and why.
    Run(Why),
}

/// Why one target runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Its tests entered these changed items.
    Entered(BTreeSet<u32>),
    /// The change is one no measurement can place.
    Everything(Everything),
    /// Its reach was not shown to be a function of the target: a second run moved it, or nothing compared a second run with the first.
    Unestablished(Steadiness),
    /// The measurement holds nothing about the target.
    Unmeasured,
}

/// Whether `path` is a file whose change can change what every target compiles to, whatever it is an item of.
#[must_use]
pub fn builds_everything(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "Cargo.toml" | "Cargo.lock" | "build.rs" | "rust-toolchain" | "rust-toolchain.toml"
    ) || path.starts_with(".cargo/")
        || path.contains("/.cargo/")
}

/// The items `changes` falls in, or the first change no measurement can place.
///
/// # Errors
/// [`Everything`] naming the first change that cannot be placed in a measurable item.
pub fn changed_items(touched: &Touched, changes: &[Changed]) -> Result<BTreeSet<u32>, Everything> {
    let mut items = BTreeSet::new();
    for change in changes {
        let path = change.path();
        if builds_everything(path) {
            return Err(Everything::Build {
                path: path.to_owned(),
            });
        }
        let ranges = match change {
            Changed::Whole { path } => {
                return Err(Everything::Whole { path: path.clone() });
            }
            Changed::Edited { ranges, .. } => ranges,
        };
        if !touched.items.iter().any(|item| item.path == path) {
            return Err(Everything::Unitemized {
                path: path.to_owned(),
            });
        }
        for range in ranges {
            let Some(item) = touched.item_holding(path, *range) else {
                return Err(Everything::OutsideItems {
                    path: path.to_owned(),
                    range: *range,
                });
            };
            if !item.measurable {
                return Err(Everything::Unmeasurable {
                    item: item.name.clone(),
                });
            }
            items.insert(item.index);
        }
    }
    Ok(items)
}

/// What a change decides for every target the measurement or its standings name: the targets whose tests entered a changed item run, as does every target whose reach was not shown to hold; the rest are skipped.
#[must_use]
pub fn decide(
    touched: &Touched,
    standing: &BTreeMap<String, Steadiness>,
    changes: &[Changed],
) -> BTreeMap<String, Decided> {
    let targets: BTreeSet<&String> = touched.targets.keys().chain(standing.keys()).collect();
    let placed = changed_items(touched, changes);
    targets
        .into_iter()
        .map(|target| {
            let decided = match (&placed, touched.targets.get(target), standing.get(target)) {
                (Err(everything), _, _) => Decided::Run(Why::Everything(everything.clone())),
                (Ok(_), None, _) | (Ok(_), Some(_), None) => Decided::Run(Why::Unmeasured),
                (
                    Ok(_),
                    Some(_),
                    Some(steadiness @ (Steadiness::Moved(_) | Steadiness::NotMeasured(_))),
                ) => Decided::Run(Why::Unestablished(steadiness.clone())),
                (Ok(items), Some(record), Some(Steadiness::Held)) => {
                    let entered: BTreeSet<u32> = record
                        .entered_by_any()
                        .intersection(items)
                        .copied()
                        .collect();
                    if entered.is_empty() {
                        Decided::Skip
                    } else {
                        Decided::Run(Why::Entered(entered))
                    }
                }
            };
            (target.clone(), decided)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{Changed, Decided, Everything, Why, decide};
    use crate::span::Span;
    use crate::touch::{Item, Seen, Steadiness, TargetTouches, Touched, Unmeasured};

    fn span(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    /// Two items of `src/lib.rs`: `alpha` with its body at 10..20 and `beta` at 30..40, and `gamma`, a `const fn`, at 50..60.
    fn measured() -> Touched {
        let item = |index, name: &str, body: Span, measurable| Item {
            index,
            package: "demo".to_owned(),
            path: "src/lib.rs".to_owned(),
            name: name.to_owned(),
            span: span(body.start.saturating_sub(5), body.end),
            body,
            measurable,
        };
        let entering = |indices: &[u32]| TargetTouches {
            entered: Seen {
                tests: BTreeMap::from([("t".to_owned(), indices.iter().copied().collect())]),
                loose: BTreeSet::new(),
            },
            ..TargetTouches::default()
        };
        Touched {
            targets: BTreeMap::from([
                ("enters-alpha".to_owned(), entering(&[0])),
                ("enters-beta".to_owned(), entering(&[1])),
            ]),
            items: vec![
                item(0, "alpha", span(10, 20), true),
                item(1, "beta", span(30, 40), true),
                item(2, "gamma", span(50, 60), false),
            ],
            ..Touched::default()
        }
    }

    fn held() -> BTreeMap<String, Steadiness> {
        BTreeMap::from([
            ("enters-alpha".to_owned(), Steadiness::Held),
            ("enters-beta".to_owned(), Steadiness::Held),
        ])
    }

    fn edited(ranges: &[Span]) -> Vec<Changed> {
        vec![Changed::Edited {
            path: "src/lib.rs".to_owned(),
            ranges: ranges.to_vec(),
        }]
    }

    #[test]
    fn a_body_edit_runs_the_targets_that_entered_it_and_skips_the_rest() {
        let decided = decide(&measured(), &held(), &edited(&[span(12, 14)]));
        assert_eq!(
            decided.get("enters-alpha"),
            Some(&Decided::Run(Why::Entered(BTreeSet::from([0])))),
            "{decided:?}"
        );
        assert_eq!(
            decided.get("enters-beta"),
            Some(&Decided::Skip),
            "a target that entered only beta ran no code the change replaced: {decided:?}"
        );
    }

    #[test]
    fn a_change_outside_every_body_runs_everything() {
        let decided = decide(&measured(), &held(), &edited(&[span(22, 24)]));
        for (target, one) in &decided {
            assert_eq!(
                one,
                &Decided::Run(Why::Everything(Everything::OutsideItems {
                    path: "src/lib.rs".to_owned(),
                    range: span(22, 24),
                })),
                "{target}: a signature, a type or a `use` between items is read by callers no \
                 entry records"
            );
        }
    }

    #[test]
    fn an_unmeasurable_body_runs_everything() {
        let decided = decide(&measured(), &held(), &edited(&[span(52, 53)]));
        assert!(
            decided.values().all(|one| one
                == &Decided::Run(Why::Everything(Everything::Unmeasurable {
                    item: "gamma".to_owned()
                }))),
            "a `const fn` evaluated by the compiler is entered by nobody the guards can see: \
             {decided:?}"
        );
    }

    #[test]
    fn a_build_file_an_unitemized_file_and_a_whole_file_each_run_everything() {
        for (changes, why) in [
            (
                vec![Changed::Edited {
                    path: "Cargo.toml".to_owned(),
                    ranges: vec![span(0, 1)],
                }],
                Everything::Build {
                    path: "Cargo.toml".to_owned(),
                },
            ),
            (
                vec![Changed::Edited {
                    path: "src/other.rs".to_owned(),
                    ranges: vec![span(0, 1)],
                }],
                Everything::Unitemized {
                    path: "src/other.rs".to_owned(),
                },
            ),
            (
                vec![Changed::Whole {
                    path: "src/lib.rs".to_owned(),
                }],
                Everything::Whole {
                    path: "src/lib.rs".to_owned(),
                },
            ),
        ] {
            let decided = decide(&measured(), &held(), &changes);
            assert!(
                decided
                    .values()
                    .all(|one| one == &Decided::Run(Why::Everything(why.clone()))),
                "{why:?}: {decided:?}"
            );
        }
    }

    #[test]
    fn a_target_whose_reach_was_not_shown_to_hold_runs_whatever_it_entered() {
        let mut standing = held();
        standing.insert(
            "enters-beta".to_owned(),
            Steadiness::NotMeasured(Unmeasured::OtherTests),
        );
        let decided = decide(&measured(), &standing, &edited(&[span(12, 14)]));
        assert_eq!(
            decided.get("enters-beta"),
            Some(&Decided::Run(Why::Unestablished(Steadiness::NotMeasured(
                Unmeasured::OtherTests
            )))),
            "entering nothing changed is a fact about one run, and nothing showed a second run \
             enters the same: {decided:?}"
        );
    }

    #[test]
    fn a_target_with_no_record_or_no_standing_runs() {
        let mut standing = held();
        standing.remove("enters-beta");
        standing.insert("unrecorded".to_owned(), Steadiness::Held);
        let decided = decide(&measured(), &standing, &edited(&[span(12, 14)]));
        assert_eq!(
            decided.get("enters-beta"),
            Some(&Decided::Run(Why::Unmeasured))
        );
        assert_eq!(
            decided.get("unrecorded"),
            Some(&Decided::Run(Why::Unmeasured))
        );
    }

    #[test]
    fn an_insertion_is_placed_where_it_went() {
        let decided = decide(&measured(), &held(), &edited(&[span(35, 35)]));
        assert_eq!(
            decided.get("enters-beta"),
            Some(&Decided::Run(Why::Entered(BTreeSet::from([1])))),
            "an empty range inside beta's body is a change to beta: {decided:?}"
        );
        assert_eq!(decided.get("enters-alpha"), Some(&Decided::Skip));
    }
}
