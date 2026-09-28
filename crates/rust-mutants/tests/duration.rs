// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The spelling of a duration, which is the one both command lines read from a configuration file.

use std::time::Duration;

use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::duration::{DurationError, parse, render};

#[test]
fn a_duration_is_a_run_of_numbers_each_with_a_unit() {
    let cases: [(&str, Duration); 9] = [
        ("0s", Duration::ZERO),
        ("500ms", Duration::from_millis(500)),
        ("30s", Duration::from_secs(30)),
        ("5m", Duration::from_mins(5)),
        ("2h", Duration::from_hours(2)),
        ("1h30m", Duration::from_mins(90)),
        ("1m30s500ms", Duration::from_millis(90_500)),
        ("250us", Duration::from_micros(250)),
        ("100ns", Duration::from_nanos(100)),
    ];
    for (text, expected) in cases {
        assert_eq!(parse(text), Ok(expected), "{text}");
    }
    assert_eq!(parse("250µs"), Ok(Duration::from_micros(250)));
}

#[test]
fn anything_that_is_not_a_duration_is_named_rather_than_guessed_at() {
    assert_eq!(parse(""), Err(DurationError::Empty));
    assert_eq!(
        parse("ms"),
        Err(DurationError::UnitWithoutNumber {
            text: "ms".to_owned()
        })
    );
    assert_eq!(
        parse("30"),
        Err(DurationError::NumberWithoutUnit {
            text: "30".to_owned()
        })
    );
    assert_eq!(
        parse("30x"),
        Err(DurationError::UnknownUnit {
            text: "30x".to_owned(),
            unit: "x".to_owned()
        })
    );
    assert_eq!(
        parse("99999999999999999999s"),
        Err(DurationError::TooLarge {
            text: "99999999999999999999s".to_owned()
        }),
        "a number no integer holds"
    );
    assert_eq!(
        parse("5000000000s"),
        Ok(Duration::from_secs(5_000_000_000)),
        "while a number wider than 32 bits is a duration like any other, because the width it \
         is multiplied in is how a duration is computed and not how long it is"
    );
    for error in [
        DurationError::Empty,
        DurationError::UnitWithoutNumber {
            text: "ms".to_owned(),
        },
        DurationError::NumberWithoutUnit {
            text: "30".to_owned(),
        },
        DurationError::UnknownUnit {
            text: "30x".to_owned(),
            unit: "x".to_owned(),
        },
        DurationError::TooLarge {
            text: "1s".to_owned(),
        },
    ] {
        assert!(!error.to_string().is_empty());
        assert_eq!(rust_mutants::EngineError::from(error).code().code, "RM9003");
    }
}

#[test]
fn rendering_a_duration_produces_text_that_parses_back_to_it() {
    let cases: [(Duration, &str); 7] = [
        (Duration::ZERO, "0s"),
        (Duration::from_nanos(7), "7ns"),
        (Duration::from_micros(250), "250us"),
        (Duration::from_millis(500), "500ms"),
        (Duration::from_secs(30), "30s"),
        (Duration::from_mins(5), "5m"),
        (Duration::from_mins(90), "1h30m"),
    ];
    for (value, text) in cases {
        assert_eq!(render(value), text, "{value:?}");
        assert_eq!(parse(&render(value)), Ok(value), "{value:?}");
    }
}

#[test]
fn a_duration_is_refused_for_its_length_and_never_for_its_spelling() {
    let crash = include_bytes!(
        "../../../fuzz/regressions/duration/crash-bdc7aa6e80adbcc05c913bad7641572f8d2f324c"
    );
    let text = std::str::from_utf8(crash).unwrap_or_else(|error| panic!("the crash: {error}"));
    let read = parse(text);
    let Ok(read) = read else {
        panic!("{text:?} sums two hour counts a duration holds: {read:?}");
    };
    let rendered = render(read);
    assert_eq!(
        parse(&rendered),
        Ok(read),
        "what a scheduled fuzz run found: {text:?} reads as {read:?} and renders as \
         {rendered:?}, one count of hours larger than either it was summed from"
    );
    for (value, why) in [
        (
            Duration::from_secs(5_000_000_000),
            "more seconds than 32 bits count",
        ),
        (Duration::MAX, "the longest duration there is"),
    ] {
        assert_eq!(parse(&render(value)), Ok(value), "{why}");
    }
    let past = format!("{}1ns", render(Duration::MAX));
    assert_eq!(
        parse(&past),
        Err(DurationError::TooLarge { text: past.clone() }),
        "a nanosecond past the longest duration is too long rather than the longest duration"
    );
    let summed = "18446744073709551615s1s";
    assert_eq!(
        parse(summed),
        Err(DurationError::TooLarge {
            text: summed.to_owned()
        }),
        "and so is a sum past it, which a saturating total rounded down to it"
    );
}

proptest::proptest! {
    /// Every duration there is renders as text the parser reads back as itself, which a law over the bounds people write once left out.
    #[test]
    fn what_the_parser_renders_it_reads_back_as_the_same_duration(
        seconds in proptest::num::u64::ANY,
        nanos in 0u32..1_000_000_000,
    ) {
        let value = Duration::new(seconds, nanos);
        let rendered = render(value);
        let again = parse(&rendered);
        proptest::prop_assert_eq!(result_state(&again), Returned, "rendered duration: {:?}", again);
        let Ok(again) = again else { return Ok(()) };
        proptest::prop_assert_eq!(again, value, "{:?} rendered as {}", value, rendered);
    }

    /// Nothing a person can type makes the parser panic, and what it accepts renders and reads back.
    #[test]
    fn no_spelling_makes_the_parser_panic_and_an_accepted_one_round_trips(
        text in "[0-9a-zA-Z ._+-]{0,24}"
    ) {
        let Ok(value) = parse(&text) else {
            return Ok(());
        };
        let rendered = render(value);
        let again = parse(&rendered);
        proptest::prop_assert_eq!(result_state(&again), Returned, "rendered duration: {:?}", again);
        let Ok(again) = again else { return Ok(()) };
        proptest::prop_assert_eq!(again, value, "{:?} rendered as {}", text, rendered);
    }
}
