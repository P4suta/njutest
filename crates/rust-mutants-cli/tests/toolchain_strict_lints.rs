// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That what the engine plants compiles under every lint a project may deny.

use njutest_devkit::fixture::Fixture;
use std::path::Path;

#[test]
fn what_the_engine_plants_passes_every_lint_a_project_may_deny() {
    let fixture = Fixture::copy("fixture-strict-lints");
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env("NO_COLOR", "1");
    command.envs(njutest_devkit::paths::temporary_directory(fixture.temp()));
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", njutest_devkit::paths::utf8(fixture.root())]);
    command.args(["--tier", "all", "--offline", "--locked"]);
    let output = command.output().expect("rust-mutants runs");
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        output.status.code().is_some_and(|code| code < 2) && !said.contains("RM4001"),
        "a project that denies a lint the engine's own code trips could not be measured at all: \
         the guards, the checkpoints and the runtime module compile as the project's code does, \
         so a module that glob-imports its parent must not see a qualified call it calls \
         unnecessary. {said}"
    );
}
