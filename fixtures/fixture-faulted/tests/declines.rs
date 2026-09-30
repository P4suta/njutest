// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One target whose every test declines after reaching the failed read.

use std::io::Write as _;
use std::path::Path;

#[test]
fn a_failed_read_is_not_measured_here() {
    let _answer = fixture_faulted::refused(Path::new("Cargo.toml"));
    let why = "this machine does not measure a failed read of the manifest";
    println!("skipping: {why}");
    if let Some(path) = std::env::var_os("RUST_MUTANTS_DECLINE_NOTICE") {
        let thread = std::thread::current();
        let name = thread.name().expect("libtest names this test");
        let mut notice = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("the notice the engine named opens");
        notice
            .write_all(format!("{name}\t{why}\n").as_bytes())
            .expect("the decline is written");
    }
}
