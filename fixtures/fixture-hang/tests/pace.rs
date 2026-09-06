// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The tests of the fixture, one of which can be told to be slow exactly once per mutation.

use std::path::Path;
use std::time::Duration;

/// Where a marker of "this mutation has already been slow once" is kept.
const MARKER: &str = "FIXTURE_HANG_MARKER";

/// How long to be, in milliseconds.
const PAUSE: &str = "FIXTURE_HANG_PAUSE_MS";

/// Sleeps once per activation, and never again.
///
/// A run believes a timeout only after it reproduces on its own, so a
/// mutation that is slow the first time and quick the second is what leaves a
/// run undecided. Without `FIXTURE_HANG_MARKER` this does nothing at all, so
/// the fixture's ordinary fates are the ordinary ones.
fn slow_once() {
    let Ok(directory) = std::env::var(MARKER) else {
        return;
    };
    let milliseconds: u64 = std::env::var(PAUSE)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    let active = std::env::var("RUST_MUTANTS_ACTIVE").unwrap_or_else(|_error| "none".to_owned());
    let marker = Path::new(&directory).join(format!("paused-{active}"));
    if marker.exists() {
        return;
    }
    drop(std::fs::create_dir_all(&directory));
    drop(std::fs::write(&marker, b"once"));
    std::thread::sleep(Duration::from_millis(milliseconds));
}

#[test]
fn counting_to_four_sums_every_step_below_it() {
    assert_eq!(fixture_hang::count_to(4), 6);
    assert_eq!(fixture_hang::count_to(0), 0);
}

#[test]
fn clamping_keeps_a_positive_and_floors_everything_else() {
    slow_once();
    assert_eq!(fixture_hang::clamp_positive(3), 3);
    assert_eq!(fixture_hang::clamp_positive(-3), 0);
    assert_eq!(fixture_hang::clamp_positive(0), 0);
}
