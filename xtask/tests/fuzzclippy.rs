// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The independent fuzz workspace receives the root lint policy without a second policy copy.

use njutest_devkit::result::{ResultState, result_state};

#[test]
fn flags_preserve_priority_and_namespace() {
    let manifest = r#"
[workspace]
[workspace.lints.rust]
unused = { level = "deny", priority = -1 }
unsafe_code = "forbid"
[workspace.lints.clippy]
pedantic = { level = "deny", priority = -1 }
module_name_repetitions = "allow"
"#;
    let flags = xtask::fuzzclippy::flags(manifest);
    assert_eq!(
        result_state(&flags),
        ResultState::Returned,
        "the literal lint table was refused: {flags:?}"
    );
    let Ok(flags) = flags else {
        return;
    };
    assert_eq!(
        flags,
        [
            "-D",
            "clippy::pedantic",
            "-D",
            "unused",
            "-A",
            "clippy::module-name-repetitions",
            "-F",
            "unsafe-code",
            "-D",
            "warnings",
        ]
    );
}

#[test]
fn a_missing_root_policy_fails_closed() {
    let failure = xtask::fuzzclippy::flags("[workspace]");
    assert_eq!(
        result_state(&failure),
        ResultState::Refused,
        "an absent policy produced flags: {failure:?}"
    );
    let Err(failure) = failure else {
        return;
    };
    assert!(failure.to_string().contains("workspace.lints.rust"));
}
