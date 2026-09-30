// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A receipt of a sealed mutation run over the module that decides one critical decision, which the registry's Mutation cell names (docs/invariants.md).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::gates::GateError;

/// The schema every receipt names.
pub const SCHEMA: &str = "njutest-mutation-receipt-v1";

/// Where receipts are kept, relative to the repository root, and the prefix a Mutation cell names one by.
pub const DIRECTORY: &str = "xtask/receipts";

/// What a sealed run of rust-mutants established about every mutant of one module.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// The registry row the run holds.
    pub decision: String,
    /// The package the run measured.
    pub package: String,
    /// The module that decides, relative to the repository root.
    pub module: String,
    /// The SHA-256 of the module's bytes when the run measured it.
    pub source_sha256: String,
    /// Every mutant of the module, as the run reported it.
    pub mutants: Vec<Mutant>,
}

/// One mutant of the module and what its sealed executions established.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutant {
    /// The identity a person types.
    pub display_id: String,
    /// Where it is, as `line:column`.
    pub position: String,
    /// The operator that made it.
    pub rule: String,
    /// The outcome the run reported.
    pub outcome: String,
    /// Whether a claim of the repository accepts it as it stands.
    pub accepted: bool,
    /// The sealed executions its outcome rests on, in the order they ran; none for a lead.
    pub executions: Vec<Execution>,
}

/// One sealed execution a mutant's outcome rests on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    /// The target whose sealed module ran.
    pub target: String,
    /// The test it ran.
    pub test: String,
    /// What it came to.
    pub came_to: String,
}

/// The endings of a sealed execution that detect the mutant.
const DETECTIONS: [&str; 6] = [
    "panicked",
    "failed",
    "trapped",
    "fuel-exceeded",
    "memory-exceeded",
    "declined",
];

/// The SHA-256 of `bytes`, as lowercase hex.
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The receipt at `name` under `root`, held to the module it names.
///
/// The module's bytes are the ones the run measured, the run measured something, and every mutant is killed by a sealed execution or accepted by a claim.
///
/// # Errors
/// A receipt that cannot be read or is not one, one that names another decision, a module that changed since, a run that measured nothing, and the first mutant no sealed detection or claim answers.
pub fn held(root: &Path, name: &str, decision: &str) -> Result<Receipt, GateError> {
    let path = root.join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| GateError(format!("receipt {name}: unreadable: {error}")))?;
    let receipt: Receipt = crate::strictjson::decode_str(&text)
        .map_err(|error| GateError(format!("receipt {name}: not a receipt: {error}")))?;
    if receipt.schema != SCHEMA {
        return Err(GateError(format!(
            "receipt {name}: schema {:?} is not {SCHEMA}",
            receipt.schema
        )));
    }
    if receipt.decision != decision {
        return Err(GateError(format!(
            "receipt {name}: it holds {:?}, and the registry names it for {decision:?}",
            receipt.decision
        )));
    }
    let module = std::fs::read(root.join(&receipt.module)).map_err(|error| {
        GateError(format!(
            "receipt {name}: the module {} is unreadable: {error}",
            receipt.module
        ))
    })?;
    let now = digest(&module);
    if now != receipt.source_sha256 {
        let named = match Path::new(name).file_stem().and_then(OsStr::to_str) {
            Some(stem) if stem != decision => format!(" --name {stem}"),
            Some(_) | None => String::new(),
        };
        return Err(GateError(format!(
            "receipt {name}: {} changed since its run measured it ({} then, {now} now); run \
             `cargo xtask receipt {decision} {} --package {}{named}` again",
            receipt.module, receipt.source_sha256, receipt.module, receipt.package
        )));
    }
    if receipt.mutants.is_empty() {
        return Err(GateError(format!(
            "receipt {name}: the run cataloged no mutant of {}, so it holds nothing",
            receipt.module
        )));
    }
    for mutant in &receipt.mutants {
        let detected = mutant
            .executions
            .iter()
            .any(|execution| DETECTIONS.contains(&execution.came_to.as_str()));
        let killed = mutant.outcome == "killed" && detected;
        if !killed && !mutant.accepted {
            return Err(GateError(format!(
                "receipt {name}: mutant {} ({} at {}) is {} with no sealed detection and no \
                 claim accepting it, so the module's tests leave it unaccounted for",
                mutant.display_id, mutant.rule, mutant.position, mutant.outcome
            )));
        }
    }
    Ok(receipt)
}

/// The file a receipt for `decision` is kept in under [`DIRECTORY`]: the decision's own name, or `name` where the decision rests on several modules, each with a receipt of its own.
///
/// # Errors
/// A name that is not the decision's, or the decision's followed by a hyphen and lowercase words, so that every receipt says which decision it holds.
pub fn file_name(decision: &str, name: Option<&str>) -> Result<String, GateError> {
    let Some(name) = name else {
        return Ok(format!("{decision}.json"));
    };
    let part = name
        .strip_prefix(decision)
        .and_then(|rest| rest.strip_prefix('-'));
    let spelled = part.is_some_and(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });
    if name == decision || spelled {
        Ok(format!("{name}.json"))
    } else {
        Err(GateError(format!(
            "receipt: {name:?} does not say which decision it holds; name it {decision:?} or \
             \"{decision}-\" and lowercase words"
        )))
    }
}

