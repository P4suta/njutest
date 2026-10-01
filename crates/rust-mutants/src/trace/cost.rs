// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cost counters fed by the same events as a recording, kept independently of verdicts.

use std::collections::BTreeSet;
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

#[derive(Debug, Default, Serialize)]
struct Work {
    builds: u64,
    build_requests: u64,
    build_hits: u64,
    build_misses: u64,
    build_keys: BTreeSet<String>,
    uncacheable: u64,
    build_ms: u64,
    units: u64,
    platform: Vec<rust_mutants_sealed::Spent>,
    platform_requests: u64,
    error: Option<String>,
}

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
            work: Mutex::new(Work::default()),
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
        if let Payload::Exec { exec } = payload {
            let cargo = exec
                .argv
                .first()
                .and_then(|program| Path::new(program).file_name())
                .is_some_and(|program| program == "cargo" || program == "cargo.exe");
            let builds = exec.argv.get(1).is_some_and(|command| {
                matches!(command.as_str(), "check" | "build" | "test" | "rustc")
            });
            if cargo && builds && !exec.argv.iter().any(|arg| arg == "--message-format=json") {
                counted(&mut work, "fixture-build-request");
                counted(&mut work, "fixture-build-uncacheable");
                let counts = work
                    .builds
                    .checked_add(1)
                    .zip(work.build_ms.checked_add(exec.duration_ms));
                match counts {
                    Some((builds, duration)) => {
                        work.builds = builds;
                        work.build_ms = duration;
                    }
                    None => work.error = Some("fixture build accounting overflowed".to_owned()),
                }
            }
        }
        if let Payload::Note { note } = payload
            && note.kind == "fixture-cargo-build"
        {
            match note.detail.parse::<u64>() {
                Ok(millis) => match work
                    .builds
                    .checked_add(1)
                    .zip(work.build_ms.checked_add(millis))
                {
                    Some((builds, duration)) => {
                        work.builds = builds;
                        work.build_ms = duration;
                    }
                    None => work.error = Some("fixture build accounting overflowed".to_owned()),
                },
                Err(source) => work.error = Some(source.to_string()),
            }
        }
        if let Payload::Note { note } = payload {
            counted(&mut work, &note.kind);
            if note.kind == "build-cache-bound" {
                work.build_keys.insert(note.detail.clone());
            }
            if note.kind == "sealed-platform-request" {
                match work.platform_requests.checked_add(1) {
                    Some(requests) => work.platform_requests = requests,
                    None => work.error = Some("platform request accounting overflowed".to_owned()),
                }
            }
            if note.kind == "sealed-platform-work" {
                match crate::strictjson::decode_str::<rust_mutants_sealed::Spent>(&note.detail) {
                    Ok(spent) => work.platform.push(spent),
                    Err(source) => work.error = Some(source.to_string()),
                }
            }
            if note.kind == "sealed-platform-work-invalid" {
                work.error = Some(note.detail.clone());
            }
        }
        if let Payload::Note { note } = payload
            && note.kind == "cargo-built-units"
        {
            match note.detail.parse::<u64>() {
                Ok(measured) => match work.units.checked_add(measured) {
                    Some(units) => work.units = units,
                    None => work.error = Some("compiled unit accounting overflowed".to_owned()),
                },
                Err(_invalid) => {
                    work.error = Some("compiled unit accounting is invalid".to_owned());
                }
            }
        }
    }
}

fn counted(work: &mut Work, kind: &str) {
    let count = match kind {
        "fixture-build-request" => &mut work.build_requests,
        "fixture-build-uncacheable" => &mut work.uncacheable,
        "build-cache-hit" => &mut work.build_hits,
        "build-cache-miss" => &mut work.build_misses,
        _ => return,
    };
    match count.checked_add(1) {
        Some(next) => *count = next,
        None => work.error = Some(format!("{kind} accounting overflowed")),
    }
}

impl Drop for Costs {
    fn drop(&mut self) {
        let Ok(work) = self.work.get_mut() else {
            return;
        };
        let record = Record {
            schema: "njutest-test-cost-v1",
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
