// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Measured, baseline_of};
use crate::execute::{self, MutantConclusion, MutantResult, Protocol, Stopped};
use crate::runner::ProcessExit;

fn exited_zero_with(output: &str, conclusion: MutantConclusion) -> MutantResult {
    let summary = match execute::parse_summary(output.as_bytes()) {
        Ok(summary) => summary,
        Err(not_text) => panic!("a test transcript is text: {not_text}"),
    };
    MutantResult {
        conclusion,
        exit_code: 0,
        output: output.as_bytes().to_vec(),
        protocol: Protocol::Libtest,
        summary,
        stopped: Stopped::Exited {
            exit: ProcessExit::Code(0),
        },
        ..MutantResult::apparatus_error("target", String::new())
    }
}

#[test]
fn a_baseline_whose_process_exited_before_its_summary_is_not_judgeable() {
    let result = exited_zero_with(
        "\nrunning 2 tests\ntest a ... ok\n",
        MutantConclusion::Inconclusive,
    );
    let baseline = baseline_of(&result, execute::Home::Confined).expect("a baseline is read");
    assert!(
        baseline
            .output
            .starts_with("the harness did not account for the run: the process ended before"),
        "a refused baseline says why and keeps what it printed: {}",
        baseline.output
    );
    assert!(
        Measured::of(baseline).judgeable().is_none(),
        "a run that ended before its harness closed its report measured only part of the target"
    );
}

#[test]
fn a_baseline_that_ran_nothing_is_still_judgeable() {
    let result = exited_zero_with(
        "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s\n",
        MutantConclusion::Inconclusive,
    );
    let baseline = baseline_of(&result, execute::Home::Confined).expect("a baseline is read");
    assert!(Measured::of(baseline).judgeable().is_some());
}

#[test]
fn an_ordinary_passing_baseline_is_judgeable() {
    let result = exited_zero_with(
        "\nrunning 1 test\ntest a ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        MutantConclusion::Survived,
    );
    let baseline = baseline_of(&result, execute::Home::Confined).expect("a baseline is read");
    assert!(Measured::of(baseline).judgeable().is_some());
}

#[test]
fn a_baseline_that_printed_nothing_says_how_its_process_ended() {
    let status_dll_not_found = -1_073_741_515;
    let result = MutantResult {
        exit_code: status_dll_not_found,
        stopped: Stopped::Exited {
            exit: ProcessExit::Code(status_dll_not_found),
        },
        ..exited_zero_with("", MutantConclusion::Inconclusive)
    };
    let baseline = baseline_of(&result, execute::Home::Confined).expect("a baseline is read");
    assert!(
        baseline
            .output
            .contains("its process exited with code -1073741515 (0xc0000135)"),
        "a process that printed nothing leaves only its ending to say why: {}",
        baseline.output
    );
}
