// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest merge`: the report the whole catalog would have written, from the reports of its parts.

use std::io::Write;
use std::path::Path;

use crate::cli::{EXIT_ERROR, Merge as Arguments};
use crate::report::Report;
use crate::report::merge::merge;

/// Combines the parts and writes the whole.
pub fn run(arguments: &Arguments, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let mut parts = Vec::new();
    for path in &arguments.reports {
        match read(path) {
            Ok(part) => parts.push(part),
            Err(said) => {
                super::diagnose(stderr, &said);
                return EXIT_ERROR;
            }
        }
    }
    let whole = match merge(&parts) {
        Ok(whole) => whole,
        Err(refused) => {
            super::diagnose(stderr, &refused.to_string());
            return EXIT_ERROR;
        }
    };
    let document = match crate::report::json::document(&whole) {
        Ok(document) => document,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    if let Some(path) = &arguments.output {
        if let Err(error) = std::fs::write(path, &document) {
            super::diagnose(stderr, &format!("{}: {error}", path.display()));
            return EXIT_ERROR;
        }
    } else {
        super::say(stdout, document.trim_end());
    }
    whole.verdict.exit_code()
}

/// One part, read from the file a person named.
fn read(path: &Path) -> Result<Report, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}
