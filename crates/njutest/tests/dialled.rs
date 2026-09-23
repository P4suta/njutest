// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a test is told to dial, once an interposer sits in front of what it would have.

use njutest::wire::dialled::{Dialled, redirected, upstream_of};

#[test]
fn the_authority_of_a_url_is_what_an_interposer_takes_the_place_of() {
    let cases: [(&str, &str, &str); 4] = [
        (
            "http://localhost:8080/orders",
            "localhost:8080",
            "http://127.0.0.1:9/orders",
        ),
        (
            "http://localhost:8080",
            "localhost:8080",
            "http://127.0.0.1:9",
        ),
        (
            "postgres://user:secret@db.internal:5432/app?sslmode=disable",
            "db.internal:5432",
            "postgres://user:secret@127.0.0.1:9/app?sslmode=disable",
        ),
        (
            "http://localhost:8080/a?to=http://elsewhere:1/b",
            "localhost:8080",
            "http://127.0.0.1:9/a?to=http://elsewhere:1/b",
        ),
    ];
    for (given, authority, expected) in cases {
        let found = upstream_of(given).unwrap_or_else(|| panic!("{given} names an authority"));
        assert_eq!(
            found, authority,
            "an interposer takes the place of what the caller would have dialled, so \
             reading the wrong part of {given} would put it in front of nothing"
        );
        assert_eq!(
            redirected(given, "127.0.0.1:9").as_deref(),
            Some(expected),
            "and everything else survives the rewrite: a credential, a path or a \
             query the run dropped would be a different program from the one the \
             baseline measured"
        );
    }
}

#[test]
fn a_value_that_names_no_authority_is_left_alone_rather_than_guessed_at() {
    for given in ["", "not a url", "/just/a/path", "localhost:8080"] {
        assert_eq!(
            upstream_of(given),
            None,
            "a run that invented an authority here would put an interposer in front \
             of something nobody dials: {given:?}"
        );
        assert_eq!(redirected(given, "127.0.0.1:9"), None, "{given:?}");
    }
}

#[test]
fn what_a_test_is_told_to_dial_names_the_variable_it_came_from() {
    let Dialled {
        variable, upstream, ..
    } = Dialled::of("BASE_URL", "http://localhost:8080/api")
        .unwrap_or_else(|| panic!("a variable that names an authority is one to sit in front of"));
    assert_eq!(variable, "BASE_URL");
    assert_eq!(
        upstream, "localhost:8080",
        "the run has to say what it sat in front of, or a reader cannot tell an \
         interposed seam from one nothing watched"
    );
}

#[test]
fn a_seam_the_run_could_not_watch_is_one_the_report_states() {
    let held = njutest::resource::Lease {
        capability: "api".to_owned(),
        instance: njutest::provider::InstanceId::checked("one").expect("instance id"),
        environment: vec![("OTHER".to_owned(), "http://127.0.0.1:9/x".to_owned())],
    };
    let resource = njutest::config::Resource {
        command: vec!["true".to_owned()],
        timeout: std::time::Duration::from_secs(1),
        shared: false,
        exclusive: false,
        environment: Vec::new(),
        interpose: "BASE_URL".to_owned(),
        wire: njutest::wire::Wire::Http,
        hold: std::time::Duration::from_millis(1),
    };
    let configured = std::collections::BTreeMap::from([("api".to_owned(), resource)]);

    let seams = njutest::assure::wire::watched(&[&held], &configured);
    assert!(
        seams.watching.is_empty(),
        "there is no variable of that name to put an interposer in front of"
    );
    assert_eq!(
        seams.unwatched.len(),
        1,
        "and the run says so rather than carrying on with the lease untouched: a seam \
         the configuration named and nothing watched used to leave no seam records, no \
         findings and no limitation, so the wire dimension read as covered"
    );
    assert!(
        njutest::limitation::ALL.contains(&njutest::limitation::SEAM_NOT_WATCHED),
        "and what it says is a name the register holds, so the page names it too"
    );
}
