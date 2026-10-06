// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the environment a fixture run is insulated from still names what the engine refuses on.

#[test]
fn an_opened_fixture_sees_the_loader_path_of_an_in_process_run_without_the_harness_output() {
    let harness = njutest_devkit::paths::harness_output().expect("this test binary's build output");
    let opened = rust_mutants::testkit::opening::opening(
        &njutest_devkit::paths::cargo_binary(),
        &std::env::temp_dir(),
    )
    .env;
    let run: rust_mutants::vars::Variables = njutest_devkit::paths::environment_for_a_run()
        .into_iter()
        .collect();
    for name in [
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
        "PATH",
    ] {
        let value = opened.var(name);
        assert!(
            value.is_none_or(
                |value| std::env::split_paths(value).all(|entry| !entry.starts_with(&harness))
            ),
            "{name} of an opened fixture still names the harness's build output {}, so the engine \
             binds every library image of target/debug/deps on each compile it is asked for: {value:?}",
            harness.display()
        );
        assert_eq!(
            value,
            run.var(name),
            "{name} differs between an opened fixture and an in-process run"
        );
    }
}

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
