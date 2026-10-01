// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cost counters fed by the same events as a recording, kept independently of verdicts.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use super::Payload;

/// One session's optional diagnostic, shared by its recorder clones.
#[derive(Debug)]
pub(super) struct Costs {
    output: File,
    binary: String,
    test: String,
    root: String,
    sealed: rust_mutants_sealed::Counted,
    work: Mutex<Work>,
}

/// What one bound build key accumulated: every request, every process it started, and why.
#[derive(Debug, Default, Serialize)]
struct KeyWork {
    requests: u64,
    hits: u64,
    misses: u64,
    processes: u64,
    reasons: BTreeMap<String, u64>,
    refused_writes: BTreeMap<String, u64>,
}

/// What one unbound identity accumulated, with the reason it never had a key.
#[derive(Debug, Default, Serialize)]
struct UnboundWork {
    requests: u64,
    misses: u64,
    processes: u64,
}

#[derive(Debug, Default, Serialize)]
struct Work {
    builds: u64,
    build_requests: u64,
    build_hits: u64,
    build_misses: u64,
    build_keys: BTreeMap<String, KeyWork>,
    unbound: BTreeMap<String, UnboundWork>,
    direct_commands: u64,
    cargo_test_processes: u64,
    cargo_other_processes: u64,
    unobserved_cargo: Vec<&'static str>,
    build_ms: u64,
    units: u64,
    platform: Vec<rust_mutants_sealed::Spent>,
    platform_requests: u64,
    error: Option<String>,
}

/// Cargo command classes the engine knows of that never run under a run's watch, so no record can count them.
const UNOBSERVED_CARGO: [&str; 1] = ["cargo -vV toolchain banners"];

#[derive(Serialize)]
struct Record<'a> {
    schema: &'a str,
    binary: &'a str,
    test: &'a str,
    root: &'a str,
    work: &'a Work,
    sealed: Option<rust_mutants_sealed::Spent>,
}

impl Costs {
    pub(super) fn new(
        vars: &crate::vars::Variables,
        root: &Path,
        sealed: rust_mutants_sealed::Counted,
        prefix: &str,
    ) -> io::Result<Option<Self>> {
        let Some(directory) = vars.var("NJUTEST_TEST_COST_DIR") else {
            return Ok(None);
        };
        let label = |name: &str| {
            vars.var(name)
                .and_then(std::ffi::OsStr::to_str)
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other(format!("cost recording requires UTF-8 {name}")))
        };
        let binary = label("NEXTEST_BINARY_ID")?;
        let test = label("NEXTEST_TEST_NAME")?;
        let root =
            crate::telling::LosslessBytes::new(root.as_os_str().as_encoded_bytes()).to_string();
        std::fs::create_dir_all(directory)?;
        let (output, path) = tempfile::Builder::new()
            .prefix(prefix)
            .suffix(".json")
            .tempfile_in(directory)?
            .keep()
            .map_err(|source| source.error)?;
        drop(path);
        Ok(Some(Self {
            output,
            binary,
            test,
            root,
            sealed,
            work: Mutex::new(Work {
                unobserved_cargo: UNOBSERVED_CARGO.to_vec(),
                ..Work::default()
            }),
        }))
    }

    pub(super) fn sealed(&self) -> rust_mutants_sealed::Counted {
        self.sealed.clone()
    }

    pub(super) fn invalid(&self, detail: String) {
        let Ok(mut work) = self.work.lock() else {
            return;
        };
        work.error = Some(detail);
    }

    pub(super) fn observe(&self, payload: &Payload) {
        let Ok(mut work) = self.work.lock() else {
            return;
        };
        if let Err(detail) = apply(&mut work, payload) {
            work.error = Some(detail.to_string());
        }
    }
}

