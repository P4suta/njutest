// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every code the sealed host can report is documented with the remedy it carries, and every documented `RS` code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rust_mutants_sealed::{
    EntryFault, EnvironmentFault, ErrorCode, ImportFault, Invariant, MemoryFault, PreopenFault,
    RuntimeStep, SealedCode, SealedError, SnapshotFault, WorkingFault, error_codes,
};

use crate::common::runner;

/// The rows of `docs/errors.md` whose code starts `RS`, by code, as their meaning and remedy.
fn documented() -> BTreeMap<String, (String, String)> {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md is readable");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, rest) = cell.split_once("` | ")?;
            let (meaning, remedy) = rest.strip_suffix(" |")?.split_once(" | ")?;
            code.starts_with("RS")
                .then(|| (code.to_owned(), (meaning.to_owned(), remedy.to_owned())))
        })
        .collect()
}

#[test]
fn every_code_is_documented_and_every_documented_code_exists() {
    let declared: BTreeSet<String> = error_codes()
        .iter()
        .map(|code| code.code().to_owned())
        .collect();
    let documented: BTreeSet<String> = documented().into_keys().collect();
    assert!(!declared.is_empty(), "the sealed host declares its codes");
    assert_eq!(
        declared, documented,
        "docs/errors.md and rust_mutants_sealed::error_codes disagree"
    );
}

#[test]
fn codes_are_unique_well_formed_and_in_code_order() {
    let codes: Vec<&str> = error_codes().iter().map(ErrorCode::code).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are declared in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("RS")
                && code
                    .chars()
                    .skip(2)
                    .all(|character| character.is_ascii_digit()),
            "malformed code {code}"
        );
    }
    assert_eq!(
        codes.len(),
        SealedCode::ALL.len(),
        "one code per failure mode"
    );
}

#[test]
fn every_row_says_what_the_code_says() {
    let documented = documented();
    let mut wrong = Vec::new();
    for code in error_codes() {
        let row = documented.get(code.code());
        let said = row.map(|(meaning, remedy)| (meaning.as_str(), remedy.as_str()));
        let carried = (capitalised(code.summary()), code.remedy());
        if said != Some((carried.0.as_str(), carried.1)) {
            wrong.push(format!(
                "{}: the table says {said:?}, the code carries {carried:?}",
                code.code()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_code_says_what_to_do_about_it() {
    for code in error_codes() {
        assert!(!code.remedy().is_empty(), "{} names no remedy", code.code());
    }
}

/// Every way a module's memory is refused, the total match beside the list failing to compile once the set gains a way.
fn memory_faults() -> [MemoryFault; 5] {
    let faults = [
        MemoryFault::Memory64,
        MemoryFault::Shared,
        MemoryFault::CustomPageSize,
        MemoryFault::Imported,
        MemoryFault::Count { count: 2 },
    ];
    for fault in faults {
        match fault {
            MemoryFault::Memory64
            | MemoryFault::Shared
            | MemoryFault::CustomPageSize
            | MemoryFault::Imported
            | MemoryFault::Count { .. } => {}
        }
    }
    faults
}

/// An error wasmtime could have given, naming `what`.
fn planted(what: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!("planted {what}"))
}

/// One failure of every variant that carries no fault, each beside the detail its message must carry for its remedy to be followed.
fn unfaulted_failures() -> Vec<(SealedError, String)> {
    let malformed = runner()
        .prepare(b"not webassembly")
        .expect_err("text is not WebAssembly");
    vec![
        (
            SealedError::ArgumentHoldsNul { index: 3 },
            "argument 3".to_owned(),
        ),
        (malformed, "not a WebAssembly binary".to_owned()),
        (SealedError::ModuleComponent, "component".to_owned()),
        (
            SealedError::Engine {
                source: planted("engine"),
            },
            "planted engine".to_owned(),
        ),
        (
            SealedError::Compile {
                source: planted("compile"),
            },
            "planted compile".to_owned(),
        ),
        (
            SealedError::Link {
                source: planted("link"),
            },
            "planted link".to_owned(),
        ),
        (
            SealedError::WatchdogUnavailable {
                source: std::io::Error::other("planted thread"),
            },
            "planted thread".to_owned(),
        ),
        (
            SealedError::WatchdogExpired {
                limit: Duration::from_millis(250),
            },
            "250ms".to_owned(),
        ),
        (
            SealedError::TrapUnclassified {
                trap: "planted trap".to_owned(),
            },
            "planted trap".to_owned(),
        ),
        (SealedError::Interrupted, "interrupted".to_owned()),
    ]
}

/// One failure of every fault of every variant that carries one, each beside the detail its message must carry.
fn faulted_failures() -> Vec<(SealedError, String)> {
    let mut failures = Vec::new();
    for fault in EnvironmentFault::ALL {
        let name = "NAME".to_owned();
        failures.push((
            SealedError::EnvironmentVariable { name, fault },
            format!("\"NAME\" cannot be given to a WASI guest: {fault}"),
        ));
    }
    for fault in SnapshotFault::ALL {
        let path = "a/b".to_owned();
        failures.push((
            SealedError::SnapshotPath { path, fault },
            format!("\"a/b\" cannot be held: {fault}"),
        ));
    }
    for fault in PreopenFault::ALL {
        let path = "/guest".to_owned();
        failures.push((
            SealedError::Preopen { path, fault },
            format!("\"/guest\" cannot be preopened: {fault}"),
        ));
    }
    for fault in WorkingFault::ALL {
        let (tree, directory) = ("/guest".to_owned(), "pkg".to_owned());
        failures.push((
            SealedError::WorkingDirectory {
                tree,
                directory,
                fault,
            },
            format!("\"pkg\" of the tree at \"/guest\" cannot be preopened: {fault}"),
        ));
    }
    for fault in memory_faults() {
        failures.push((SealedError::ModuleMemory { fault }, fault.to_string()));
    }
    for fault in ImportFault::ALL {
        let (module, name) = ("env".to_owned(), "f".to_owned());
        failures.push((
            SealedError::ModuleImport {
                module,
                name,
                fault,
            },
            format!("env::f, which {fault}"),
        ));
    }
    for fault in EntryFault::ALL {
        failures.push((SealedError::ModuleEntry { fault }, fault.to_string()));
    }
    for during in RuntimeStep::ALL {
        failures.push((
            SealedError::Runtime {
                during,
                source: planted("runtime"),
            },
            format!("while {during}: planted runtime"),
        ));
    }
    for invariant in Invariant::ALL {
        failures.push((
            SealedError::HostInvariant { invariant },
            invariant.to_string(),
        ));
    }
    failures
}

#[test]
fn every_failure_reports_its_own_code_and_says_what_went_wrong() {
    let mut failures = unfaulted_failures();
    failures.extend(faulted_failures());
    let mut reported = BTreeSet::new();
    let mut messages = BTreeSet::new();
    for (failure, detail) in &failures {
        let message = failure.to_string();
        assert!(
            message.contains(detail.as_str()),
            "{message:?} does not say {detail:?}"
        );
        reported.insert(failure.code().code());
        messages.insert(message);
    }
    let declared: BTreeSet<&str> = error_codes().iter().map(ErrorCode::code).collect();
    assert_eq!(
        reported, declared,
        "every code is some failure's, and every failure's code is declared"
    );
    assert_eq!(
        messages.len(),
        failures.len(),
        "two different failures read the same, so a reader cannot tell which happened"
    );
}

/// `summary` as a sentence: its first letter capitalised and a full stop after it.
fn capitalised(summary: &str) -> String {
    let mut characters = summary.chars();
    let first: String = characters
        .next()
        .map(|first| first.to_uppercase().collect())
        .unwrap_or_default();
    format!("{first}{}.", characters.as_str())
}
