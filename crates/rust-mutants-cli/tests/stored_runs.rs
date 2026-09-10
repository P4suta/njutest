// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run is called, and which of the stored ones a collection keeps.

use jiff::Timestamp;
use rust_mutants_cli::app::run_id;

#[test]
fn a_run_is_named_for_the_moment_it_began_and_nothing_else() {
    let at = Timestamp::from_second(1_800_000_000).expect("in range");
    let name = run_id(at);
    assert_eq!(
        name.len(),
        19,
        "a run identity is a fixed width, so a directory listing of them sorts \
         chronologically by name and a person reading two of them can tell which came \
         first at a glance: {name}"
    );
    assert!(
        name.chars().all(|one| one.is_ascii_alphanumeric()),
        "and it is a name every filesystem takes: a colon is a path separator on one of \
         them and a run nobody can store is a run nobody can read back: {name}"
    );
    assert!(
        name.starts_with("2027") && name.contains('T') && name.ends_with('Z'),
        "while it is still the moment it began, spelled the way the rest of the reports \
         spell one: {name}"
    );
    assert!(
        run_id(at) == name
            && run_id(
                at.checked_add(std::time::Duration::from_millis(1))
                    .expect("in range")
            ) != name,
        "two runs a millisecond apart are two names: a run that took a name another run \
         is using writes its report over that run's"
    );
}
