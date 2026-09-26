// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `cargo-njutest` binary: the same program under the name cargo looks for, so `cargo njutest` is `njutest`.

#![forbid(unsafe_code)]

use std::process::ExitCode;

pub(crate) fn main() -> ExitCode {
    let vars: rust_mutants::vars::Variables = std::env::vars_os().collect();
    let (cancel, signalled) = match njutest::interruptible() {
        Ok(interruptible) => interruptible,
        Err(error) => {
            eprintln!("cargo-njutest: cannot install the cancellation handlers: {error}");
            return ExitCode::from(2);
        }
    };
    let working_directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("cargo-njutest: cannot read the working directory: {error}");
            return ExitCode::from(2);
        }
    };
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            eprintln!("cargo-njutest: cannot locate its executable: {error}");
            return ExitCode::from(2);
        }
    };
    let asked = match njutest::asked(&vars) {
        Ok(asked) => asked,
        Err(error) => {
            eprintln!("cargo-njutest: cannot read the terminal environment: {error}");
            return ExitCode::from(2);
        }
    };
    let environment = njutest::cli::Environment {
        working_directory,
        temp_directory: std::env::temp_dir(),
        program,
        cache_directory: njutest::cli::Environment::cache_directory_of(&vars),
        terminal: njutest::presentation::Terminal::of(&asked),
        vars,
        cancel,
    };
    let code = njutest::run_from(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    njutest::ended(code, &signalled)
}
