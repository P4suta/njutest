// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::time::Duration;

use super::{BEATS_PER_WINDOW, SHORTEST_BEAT, Stillness, beat_every};

const WINDOWS: [u64; 12] = [0, 1, 2, 3, 4, 5, 7, 8, 100, 500, 5_000, 3_600_000];

#[test]
fn a_beat_is_a_quarter_of_the_window_and_never_under_a_millisecond() {
    assert_eq!(BEATS_PER_WINDOW, 4);
    assert_eq!(SHORTEST_BEAT, Duration::from_millis(1));
    for (millis, every) in [
        (0, 1_000_000),
        (1, 1_000_000),
        (3, 1_000_000),
        (4, 1_000_000),
        (5, 1_250_000),
        (100, 25_000_000),
        (500, 125_000_000),
        (5_000, 1_250_000_000),
    ] {
        assert_eq!(
            beat_every(Duration::from_millis(millis)),
            Duration::from_nanos(every),
            "a {millis} ms window"
        );
    }
}

#[test]
fn a_process_that_beats_as_told_is_never_still_for_a_window() {
    for millis in WINDOWS {
        let quiet = Duration::from_millis(millis);
        let every = beat_every(quiet);
        assert!(
            every >= SHORTEST_BEAT && (every * 2 <= quiet || every >= quiet),
            "a beat every {every:?} in a {quiet:?} window"
        );
        if every * 2 > quiet {
            continue;
        }
        let mut watch = Stillness::new(quiet);
        let mut beat = Duration::ZERO;
        for _ in 0..16 {
            let before_the_next = beat
                .checked_add(every)
                .and_then(|next| next.checked_sub(Duration::from_nanos(1)))
                .expect("a beat within a day");
            assert!(
                !watch.still_for_the_window(before_the_next),
                "a process that beat at {beat:?} is not stalled at {before_the_next:?} in a \
                 {quiet:?} window"
            );
            beat = beat.checked_add(every).expect("a beat within a day");
            watch = watch.looked(beat, true);
        }
    }
}

#[test]
fn a_look_that_saw_a_change_moves_the_stall_and_one_that_saw_none_does_not() {
    let quiet = Duration::from_millis(500);
    let watch = Stillness::new(quiet);
    assert_eq!(watch.stalls_at(), Some(quiet));
    let unchanged = watch.looked(Duration::from_millis(300), false);
    assert_eq!(unchanged, watch);
    assert_eq!(unchanged.stalls_at(), Some(quiet));
    let moved = watch.looked(Duration::from_millis(300), true);
    assert_eq!(moved.stalls_at(), Some(Duration::from_millis(800)));
    assert_ne!(moved, watch);
}

#[test]
fn a_stall_is_the_whole_window_still_and_nothing_less() {
    let quiet = Duration::from_millis(500);
    let watch = Stillness::new(quiet).looked(Duration::from_millis(200), true);
    assert!(!watch.still_for_the_window(Duration::from_millis(100)));
    assert!(!watch.still_for_the_window(Duration::from_millis(200)));
    assert!(!watch.still_for_the_window(Duration::from_millis(699)));
    assert!(watch.still_for_the_window(Duration::from_millis(700)));
    assert!(watch.still_for_the_window(Duration::from_millis(701)));
}

#[test]
fn a_stall_that_cannot_be_said_is_none() {
    let watch = Stillness::new(Duration::MAX).looked(Duration::from_secs(1), true);
    assert_eq!(watch.stalls_at(), None);
    assert!(!watch.still_for_the_window(Duration::MAX));
}
