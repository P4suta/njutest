// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run is called, and which of the stored ones a collection keeps.

use jiff::Timestamp;
use rust_mutants_cli::app::stored::run_id;

#[test]
fn a_run_is_named_for_the_moment_it_began_and_nothing_else() {
    let at = Timestamp::from_second(1_800_000_000).expect("in range");
    let name = run_id(at).expect("the timestamp has the run-id alphabet");
    assert_eq!(
        name.as_str().len(),
        19,
        "a run identity is a fixed width, so a directory listing of them sorts \
         chronologically by name and a person reading two of them can tell which came \
         first at a glance: {name}"
    );
    assert!(
        name.as_str().chars().all(|one| one.is_ascii_alphanumeric()),
        "and it is a name every filesystem takes: a colon is a path separator on one of \
         them and a run nobody can store is a run nobody can read back: {name}"
    );
    assert!(
        name.as_str().starts_with("2027")
            && name.as_str().contains('t')
            && name.as_str().ends_with('z'),
        "while it is still the moment it began, spelled the way the rest of the reports \
         spell one: {name}"
    );
    assert!(
        run_id(at).expect("canonical") == name
            && run_id(
                at.checked_add(std::time::Duration::from_millis(1))
                    .expect("in range")
            )
            .expect("canonical")
                != name,
        "two runs a millisecond apart are two names: a run that took a name another run \
         is using writes its report over that run's"
    );
}