/// Why the cost fold could not keep an exact count.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
enum AccountingError {
    /// A counter reached the width of its field.
    #[error("{what} overflowed")]
    Overflowed {
        /// Which accounting overflowed.
        what: &'static str,
    },
    /// A note did not carry what its kind promises.
    #[error("{problem}")]
    Invalid {
        /// What the note lacked.
        problem: String,
    },
}

/// A complete input key is 64 hexadecimal characters; anything else is an unbound reason.
fn bound(detail: &str) -> bool {
    detail.len() == 64 && detail.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn apply(work: &mut Work, payload: &Payload) -> Result<(), AccountingError> {
    if let Payload::Exec { exec } = payload {
        observed_cargo(work, exec)?;
    }
    if let Payload::Note { note } = payload {
        noted(work, note)?;
    }
    Ok(())
}

/// Classifies one Cargo command the engine observed under this run's watch.
fn observed_cargo(work: &mut Work, exec: &super::ExecRecord) -> Result<(), AccountingError> {
    let cargo = exec
        .argv
        .first()
        .and_then(|program| Path::new(program).file_name())
        .is_some_and(|program| program == "cargo" || program == "cargo.exe");
    if !cargo {
        return Ok(());
    }
    let plain = !exec
        .argv
        .iter()
        .any(|arg| arg == "--message-format=json" || arg.starts_with("--message-format="));
    let first = exec.argv.get(1).map(String::as_str);
    let compile = matches!(first, Some("check" | "build" | "rustc"))
        || (first == Some("test") && exec.argv.iter().any(|arg| arg == "--no-run"));
    let test = first == Some("test") && !exec.argv.iter().any(|arg| arg == "--no-run");
    match (compile, test, plain) {
        (true, _, true) => {
            add(&mut work.builds, "fixture build accounting")?;
            add(&mut work.direct_commands, "direct command accounting")?;
            sum(
                &mut work.build_ms,
                exec.duration_ms,
                "fixture build accounting",
            )?;
        }
        (_, true, true) => {
            add(&mut work.builds, "test process accounting")?;
            add(&mut work.cargo_test_processes, "test process accounting")?;
            sum(
                &mut work.build_ms,
                exec.duration_ms,
                "test process accounting",
            )?;
        }
        (_, _, false) => {}
        (_, _, _) => {
            add(
                &mut work.cargo_other_processes,
                "cargo inventory accounting",
            )?;
        }
    }
    Ok(())
}

/// Folds one diagnostic note into the multiplicity it observes.
fn noted(work: &mut Work, note: &crate::trace::NoteRecord) -> Result<(), AccountingError> {
    match note.kind.as_str() {
        "fixture-cargo-build" => {
            let millis = note
                .detail
                .parse::<u64>()
                .map_err(|source| AccountingError::Invalid {
                    problem: format!("fixture build duration is invalid: {source}"),
                })?;
            add(&mut work.builds, "fixture build accounting")?;
            sum(&mut work.build_ms, millis, "fixture build accounting")?;
        }
        "fixture-build-request"
        | "fixture-build-process"
        | "build-cache-hit"
        | "build-cache-miss"
        | "build-cache-unavailable" => identified(work, note)?,
        "cargo-built-units" => {
            let measured =
                note.detail
                    .parse::<u64>()
                    .map_err(|_invalid| AccountingError::Invalid {
                        problem: "compiled unit accounting is invalid".to_owned(),
                    })?;
            sum(&mut work.units, measured, "compiled unit accounting")?;
        }
        "sealed-platform-request" => {
            add(&mut work.platform_requests, "platform request accounting")?;
        }
        "sealed-platform-work" => {
            let spent = crate::strictjson::decode_str::<rust_mutants_sealed::Spent>(&note.detail)
                .map_err(|source| AccountingError::Invalid {
                problem: source.to_string(),
            })?;
            work.platform.push(spent);
        }
        "sealed-platform-work-invalid" => {
            return Err(AccountingError::Invalid {
                problem: note.detail.clone(),
            });
        }
        _ => {}
    }
    Ok(())
}

/// Folds one identity-carrying note: a request, a process, or a cache answer, each under its key or unbound reason.
fn identified(work: &mut Work, note: &crate::trace::NoteRecord) -> Result<(), AccountingError> {
    match note.kind.as_str() {
        "fixture-build-request" => {
            add(&mut work.build_requests, "build request accounting")?;
            if bound(&note.detail) {
                let held = work.build_keys.entry(note.detail.clone()).or_default();
                held.requests = raised(held.requests, "bound request accounting")?;
            } else {
                let held = work.unbound.entry(note.detail.clone()).or_default();
                held.requests = raised(held.requests, "unbound request accounting")?;
            }
        }
        "fixture-build-process" => {
            if bound(&note.detail) {
                let held = work.build_keys.entry(note.detail.clone()).or_default();
                held.processes = raised(held.processes, "bound process accounting")?;
            } else {
                let held = work.unbound.entry(note.detail.clone()).or_default();
                held.processes = raised(held.processes, "unbound process accounting")?;
            }
        }
        "build-cache-hit" => {
            if !bound(&note.detail) {
                return Err(AccountingError::Invalid {
                    problem: "a cache hit names no complete input key".to_owned(),
                });
            }
            add(&mut work.build_hits, "cache hit accounting")?;
            let held = work.build_keys.entry(note.detail.clone()).or_default();
            held.hits = raised(held.hits, "bound hit accounting")?;
        }
        "build-cache-miss" => {
            add(&mut work.build_misses, "cache miss accounting")?;
            let (identity, reason) =
                note.detail
                    .split_once(' ')
                    .ok_or_else(|| AccountingError::Invalid {
                        problem: "a cache miss names neither a key nor a reason".to_owned(),
                    })?;
            if bound(identity) {
                let held = work.build_keys.entry(identity.to_owned()).or_default();
                held.misses = raised(held.misses, "bound miss accounting")?;
                let counted = held.reasons.entry(reason.to_owned()).or_default();
                *counted = raised(*counted, "miss reason accounting")?;
            } else {
                let held = work.unbound.entry(note.detail.clone()).or_default();
                held.misses = raised(held.misses, "unbound miss accounting")?;
            }
        }
        "build-cache-unavailable" => {
            let (identity, reason) =
                note.detail
                    .split_once(' ')
                    .ok_or_else(|| AccountingError::Invalid {
                        problem: "a refused write names neither a key nor a reason".to_owned(),
                    })?;
            if !bound(identity) {
                return Err(AccountingError::Invalid {
                    problem: "a refused write names no complete input key".to_owned(),
                });
            }
            let held = work.build_keys.entry(identity.to_owned()).or_default();
            let counted = held.refused_writes.entry(reason.to_owned()).or_default();
            *counted = raised(*counted, "refused write accounting")?;
        }
        _ => {}
    }
    Ok(())
}

fn add(count: &mut u64, what: &'static str) -> Result<(), AccountingError> {
    *count = raised(*count, what)?;
    Ok(())
}

fn raised(count: u64, what: &'static str) -> Result<u64, AccountingError> {
    count
        .checked_add(1)
        .ok_or(AccountingError::Overflowed { what })
}

fn sum(count: &mut u64, measured: u64, what: &'static str) -> Result<(), AccountingError> {
    count
        .checked_add(measured)
        .map(|next| *count = next)
        .ok_or(AccountingError::Overflowed { what })
}

impl Drop for Costs {
    fn drop(&mut self) {
        let Ok(work) = self.work.get_mut() else {
            return;
        };
        let record = Record {
            schema: "njutest-test-cost-v2",
            binary: &self.binary,
            test: &self.test,
            root: &self.root,
            work,
            sealed: self.sealed.spent(),
        };
        match serde_json::to_writer(&mut self.output, &record) {
            Ok(()) => match self.output.write_all(b"\n") {
                Ok(()) | Err(_) => {}
            },
            Err(_incomplete_record_is_refused_by_the_cost_gate) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Payload, UNOBSERVED_CARGO, Work, apply};

    fn note(kind: &str, detail: &str) -> Payload {
        Payload::Note {
            note: crate::trace::NoteRecord {
                kind: kind.to_owned(),
                detail: detail.to_owned(),
            },
        }
    }

    #[test]
    fn a_cold_build_then_a_repair_each_keep_their_reason_and_their_process() {
        let key = "ab".repeat(32);
        let mut work = Work {
            unobserved_cargo: UNOBSERVED_CARGO.to_vec(),
            ..Work::default()
        };
        for reason in [
            "cold: the compilation record is absent",
            "repair: the recorded artifact changed",
        ] {
            apply(&mut work, &note("fixture-build-request", &key)).unwrap();
            apply(
                &mut work,
                &note("build-cache-miss", &format!("{key} {reason}")),
            )
            .unwrap();
            apply(&mut work, &note("fixture-build-process", &key)).unwrap();
            apply(&mut work, &note("fixture-cargo-build", "10")).unwrap();
        }
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(held.requests, 2);
        assert_eq!(held.misses, 2);
        assert_eq!(held.processes, 2);
        assert_eq!(held.reasons.get(repair_reason()).copied(), Some(1));
        assert_eq!(work.builds, 2);
        assert_eq!(work.build_misses, 2);
    }

    #[test]
    fn a_hit_keeps_its_multiplicity_without_a_process() {
        let key = "cd".repeat(32);
        let mut work = Work::default();
        for _ in 0..2 {
            apply(&mut work, &note("fixture-build-request", &key)).unwrap();
            apply(&mut work, &note("build-cache-hit", &key)).unwrap();
        }
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(held.requests, 2);
        assert_eq!(held.hits, 2);
        assert_eq!(held.processes, 0);
    }

    #[test]
    fn an_unbound_command_never_gains_a_fabricated_key() {
        let mut work = Work::default();
        apply(
            &mut work,
            &note("fixture-build-request", "unbound: inherited environment"),
        )
        .unwrap();
        apply(
            &mut work,
            &note("build-cache-miss", "unbound: inherited environment"),
        )
        .unwrap();
        apply(
            &mut work,
            &note("fixture-build-process", "unbound: inherited environment"),
        )
        .unwrap();
        apply(&mut work, &note("fixture-cargo-build", "5")).unwrap();
        assert!(work.build_keys.is_empty());
        let held = work.unbound.get("unbound: inherited environment").unwrap();
        assert_eq!((held.requests, held.misses, held.processes), (1, 1, 1));
    }

    #[test]
    fn a_direct_build_publishes_its_reason_without_a_miss() {
        let mut work = Work::default();
        let identity = "direct: native edit oracle build";
        apply(&mut work, &note("fixture-build-request", identity)).unwrap();
        apply(&mut work, &note("fixture-build-process", identity)).unwrap();
        let held = work.unbound.get(identity).unwrap();
        assert_eq!((held.requests, held.misses, held.processes), (1, 0, 1));
    }

    #[test]
    fn a_refused_write_is_attributed_to_its_key() {
        let key = "ef".repeat(32);
        let mut work = Work::default();
        apply(
            &mut work,
            &note(
                "build-cache-unavailable",
                &format!("{key} the record could not be written"),
            ),
        )
        .unwrap();
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(
            held.refused_writes
                .get("the record could not be written")
                .copied(),
            Some(1),
        );
    }

    #[test]
    fn a_hit_or_a_refused_write_without_a_complete_key_is_refused() {
        let mut work = Work::default();
        apply(&mut work, &note("build-cache-hit", "not a key")).unwrap_err();
        apply(
            &mut work,
            &note("build-cache-unavailable", "also not a key no reason"),
        )
        .unwrap_err();
    }

    fn repair_reason() -> &'static str {
        "repair: the recorded artifact changed"
    }
}
