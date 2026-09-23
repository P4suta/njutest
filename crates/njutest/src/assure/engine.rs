// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one place njutest answers every switch the engine takes, so a switch the engine adds is a line somebody here has to write rather than a default that arrives with a release.

use rust_mutants::session::{Failing, PrepareOptions, Timeout};

/// Every switch the engine takes, as njutest answers it before a phase says otherwise.
#[must_use]
pub const fn switches() -> PrepareOptions {
    PrepareOptions {
        tier: rust_mutants::rule::Tier::Balanced,
        operators: Vec::new(),
        scratch_working_directory: false,
        include: Vec::new(),
        exclude: Vec::new(),
        packages: Vec::new(),
        skips: Vec::new(),
        measurements: None,
        harness_args: Vec::new(),
        verify: true,
        touch: true,
        failing: Failing::Refuse,
        coverage: false,
        branch_proofs: true,
        max_rounds: rust_mutants::validate::DEFAULT_MAX_ROUNDS,
        build_timeout: None,
        mutant_timeout: Timeout::Auto,
        mutant_steps: Some(rust_mutants::session::DEFAULT_MUTANT_STEPS),
        doctests: true,
        build: rust_mutants::cargo::BuildConfig {
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target: None,
            profile: None,
            jobs: None,
            debug: false,
        },
        skip_targets: Vec::new(),
        validation_filter: None,
    }
}
