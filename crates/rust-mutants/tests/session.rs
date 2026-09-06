// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a session is asked, in the words a caller says it in. Nothing here starts a toolchain.

use std::time::Duration;

use rust_mutants::session::Request;

#[test]
fn a_request_built_step_by_step_equals_the_literal_it_replaces() {
    let built = Request::new("abc")
        .with_target("demo/lib/demo")
        .test(Some("tests::one".to_owned()))
        .with_args(vec!["--nocapture".to_owned()])
        .with_timeout(Some(Duration::from_secs(30)));
    assert_eq!(built.mutant, "abc");
    assert_eq!(built.target.as_deref(), Some("demo/lib/demo"));
    assert_eq!(built.test.as_deref(), Some("tests::one"));
    assert_eq!(built.args, vec!["--nocapture".to_owned()]);
    assert_eq!(built.timeout, Some(Duration::from_secs(30)));

    let plain = Request::new("abc");
    assert_eq!(plain.target, None);
    assert_eq!(plain.test, None);
    assert!(plain.args.is_empty());
    assert_eq!(plain.timeout, None);
}

#[test]
fn asking_for_the_whole_target_again_is_one_call_rather_than_a_struct_update() {
    let one = Request::new("abc")
        .with_target("demo/lib/demo")
        .test(Some("tests::one".to_owned()));
    let whole = one.clone().test(None);
    assert_eq!(whole.test, None);
    assert_eq!(
        whole.target, one.target,
        "asking for the whole target changes what runs and nothing else"
    );
    assert_eq!(whole.mutant, one.mutant);
}