/// Runs the engine built from `root` over `package`, sealed, and writes the receipt of `module`'s mutants for `decision`, under `name` where it is one of several.
///
/// # Errors
/// A name that does not say which decision it holds, an engine that cannot run or reports nothing readable, and a receipt that cannot be written.
pub fn write(
    root: &Path,
    cargo: &OsStr,
    (decision, package, module): (&str, &str, &str),
    name: Option<&str>,
) -> Result<String, GateError> {
    let file = file_name(decision, name)?;
    let document = reported(root, cargo, package)?;
    let mutants = rows_of(&document, package, module)?;
    let bytes = std::fs::read(root.join(module)).map_err(|error| {
        GateError(format!(
            "receipt: the module {module} is unreadable: {error}"
        ))
    })?;
    let receipt = Receipt {
        schema: SCHEMA.to_owned(),
        decision: decision.to_owned(),
        package: package.to_owned(),
        module: module.to_owned(),
        source_sha256: digest(&bytes),
        mutants,
    };
    let directory = root.join(DIRECTORY);
    std::fs::create_dir_all(&directory).map_err(|error| {
        GateError(format!(
            "receipt: {} cannot be made: {error}",
            directory.display()
        ))
    })?;
    let path = directory.join(file);
    let mut written = serde_json::to_string_pretty(&receipt)
        .map_err(|error| GateError(format!("receipt: cannot be written as JSON: {error}")))?;
    written.push('\n');
    std::fs::write(&path, written).map_err(|error| {
        GateError(format!(
            "receipt: {} cannot be written: {error}",
            path.display()
        ))
    })?;
    Ok(format!(
        "receipt: {} holds {} mutant(s) of {module}",
        path.display(),
        receipt.mutants.len()
    ))
}

/// The report a sealed run of the engine built from `root` writes about `package`.
fn reported(root: &Path, cargo: &OsStr, package: &str) -> Result<serde_json::Value, GateError> {
    let temporary = tempfile::tempdir()
        .map_err(|error| GateError(format!("receipt: no temporary directory: {error}")))?;
    let asked = std::process::Command::new(cargo)
        .args([
            "run",
            "--locked",
            "--quiet",
            "--package",
            "rust-mutants-cli",
            "--bin",
            "rust-mutants",
            "--",
            "run",
            "--package",
            package,
            "--tier",
            "all",
            "--offline",
            "--locked",
            "--root",
        ])
        .arg(root)
        .current_dir(root)
        .envs(super::TEMPORARY_VARIABLES.map(|name| (name, temporary.path())))
        .output()
        .map_err(|error| GateError(format!("receipt: cargo run could not start: {error}")))?;
    let said = String::from_utf8(asked.stdout).map_err(|_not_text| {
        GateError("receipt: rust-mutants run printed bytes that are not text".to_owned())
    })?;
    let Some(report) = said
        .lines()
        .find_map(|line| line.strip_prefix("REPORT"))
        .map(|rest| PathBuf::from(rest.trim()))
    else {
        let complaint = match String::from_utf8(asked.stderr) {
            Ok(complaint) => complaint,
            Err(_not_text) => "its diagnostics are not text".to_owned(),
        };
        return Err(GateError(format!(
            "receipt: rust-mutants run over {package} ended {} and named no report:\n{}",
            asked.status,
            complaint.trim()
        )));
    };
    let text = std::fs::read_to_string(&report).map_err(|error| {
        GateError(format!(
            "receipt: the report {} is unreadable: {error}",
            report.display()
        ))
    })?;
    crate::strictjson::from_str(&text).map_err(|error| {
        GateError(format!(
            "receipt: the report {} is not JSON: {error}",
            report.display()
        ))
    })
}

/// Every row of `document` about `module` of `package`, as a receipt keeps it.
fn rows_of(
    document: &serde_json::Value,
    package: &str,
    module: &str,
) -> Result<Vec<Mutant>, GateError> {
    let rows = document
        .get("mutants")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| GateError("receipt: the report lists no mutants".to_owned()))?;
    let mut mutants = Vec::new();
    for row in rows {
        let ours = row.get("package").and_then(serde_json::Value::as_str) == Some(package)
            && row.get("path").and_then(serde_json::Value::as_str) == Some(module);
        if ours {
            mutants.push(mutant_of(row)?);
        }
    }
    Ok(mutants)
}

/// One report row, as a receipt keeps it.
fn mutant_of(row: &serde_json::Value) -> Result<Mutant, GateError> {
    let text = |pointer: &str| -> Result<String, GateError> {
        row.pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| GateError(format!("receipt: a report row has no {pointer}")))
    };
    let number = |pointer: &str| -> Result<u64, GateError> {
        row.pointer(pointer)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| GateError(format!("receipt: a report row has no {pointer}")))
    };
    let accepted = row
        .get("expected")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| GateError("receipt: a report row has no /expected".to_owned()))?;
    let executions = match row.pointer("/evidence/executions") {
        None => Vec::new(),
        Some(listed) => serde_json::from_value(listed.clone()).map_err(|error| {
            GateError(format!(
                "receipt: a row's sealed executions are unreadable: {error}"
            ))
        })?,
    };
    Ok(Mutant {
        display_id: text("/display_id")?,
        position: format!("{}:{}", number("/line")?, number("/column")?),
        rule: text("/rule")?,
        outcome: text("/outcome")?,
        accepted,
        executions,
    })
}
