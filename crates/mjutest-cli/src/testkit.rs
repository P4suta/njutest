// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test support this crate's own suite and a sibling's may reach for.
//!
//! Nothing in production imports this: `cargo xtask devgates` refuses it.

#![expect(
    clippy::unreachable,
    reason = "three of these samples are made by asking the library to refuse something it always refuses; a sample that came back as a success would mean the library stopped refusing it, which is a failure to report loudly rather than to carry"
)]

use std::path::Path;

use crate::error::RunnerError;

/// One failure of every shape this runner reports.
///
/// The list is held to the enum by the `match` below, which names every
/// variant and has no catch-all: a failure added without a sample here does
/// not compile. That is the point. What this is for — checking that every
/// code a run can print is one `docs/errors.md` explains — is only as good as
/// the list being every one, and a list somebody maintains by hand is a list
/// that is one behind.
#[must_use]
pub fn every_failure() -> Vec<RunnerError> {
    let nowhere = Path::new("nowhere");
    let failures = vec![
        RunnerError::Interrupted,
        RunnerError::Config(
            crate::config::Config::parse("version = 9\n", nowhere)
                .err()
                .unwrap_or_else(|| unreachable!("nine is not a version this release knows")),
        ),
        RunnerError::Target(crate::targets::TargetError::new(
            crate::targets::TargetErrorKind::ListFailed,
            "pkg/lib/pkg",
            "the binary said nothing",
        )),
        RunnerError::Evidence(crate::evidence::tree::ScanError::Unreadable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Cache(crate::cache::store::CacheError::Refused {
            message: "a report with no identity answers for no inputs".to_owned(),
        }),
        RunnerError::Coverage(
            rust_mutants::coverage::parse_export(b"not an export")
                .err()
                .unwrap_or_else(|| unreachable!("that is not an export"))
                .into(),
        ),
        RunnerError::Provider(crate::provider::ProviderError::new(
            crate::provider::ProviderErrorKind::Unstartable,
            "no such command",
        )),
        RunnerError::MiriMissing {
            message: "the toolchain has no miri".to_owned(),
        },
        RunnerError::Resource(crate::resource::ResourceError::EnvironmentRefused {
            capability: "postgres".to_owned(),
            name: "RUSTFLAGS".to_owned(),
        }),
        RunnerError::Report(
            crate::report::json::parse("{}")
                .err()
                .unwrap_or_else(|| unreachable!("an empty object is not a report")),
        ),
        RunnerError::Scratch(crate::scratch::ScratchError::Unusable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Build(crate::build::BuildError::NotRun {
            message: "cargo would not start".to_owned(),
        }),
        RunnerError::Engine(rust_mutants::EngineError::Interrupted),
    ];
    for one in &failures {
        match one {
            RunnerError::Interrupted
            | RunnerError::Config(_)
            | RunnerError::Target(_)
            | RunnerError::Evidence(_)
            | RunnerError::Cache(_)
            | RunnerError::Coverage(_)
            | RunnerError::Provider(_)
            | RunnerError::MiriMissing { .. }
            | RunnerError::Resource(_)
            | RunnerError::Report(_)
            | RunnerError::Scratch(_)
            | RunnerError::Build(_)
            | RunnerError::Engine(_) => {}
        }
    }
    failures
}
