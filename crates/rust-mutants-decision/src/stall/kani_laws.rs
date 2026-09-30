// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::time::Duration;

use super::{SHORTEST_BEAT, Stillness, beat_every};

fn symbolic_duration() -> Duration {
    let duration = kani::any::<Duration>();
    kani::assume(duration.subsec_nanos() < 1_000_000_000);
    duration
}

#[kani::proof]
fn a_look_that_saw_no_change_moves_nothing_and_one_that_did_restarts_the_watch() {
    let quiet = symbolic_duration();
    let watch = Stillness::new(quiet);
    let now = symbolic_duration();
    kani::assert(
        watch.looked(now, false) == watch,
        "njutest-law-assertion:unchanged-look-still",
    );
    let moved = watch.looked(now, true);
    kani::assert(
        moved.stalls_at() == now.checked_add(quiet),
        "njutest-law-assertion:change-restarts",
    );
    kani::cover!(now.checked_add(quiet).is_some(), "njutest-law-branch:said");
    kani::cover!(
        now.checked_add(quiet).is_none(),
        "njutest-law-branch:unsayable"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_process_that_moves_within_its_window_is_never_stalled() {
    let quiet = symbolic_duration();
    kani::assume(!quiet.is_zero());
    let moved = symbolic_duration();
    let watch = Stillness::new(quiet).looked(moved, true);
    let now = symbolic_duration();
    kani::assume(watch.stalls_at().is_none_or(|stalls_at| now < stalls_at));
    kani::assert(
        !watch.still_for_the_window(now),
        "njutest-law-assertion:moving-within-the-window",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_quiet_window_is_never_told_to_beat_faster_than_the_floor() {
    let quiet = symbolic_duration();
    let beat = beat_every(quiet);
    kani::assert(
        beat >= SHORTEST_BEAT,
        "njutest-law-assertion:never-faster-than-the-floor",
    );
    let share = quiet.checked_div(4).expect("four is nonzero");
    kani::assert(
        beat == share.max(SHORTEST_BEAT),
        "njutest-law-assertion:a-quarter-of-the-quiet",
    );
    kani::cover!(share < SHORTEST_BEAT, "njutest-law-branch:clamped");
    kani::cover!(share >= SHORTEST_BEAT, "njutest-law-branch:shared");
    kani::cover!(true, "njutest-law-reached");
}
