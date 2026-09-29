// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::format;
use std::string::String;

use super::{Record, STEP_CHECKS, STOP_SCHEMA, record, stated, step_check};

const PROTOCOL: i32 = 94;

fn line(status: &str, check: &str, os: &str) -> String {
    format!("{STOP_SCHEMA}\t{status}\t{check}\t{os}\n")
}

#[test]
fn the_last_line_the_runtime_wrote_is_the_stop_it_made() {
    let output = format!(
        "running 1 test\n{}{}",
        line("94", "open", "2"),
        line("94", "lock", "33")
    );
    assert_eq!(
        stated(output.as_bytes(), PROTOCOL),
        Some(Record {
            status: PROTOCOL,
            check: "lock",
            os: 33
        }),
        "the last stop the runtime said is the one that ended it"
    );
    assert_eq!(
        stated(line("94", "no state path", "0").as_bytes(), PROTOCOL),
        Some(Record {
            status: PROTOCOL,
            check: "no state path",
            os: 0
        }),
        "an output that is one stop line is that stop"
    );
    assert_eq!(
        stated(
            format!("{STOP_SCHEMA}\t94\tseek\t-5\r\n").as_bytes(),
            PROTOCOL
        ),
        Some(Record {
            status: PROTOCOL,
            check: "seek",
            os: -5
        }),
        "a line a platform ended with a carriage return is read the same"
    );
}

#[test]
fn a_record_for_another_status_or_an_unknown_check_is_read_and_names_no_step_stop() {
    let touch = line("96", "touch: open", "5");
    assert_eq!(
        record(touch.as_bytes()),
        Some(Record {
            status: 96,
            check: "touch: open",
            os: 5
        })
    );
    assert_eq!(stated(touch.as_bytes(), PROTOCOL), None);
    for check in ["", "invented check", "Lock"] {
        let said = line("94", check, "0");
        assert_eq!(
            record(said.as_bytes()).map(|record| record.check),
            Some(check)
        );
        assert_eq!(stated(said.as_bytes(), PROTOCOL), None, "{check:?}");
    }
}

#[test]
fn a_line_out_of_shape_names_nothing() {
    for unread in [
        String::new(),
        String::from("running 1 test\n"),
        format!("{STOP_SCHEMA}\t94\tlock\t33"),
        format!("{STOP_SCHEMA}x\t94\tlock\t33\n"),
        format!("{STOP_SCHEMA}\t94\n"),
        format!("{STOP_SCHEMA}\t94\tlock\n"),
        format!("{STOP_SCHEMA}\t94\tlock\t33\tmore\n"),
        format!("said: {}", line("94", "lock", "33")),
        format!("{}{}", line("94", "open", "2"), "running 1 test\n"),
        String::from("\u{fffd}\n"),
    ] {
        assert_eq!(
            stated(unread.as_bytes(), PROTOCOL),
            None,
            "a line for another stop, or one out of shape, names nothing: {unread:?}"
        );
    }
    assert_eq!(record(b"\xff\n"), None, "a last line that is not text");
    let mut torn = line("94", "lock", "33").into_bytes();
    torn.extend_from_slice(b"\xff\n");
    assert_eq!(
        record(&torn),
        None,
        "a stop line followed by one that is not text"
    );
}

#[test]
fn a_number_is_read_only_as_the_runtime_spells_one() {
    for (spelled, number) in [
        ("0", 0),
        ("7", 7),
        ("33", 33),
        ("-5", -5),
        ("2147483647", i32::MAX),
        ("-2147483648", i32::MIN),
    ] {
        assert_eq!(
            record(line("94", "lock", spelled).as_bytes()).map(|record| record.os),
            Some(number),
            "{spelled:?}"
        );
        assert_eq!(
            record(line(spelled, "lock", "1").as_bytes()).map(|record| record.status),
            Some(number),
            "{spelled:?}"
        );
    }
    for unspelled in [
        "",
        "-",
        "+33",
        "033",
        "00",
        "-0",
        "-033",
        "3x",
        "x3",
        " 3",
        "2147483648",
        "-2147483649",
        "not a code",
    ] {
        assert_eq!(
            record(line("94", "lock", unspelled).as_bytes()),
            None,
            "an operating system code {unspelled:?}"
        );
        assert_eq!(
            record(line(unspelled, "lock", "0").as_bytes()),
            None,
            "a status {unspelled:?}"
        );
    }
}

#[test]
fn every_step_check_is_known_and_nothing_else_is() {
    for check in STEP_CHECKS {
        assert!(step_check(check), "{check:?}");
        assert_eq!(
            stated(line("94", check, "0").as_bytes(), PROTOCOL).map(|said| said.check),
            Some(check)
        );
    }
    for other in ["", "touch: open", "orphan: write", "allowance", "lock "] {
        assert!(!step_check(other), "{other:?}");
    }
}
