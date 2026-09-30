// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the generated runtime said on its way out of a process it ended, read from the last line it wrote: the status, the check that failed, and the operating system's code.

/// The first field of the line the runtime writes to standard error before it ends a process, followed by the status, the check that failed, and the operating system's code, `0` where no call is what failed.
pub const STOP_SCHEMA: &str = "rust-mutants-stop-v1";

/// Every check of the step protocol the runtime of this release says it stopped for, in its own words.
pub const STEP_CHECKS: [&str; 35] = [
    "allowance: not Unicode",
    "allowance: not a number",
    "allowance: not canonical",
    "allowance: no room past it",
    "no state path",
    "no nonce",
    "no active mutant",
    "poisoned",
    "metadata",
    "not a regular file",
    "open",
    "lock",
    "count",
    "unlock",
    "seek",
    "read",
    "too large",
    "not UTF-8",
    "not canonical",
    "a field too many",
    "another schema",
    "another execution's nonce",
    "another mutant",
    "phase",
    "truncate",
    "write",
    "sync",
    "notice: no path",
    "notice: no nonce",
    "notice: no active mutant",
    "notice: create",
    "notice: write",
    "notice: sync",
    "notice: publish",
    "unstated",
];

/// The runtime's stop record: the status it ended the process with, the check that failed, and the operating system's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record<'output> {
    /// The status it ended the process with.
    pub status: i32,
    /// The check that failed, in the runtime's words.
    pub check: &'output str,
    /// The operating system's code where a call the runtime made is what failed, and `0` where none did.
    pub os: i32,
}

/// The runtime's stop record, where it is the final complete line of `output` and each of its numbers is spelled as the runtime spells one.
#[must_use]
pub fn record(output: &[u8]) -> Option<Record<'_>> {
    let complete = output.strip_suffix(b"\n")?;
    let last = complete
        .rsplit(|byte| *byte == b'\n')
        .next()
        .unwrap_or(complete);
    let Ok(line) = core::str::from_utf8(last) else {
        return None;
    };
    let line = line.strip_suffix('\r').unwrap_or(line);
    let fields = line.strip_prefix(STOP_SCHEMA)?.strip_prefix('\t')?;
    let (status, rest) = fields.split_once('\t')?;
    let (check, os) = rest.split_once('\t')?;
    Some(Record {
        status: canonical(status)?,
        check,
        os: canonical(os)?,
    })
}

/// Whether `check` is one the runtime of this release stops a step protocol for.
#[must_use]
pub fn step_check(check: &str) -> bool {
    STEP_CHECKS.contains(&check)
}

/// What a process that ended with `status` said failed, where its final line is the stop record of a step-protocol check this release knows, and nothing where it said nothing this release reads.
#[must_use]
pub fn stated(output: &[u8], status: i32) -> Option<Record<'_>> {
    match record(output) {
        Some(said) if names(said, status) => Some(said),
        Some(_) | None => None,
    }
}

/// Whether a parsed record names this status and a check this release knows.
fn names(said: Record<'_>, status: i32) -> bool {
    said.status == status && step_check(said.check)
}

/// The number `field` spells, where it spells it as the runtime writes one: an optional minus and then digits, with no leading zero and never a minus zero.
fn canonical(field: &str) -> Option<i32> {
    let digits = match field.strip_prefix('-') {
        Some(digits) => digits,
        None => field,
    };
    let spelled = !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && field != "-0";
    if !spelled {
        return None;
    }
    match field.parse::<i32>() {
        Ok(number) => Some(number),
        Err(_unrepresentable) => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
