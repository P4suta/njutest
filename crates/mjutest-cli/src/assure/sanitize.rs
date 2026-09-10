// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running the suite under a sanitizer, when the configuration asks for one.
//!
//! A sanitizer is opt-in, so its absence never fails a run. What it does do
//! is state a limitation and a finding: somebody asked for the suite to be
//! run under it, and a run that could not do that has not established what
//! was asked for.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Duration;

use rust_mutants::runner::{Spec, run};

use crate::report::{Finding, FindingKind, Limitation};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// What a sanitizer says when it has found something.
const FOUND: [&str; 5] = [
    "ERROR: AddressSanitizer",
    "WARNING: ThreadSanitizer",
    "ERROR: LeakSanitizer",
    "MemorySanitizer:",
    "runtime error:",
];

/// What the toolchain says when it will not sanitize.
const UNAVAILABLE: [&str; 4] = [
    "only accepted on the nightly",
    "unknown `-Z` flag",
    "is not supported for this target",
    "no such command",
];

/// What the phase is asked to run, and how it is bounded.
#[derive(Debug, Clone)]
pub struct Sanitizing<'a> {
    /// The workspace root.
    pub root: &'a Path,
    /// The cargo to drive, which must be one that understands `+nightly`.
    pub cargo: &'a Path,
    /// The target triple the suite is built for, which a sanitizer needs named so the host tools are not instrumented too.
    pub host: &'a str,
    /// The environment it runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The packages to run. Empty is the whole workspace.
    pub packages: &'a [String],
    /// The sanitizers the configuration asked for, by name.
    pub sanitizers: &'a [String],
    /// How long one sanitizer run may take.
    pub timeout: Option<Duration>,
    /// Whether cargo may reach the network.
    pub offline: bool,
    /// Whether cargo may change the lock file. A phase that let it would measure a dependency set the baseline never saw, and would write into the tree under measurement.
    pub locked: bool,
}

/// What running the suite under the sanitizers established.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sanitized {
    /// The sanitizers that ran, in the order they were asked for.
    pub ran: Vec<String>,
    /// What they found.
    pub findings: Vec<Finding>,
    /// What they could not say.
    pub limitations: Vec<Limitation>,
}

/// Runs the suite once under each sanitizer the configuration asked for.
#[must_use]
pub fn sanitize(sanitizing: &Sanitizing<'_>, watch: Watch<'_>) -> Sanitized {
    let mut done = Sanitized::default();
    if !sanitizing.sanitizers.is_empty() {
        done.limitations.push(Limitation::new(
            crate::limitation::SANITIZER_STANDARD_LIBRARY_NOT_INSTRUMENTED,
            "the standard library the suite links is not built with the sanitizer, so what \
             it holds is not what was checked",
        ));
        for sanitizer in sanitizing
            .sanitizers
            .iter()
            .take_while(|_| !watch.cancel.is_cancelled())
        {
            one(&mut done, sanitizing, sanitizer, watch);
        }
    }
    done
}

/// Runs the suite once under one sanitizer.
fn one(done: &mut Sanitized, sanitizing: &Sanitizing<'_>, sanitizer: &str, watch: Watch<'_>) {
    let mut argv: Vec<OsString> = vec![
        sanitizing.cargo.as_os_str().to_owned(),
        OsString::from("+nightly"),
        OsString::from("test"),
        OsString::from("--target"),
        OsString::from(sanitizing.host),
    ];
    if sanitizing.packages.is_empty() {
        argv.push(OsString::from("--workspace"));
    } else {
        for package in sanitizing.packages {
            argv.push(OsString::from("--package"));
            argv.push(OsString::from(package));
        }
    }
    if sanitizing.offline {
        argv.push(OsString::from("--offline"));
    }
    if sanitizing.locked {
        argv.push(OsString::from("--locked"));
    }
    let mut spec = Spec::new(argv);
    spec.dir = Some(sanitizing.root.to_path_buf());
    spec.timeout = sanitizing.timeout;
    spec.env = Some(instrumenting(&sanitizing.env, sanitizer));

    let ran = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &ran));
    let said = String::from_utf8_lossy(&ran.output).into_owned();
    if ran.error.is_some() || UNAVAILABLE.iter().any(|marker| said.contains(marker)) {
        refuse(done, sanitizer, "the toolchain would not run it");
        return;
    }
    if ran.timed_out {
        refuse(done, sanitizer, "it ran out of time");
        return;
    }
    done.ran.push(sanitizer.to_owned());
    if let Some(line) = FOUND.iter().find_map(|marker| {
        said.lines()
            .find(|line| line.contains(marker))
            .map(|line| line.trim().to_owned())
    }) {
        done.findings.push(Finding {
            kind: FindingKind::UndefinedBehaviour,
            subject: format!("sanitizer:{sanitizer}"),
            detail: line,
            path: None,
            position: None,
        });
    } else if ran.exit_code != 0 {
        done.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: format!("sanitizer:{sanitizer}"),
            detail: format!("a test fails under {sanitizer} that passes without it"),
            path: None,
            position: None,
        });
    }
}

/// A sanitizer that was asked for and could not be run: a gap somebody asked to close, stated as one.
fn refuse(done: &mut Sanitized, sanitizer: &str, why: &str) {
    done.limitations.push(Limitation::new(
        crate::limitation::SANITIZER_UNAVAILABLE,
        &format!("{sanitizer} was asked for and {why}"),
    ));
    done.findings.push(Finding {
        kind: FindingKind::NotMeasured,
        subject: format!("sanitizer:{sanitizer}"),
        detail: format!(
            "the suite was not run under {sanitizer}, which the configuration asks for"
        ),
        path: None,
        position: None,
    });
}

/// The environment one sanitizer run adds: the flag, and nothing else the caller did not already have.
///
/// A map and not a list, because a process started with two bindings of one
/// name reads whichever of them the operating system hands it first, and
/// whether the suite is built with the sanitizer at all would then be decided
/// by the order a list happened to be in. Saying it as a map is what makes
/// "one name, one value" true rather than maintained.
fn instrumenting(base: &[(OsString, OsString)], sanitizer: &str) -> Vec<(OsString, OsString)> {
    let mut env: BTreeMap<OsString, OsString> = base.iter().cloned().collect();
    let mut flags: Vec<String> = env
        .get(OsStr::new("RUSTFLAGS"))
        .map(|value| {
            value
                .to_string_lossy()
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let _dropped = env.remove(OsStr::new("CARGO_ENCODED_RUSTFLAGS"));
    flags.push(format!("-Zsanitizer={sanitizer}"));
    let _replaced = env.insert(OsString::from("RUSTFLAGS"), OsString::from(flags.join(" ")));
    env.into_iter().collect()
}
