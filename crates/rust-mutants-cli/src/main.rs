// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `rust-mutants` binary: the composition root, and the only place that reads the process's arguments, streams, environment, and signals.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use rust_mutants_cli::Environment;

pub(crate) fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let interrupt = match rust_mutants_cli::interruptible() {
        Ok(interruptible) => interruptible,
        Err(error) => {
            eprintln!("rust-mutants: cannot install the cancellation handlers: {error}");
            return ExitCode::from(2);
        }
    };
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            eprintln!("rust-mutants: cannot identify the running executable: {error}");
            return ExitCode::from(2);
        }
    };
    let working_directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("rust-mutants: cannot read the working directory: {error}");
            return ExitCode::from(2);
        }
    };
    let environment = Environment {
        temp_directory: std::env::temp_dir(),
        program,
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory,
        no_color: Environment::no_color_of(&vars),
        stdout_is_terminal: std::io::IsTerminal::is_terminal(&std::io::stdout()),
        paints: false,
        cargo: None,
        ci: Environment::ci_host_of(&vars),
        vars,
    };
    let code = rust_mutants_cli::run_from_compiled(
        std::env::args_os(),
        rust_mutants_cli::Composition::new(
            &environment,
            option_env!("RUST_MUTANTS_COMPILED_CATALOG"),
        ),
        interrupt.cancel(),
        rust_mutants_cli::Streams {
            out: &mut std::io::stdout().lock(),
            err: &mut std::io::stderr().lock(),
        },
    );
    rust_mutants_cli::ended(code, interrupt.signalled())
}
