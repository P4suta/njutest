// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each test binary a baseline measured is proven to run one thread: its touch record, held beside what every package it links can start.

use std::collections::BTreeMap;

use crate::concurrency::proof::{
    Evidence, Harness, PackageScan, Reach, Threads, standing, threads_of,
};
use crate::observe::SourceReadError;
use crate::report::concurrency::ConcurrencyRecord;
use rust_mutants::execute::TargetKind;

/// One record per measured test binary, and the closure packages the build compiled nothing of.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn recorded(
    session: &rust_mutants::session::Session,
    harness_args: &[String],
) -> Result<(Vec<ConcurrencyRecord>, Vec<String>), SourceReadError> {
    let threads = threads_of(harness_args);
    let mut binaries: BTreeMap<String, (&str, Harness)> = BTreeMap::new();
    for target in session.targets() {
        let harness = harness_of(target, threads);
        binaries.insert(target.id.clone(), (target.package.as_str(), harness));
    }
    let metadata = session.metadata();
    let touched = &session.verified().touched.targets;
    let compiled = crate::concurrency::read::Compiled::of(session.compilation())?;
    let mut read: BTreeMap<String, PackageScan> = BTreeMap::new();
    let mut uncompiled: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let records = binaries
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
            let (linked, left_out) = linked(&closure, &compiled);
            uncompiled.extend(left_out.into_iter().cloned());
            for id in &linked {
                let id = *id;
                if !read.contains_key(id) {
                    let scan = match metadata.package(id) {
                        Some(package) => crate::concurrency::read::package(package, &compiled)?,
                        None => PackageScan {
                            package: id.clone(),
                            links: false,
                            found: Vec::new(),
                            unread: vec!["Cargo.toml".to_owned()],
                        },
                    };
                    let mut scan = scan;
                    if compiled.inputs.is_empty() {
                        scan.unread
                            .push("the build reported no unit it compiled".to_owned());
                    }
                    read.insert(id.clone(), scan);
                }
            }
            let packages: Vec<&PackageScan> = linked
                .iter()
                .filter_map(|id| read.get(*id))
                .chain(missing.iter())
                .collect();
            let reach = match touched.get(&binary) {
                None => Reach::NotRecorded,
                Some(recorded) if recorded.reached.loose.is_empty() => Reach::OnItsTests,
                Some(_) => Reach::OffItsTests,
            };
            Ok(ConcurrencyRecord {
                standing: standing(Evidence {
                    reach,
                    harness,
                    packages: &packages,
                }),
                target: binary,
            })
        })
        .collect::<Result<Vec<ConcurrencyRecord>, SourceReadError>>()?;
    Ok((records, uncompiled.into_iter().collect()))
}

/// Which packages of `closure` the session's build compiled, and so links, and which it compiled no unit of for this target and these features, and so does not; where the build reported no unit at all nothing is left out, since that says nothing about what it compiled.
#[must_use]
pub fn linked<'a>(
    closure: &'a [String],
    compiled: &crate::concurrency::read::Compiled,
) -> (Vec<&'a String>, Vec<&'a String>) {
    if compiled.inputs.is_empty() {
        return (closure.iter().collect(), Vec::new());
    }
    closure
        .iter()
        .partition(|id| compiled.inputs.contains_key(id.as_str()))
}

/// The harness `target` runs its tests under, where libtest's runs them on `threads`.
const fn harness_of(target: &rust_mutants::execute::TestTarget, threads: Threads) -> Harness {
    match (target.kind, target.harness) {
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
    }
}
