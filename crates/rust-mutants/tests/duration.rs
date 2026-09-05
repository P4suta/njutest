// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The spelling of a duration, which is the one both command lines read from a configuration file.

use std::time::Duration;

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
        })
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
