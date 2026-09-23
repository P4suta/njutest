// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The tests of the fixture, one of which can be told to be slow exactly once per mutation, or slow and moving.

use std::path::Path;
use std::time::Duration;

/// Where a marker of "this mutation has already been slow once" is kept.
const MARKER: &str = "FIXTURE_HANG_MARKER";

/// How long to be, in milliseconds.
const PAUSE: &str = "FIXTURE_HANG_PAUSE_MS";

/// How far apart, in milliseconds, a slow test passes through the mutated function.
const STRIDE: &str = "FIXTURE_HANG_STRIDE_MS";

/// How many times a slow test passes through the mutated function.
const STRIDES: u32 = 40;

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

/// Passes through `clamp_positive` again and again, a stride apart, while a mutation is active.
///
/// Every pass takes the active mutant's guard, so the run can see the test moving the whole time it is slow.
/// Without `FIXTURE_HANG_STRIDE_MS`, or with nothing active, this does nothing at all.
fn slow_but_moving() {
    let Some(milliseconds) = std::env::var(STRIDE)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return;
    };
    if std::env::var_os("RUST_MUTANTS_ACTIVE").is_none() {
        return;
    }
    for _ in 0..STRIDES {
        std::hint::black_box(fixture_hang::clamp_positive(std::hint::black_box(1)));
        std::thread::sleep(Duration::from_millis(milliseconds));
    }
}

#[test]
fn counting_to_four_sums_every_step_below_it() {
    assert_eq!(fixture_hang::count_to(4), 6);
    assert_eq!(fixture_hang::count_to(0), 0);
}

#[test]
fn clamping_keeps_a_positive_and_floors_everything_else() {
    slow_once();
    slow_but_moving();
    assert_eq!(fixture_hang::clamp_positive(3), 3);
    assert_eq!(fixture_hang::clamp_positive(-3), 0);
    assert_eq!(fixture_hang::clamp_positive(0), 0);
}
