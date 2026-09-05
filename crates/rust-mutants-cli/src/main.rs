// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `rust-mutants` binary: the composition root, and the only place that reads the process's arguments, streams, environment, and signals.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_mutants::runner::Cancel;
use rust_mutants_cli::Environment;
use signal_hook::consts::{SIGINT, SIGTERM};

fn main() -> ExitCode {
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let environment = Environment {
        temp_directory: std::env::temp_dir(),
        cache_directory: Environment::cache_directory_of(&vars),
        working_directory: std::env::current_dir().unwrap_or_else(|_error| PathBuf::from(".")),
        vars,
    };
    let cancel = Cancel::new();
    let signalled = Arc::new(AtomicUsize::new(0));
    for signal in [SIGINT, SIGTERM] {
        drop(signal_hook::flag::register(signal, cancel.flag()));
        drop(signal_hook::flag::register_usize(
            signal,
            Arc::clone(&signalled),
            usize::try_from(signal).unwrap_or(0),
        ));
    }
    let code = rust_mutants_cli::run_from(
        std::env::args_os(),
        &environment,
        &cancel,
        rust_mutants_cli::Streams {
            out: &mut std::io::stdout().lock(),
            err: &mut std::io::stderr().lock(),
        },
    );
    ExitCode::from(match signalled.load(Ordering::SeqCst) {
        0 => code,
        signal => u8::try_from(signal)
            .ok()
            .map_or(code, |number| 128u8.saturating_add(number)),
    })
}
