// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether a test binary is proven to run one thread: what its baseline reached, held beside what every package it links can start.

use super::scan::{Found, Starts};

/// What the sources of one package can start, and what of it could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageScan {
    /// The package, as `name@version`.
    pub package: String,
    /// Whether its manifest names a native library it links.
    pub links: bool,
    /// Every place a file of it can start something, with the file's path relative to the package.
    pub found: Vec<(String, Found)>,
    /// Every file of it that was not Rust this release reads.
    pub unread: Vec<String>,
}

/// Why a binary can run more than one thread.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Because {
    /// Its baseline reached code on a thread that is not one of its tests.
    LooseReach,
    /// A package it links can start one here.
    Starts {
        /// The package.
        package: String,
        /// The file, relative to the package.
        path: String,
        /// The 1-based line.
        line: usize,
        /// What it can start.
        what: Starts,
    },
}

/// Why nothing is proven about a binary either way.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unproven {
    /// Its baseline recorded nothing it reached.
    NoTouch,
    /// Its harness is not libtest, which is what names each test's thread.
    NotLibtest,
    /// A file of a package it links was not read.
    Unread {
        /// The package.
        package: String,
        /// The file.
        path: String,
    },
    /// A package it links runs code no Rust source here shows: a native library, or an `extern` block.
    NativeCode {
        /// The package.
        package: String,
        /// Where, or `links` where its manifest names the library.
        by: String,
    },
}

/// What a run establishes about whether one binary runs more than one thread.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Standing {
    /// Proven to run one thread: its baseline reached nothing off its tests' threads, and nothing it links can start another.
    SingleThreaded,
    /// It can run more than one, for every reason that holds.
    Concurrent {
        /// The reasons.
        because: Vec<Because>,
    },
    /// Nothing is proven either way, for every reason that holds.
    NotProven {
        /// The reasons.
        why: Vec<Unproven>,
    },
}

/// Where a binary's baseline reached code, as its touch record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// It recorded nothing.
    NotRecorded,
    /// Only on the threads its tests ran on.
    OnItsTests,
    /// On a thread that is not one of its tests, too.
    OffItsTests,
}

/// What is known about one binary: where its baseline reached code, whether its harness is libtest, and what every package it links can start.
#[derive(Debug, Clone, Copy)]
pub struct Evidence<'a> {
    /// Where its baseline reached code.
    pub reach: Reach,
    /// Whether its harness is libtest.
    pub libtest: bool,
    /// Every package in its closure.
    pub packages: &'a [&'a PackageScan],
}

/// What `evidence` establishes: concurrent where anything says it can be, not proven where anything could not be looked at, and single-threaded only where every premise holds.
#[must_use]
pub fn standing(evidence: Evidence<'_>) -> Standing {
    let mut because = Vec::new();
    let mut why = Vec::new();
    match evidence.reach {
        Reach::OffItsTests => because.push(Because::LooseReach),
        Reach::OnItsTests => {}
        Reach::NotRecorded => why.push(Unproven::NoTouch),
    }
    if !evidence.libtest {
        why.push(Unproven::NotLibtest);
    }
    for scan in evidence.packages {
        why.extend(scan.unread.iter().map(|path| Unproven::Unread {
            package: scan.package.clone(),
            path: path.clone(),
        }));
        for (path, found) in &scan.found {
            match found.what {
                Starts::Native => why.push(Unproven::NativeCode {
                    package: scan.package.clone(),
                    by: format!("{path}:{}", found.line),
                }),
                Starts::Spawn | Starts::Scope | Starts::Parallel | Starts::Runtime => {
                    because.push(Because::Starts {
                        package: scan.package.clone(),
                        path: path.clone(),
                        line: found.line,
                        what: found.what,
                    });
                }
            }
        }
        if scan.links {
            why.push(Unproven::NativeCode {
                package: scan.package.clone(),
                by: "links".to_owned(),
            });
        }
    }
    if !because.is_empty() {
        because.sort();
        because.dedup();
        return Standing::Concurrent { because };
    }
    if !why.is_empty() {
        why.sort();
        why.dedup();
        return Standing::NotProven { why };
    }
    Standing::SingleThreaded
}
