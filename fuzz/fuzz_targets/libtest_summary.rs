// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a target printed is read for a line to show a person, and never for the answer: only the outcome of the execution says whether the target passed. A target that prints its own `test result:` line is the attack, and a hostile line must neither panic the reader nor turn any other outcome into a pass.

#![no_main]

use libfuzzer_sys::fuzz_target;
use njutest_cli::assure::baseline::status_of;
use njutest_cli::report::TargetStatus;
use rust_mutants::execute::parse_summary;
use rust_mutants::outcome::Outcome;

fuzz_target!(|data: &[u8]| {
    let _parsed = parse_summary(data);
    let output = String::from_utf8_lossy(data);
    for outcome in [
        Outcome::Survived,
        Outcome::Killed,
        Outcome::Runaway,
        Outcome::Waited,
        Outcome::Inconclusive,
        Outcome::NotRun,
        Outcome::Errored,
    ] {
        for ignored in [0u32, 1, 4096] {
            let (status, message) = status_of(outcome, ignored, &output);
            assert_eq!(
                status == TargetStatus::Passed,
                outcome == Outcome::Survived,
                "nothing a target prints decides that it passed: {outcome:?} read as {status:?}"
            );
            assert_eq!(
                status == TargetStatus::Passed,
                message.is_none(),
                "a target that did not pass says why, and one that passed has nothing to say"
            );
            assert!(
                status != TargetStatus::Skipped || ignored > 0,
                "a target is skipped only when libtest was told to skip every test of it"
            );
        }
    }
});
