// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the environment a fixture run is insulated from still names what the engine refuses on.

#[test]
fn the_devkit_strips_every_variable_the_engine_refuses_coverage_for() {
    for named in [
        rust_mutants::cargo::config::RUSTFLAGS,
        rust_mutants::cargo::config::ENCODED_RUSTFLAGS,
    ] {
        assert!(
            njutest_devkit::paths::NOT_INHERITED.contains(&named),
            "the engine refuses to reach into code compiled with {named} set, and reports what \
             it could not probe as refused rather than killed. A fixture run that inherits it \
             from whoever started the suite is a measurement of the environment: {:?}",
            njutest_devkit::paths::NOT_INHERITED
        );
    }
}
