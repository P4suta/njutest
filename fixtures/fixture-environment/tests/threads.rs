// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two tests of which one only passes while the other runs beside it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static SIGNALLED: AtomicBool = AtomicBool::new(false);

#[test]
fn a_waits_for_the_signal() {
    let waited = Instant::now();
    while !SIGNALLED.load(Ordering::SeqCst) {
        assert!(waited.elapsed() < Duration::from_secs(2), "nothing signalled");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn b_signals() {
    SIGNALLED.store(true, Ordering::SeqCst);
}
