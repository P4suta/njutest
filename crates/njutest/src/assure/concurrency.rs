// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each test binary a baseline measured is proven to run one thread: its touch record, held beside what every package it links can start.

use std::collections::BTreeMap;

use crate::concurrency::proof::{Evidence, Harness, PackageScan, Reach, standing, threads_of};
use crate::report::concurrency::ConcurrencyRecord;
use rust_mutants::execute::TargetKind;

/// One record per test binary the session measured, in binary order, each package of every closure read once; `harness_args` are what every libtest binary was run with.
#[must_use]
pub fn recorded(
    session: &rust_mutants::session::Session,
    harness_args: &[String],
) -> Vec<ConcurrencyRecord> {
    let threads = threads_of(harness_args);
    let mut binaries: BTreeMap<String, (&str, Harness)> = BTreeMap::new();
    for target in session.targets() {
        let harness = match (target.kind, target.harness) {
            (TargetKind::Doc, _) => Harness::Doctest,
            (
                TargetKind::Lib
                | TargetKind::Bin
                | TargetKind::Test
                | TargetKind::Example
                | TargetKind::ProcMacro,
                true,
            ) => Harness::Libtest(threads),
            (
                TargetKind::Lib
                | TargetKind::Bin
                | TargetKind::Test
                | TargetKind::Example
                | TargetKind::ProcMacro,
                false,
            ) => Harness::Other,
        };
        binaries.insert(target.id.clone(), (target.package.as_str(), harness));
    }
    let metadata = session.metadata();
    let touched = &session.verified().touched.targets;
    let mut read: BTreeMap<String, PackageScan> = BTreeMap::new();
    binaries
        .into_iter()
        .map(|(binary, (package, harness))| {
            let closure = metadata
                .members()
                .find(|member| member.name == package)
                .map_or_else(Vec::new, |member| metadata.closure(&member.id));
            let missing = closure.is_empty().then(|| PackageScan {
                package: package.to_owned(),
                links: false,
                found: Vec::new(),
                unread: vec!["Cargo.toml".to_owned()],
            });
            for id in &closure {
                if !read.contains_key(id) {
                    let scan = metadata.package(id).map_or_else(
                        || PackageScan {
                            package: id.clone(),
                            links: false,
                            found: Vec::new(),
                            unread: vec!["Cargo.toml".to_owned()],
                        },
                        crate::concurrency::read::package,
                    );
                    read.insert(id.clone(), scan);
                }
            }
            let packages: Vec<&PackageScan> = closure
                .iter()
                .filter_map(|id| read.get(id))
                .chain(missing.iter())
                .collect();
            let reach = match touched.get(&binary) {
                None => Reach::NotRecorded,
                Some(recorded) if recorded.reached.loose.is_empty() => Reach::OnItsTests,
                Some(_) => Reach::OffItsTests,
            };
            ConcurrencyRecord {
                standing: standing(Evidence {
                    reach,
                    harness,
                    packages: &packages,
                }),
                target: binary,
            }
        })
        .collect()
}
