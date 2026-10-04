// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asks each call once, checking some answers and not others.

use std::path::Path;

#[test]
fn the_manifest_loads() {
    assert!(fixture_faulted::load(Path::new("Cargo.toml")).is_ok());
}

#[test]
fn seven_is_a_number() {
    assert_eq!(fixture_faulted::number("7"), Ok(7));
}

#[test]
fn a_length_is_asked_and_never_checked() {
    let _length = fixture_faulted::length(Path::new("Cargo.toml"));
}

#[test]
fn ours_and_maybe_answer() {
    assert_eq!(fixture_faulted::ours(), Ok(7));
    assert_eq!(fixture_faulted::maybe(Some(3)), Some(3));
}

#[test]
fn a_failed_read_is_waited_out() {
    match fixture_faulted::linger(Path::new("Cargo.toml")) {
        Ok(text) => assert!(text.contains("fixture-faulted"), "the manifest reads"),
        Err(_failed) => wait(),
    }
}

#[cfg(not(target_os = "wasi"))]
fn wait() {
    let directory = std::env::var_os("NJUTEST_TEST_CLOCK").expect("an injected supervision clock");
    let pid = std::process::id();
    let event = Path::new(&directory).join(pid.to_string());
    let pending = Path::new(&directory).join(format!("{pid}.next"));
    std::fs::write(&pending, "60000").expect("one elapsed minute");
    std::fs::rename(&pending, &event).expect("the elapsed minute published whole");
    let acknowledged = event.with_extension("ack");
    while !std::fs::read(&acknowledged).is_ok_and(|value| value == b"60000") {
        std::thread::yield_now();
    }
}

#[cfg(target_os = "wasi")]
fn wait() {
    std::thread::sleep(std::time::Duration::from_secs(60));
}
