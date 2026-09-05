// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `test result:` line is what decides whether a target passed, so a hostile one must not panic the parser and must not read as a pass. A test that printed its own summary line is the attack, and libtest itself is the only writer a run should believe.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mjutest_cli::assure::baseline::status_of;
use mjutest_cli::report::TargetStatus;
use rust_mutants::execute::parse_summary;

fuzz_target!(|data: &[u8]| {
    let summary = parse_summary(data);
    let (status, message) = status_of(summary, false);

    if status == TargetStatus::Passed {
        let counted = summary.expect("a pass comes from a summary line");
        assert!(counted.passed > 0);
        assert_eq!(counted.failed, 0);
        assert!(message.is_none());
    }
    assert_eq!(status_of(summary, true).0, TargetStatus::Failed);
});
