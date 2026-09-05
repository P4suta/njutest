// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a generation provider may offer: what is read, what is refused, and what is never written.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a setup failure by panicking, asserts with panics, and spells the encoding it checks the decoder against"
)]

use std::path::Path;

use mjutest_cli::repair::{
    CANDIDATE_LIMIT, DEFAULT_ALLOWED, Kind, RepairErrorKind, allowed, preimage_of, take,
};

/// The base64 of `text`, computed the long way so the test does not use the decoder it tests.
fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut value = 0u32;
        for position in 0..3 {
            value = (value << 8) | u32::from(chunk.get(position).copied().unwrap_or(0));
        }
        for position in 0..4 {
            if position <= chunk.len() {
                let shift = 18 - 6 * position;
                let index = usize::try_from((value >> shift) & 0x3f).expect("an index");
                out.push(char::from(ALPHABET[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn answer(candidates: &str) -> String {
    format!(r#"{{"version":1,"candidates":[{candidates}]}}"#)
}

fn patch(path: &str, preimage: Option<&str>, content: &str) -> String {
    let preimage = preimage.map_or_else(
        || "null".to_owned(),
        |digest| format!("{}", serde_json::Value::String(digest.to_owned())),
    );
    format!(
        r#"{{"kind":"patch","path":{},"preimage_sha256":{preimage},"content_base64":{}}}"#,
        serde_json::Value::String(path.to_owned()),
        serde_json::Value::String(base64(content))
    )
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("tests")).expect("mkdir");
    dir
}

fn patterns() -> Vec<String> {
    allowed(&[])
}

#[test]
fn a_candidate_that_creates_a_test_file_is_read_whole() {
    let dir = tree();
    let said = answer(&patch("tests/closes.rs", None, "#[test]\nfn t() {}\n"));
    let taken = take(&said, dir.path(), &patterns()).expect("taken");
    assert_eq!(taken.len(), 1);
    let one = taken.first().expect("one");
    assert_eq!(one.kind, Kind::Patch);
    assert_eq!(one.path, "tests/closes.rs");
    assert_eq!(one.preimage, None);
    assert_eq!(one.content, b"#[test]\nfn t() {}\n");
    assert_eq!(one.digest.len(), 64);
}

#[test]
fn nothing_a_provider_says_reaches_the_tree() {
    let dir = tree();
    let said = answer(&patch("tests/closes.rs", None, "#[test]\nfn t() {}\n"));
    let _taken = take(&said, dir.path(), &patterns()).expect("taken");
    assert!(
        !dir.path().join("tests/closes.rs").exists(),
        "a candidate is a proposal, never a change"
    );
}

#[test]
fn a_candidate_may_not_be_written_outside_the_allowed_paths() {
    let dir = tree();
    for path in [
        "src/lib.rs",
        "/etc/passwd",
        "../elsewhere/tests/x.rs",
        "Cargo.toml",
        "tests/../src/lib.rs",
    ] {
        let said = answer(&patch(path, None, "anything"));
        let refused = take(&said, dir.path(), &patterns()).expect_err("refused");
        assert_eq!(refused.kind(), RepairErrorKind::PathRefused, "{path}");
        assert_eq!(refused.code().code, "MJ5007", "{path}");
    }
}

#[test]
fn a_candidate_that_patches_a_file_must_have_seen_the_file_that_is_there() {
    let dir = tree();
    let path = dir.path().join("tests/existing.rs");
    std::fs::write(&path, "#[test]\nfn old() {}\n").expect("write");
    let real = preimage_of(dir.path(), "tests/existing.rs").expect("a preimage");

    let ok = answer(&patch(
        "tests/existing.rs",
        Some(&real),
        "#[test]\nfn new() {}\n",
    ));
    assert_eq!(take(&ok, dir.path(), &patterns()).expect("taken").len(), 1);

    let stale = answer(&patch(
        "tests/existing.rs",
        Some(&"a".repeat(64)),
        "#[test]\nfn new() {}\n",
    ));
    let refused = take(&stale, dir.path(), &patterns()).expect_err("refused");
    assert_eq!(refused.kind(), RepairErrorKind::PreimageMoved);
    assert_eq!(refused.code().code, "MJ5008");

    let creating = answer(&patch("tests/existing.rs", None, "#[test]\nfn new() {}\n"));
    let refused = take(&creating, dir.path(), &patterns()).expect_err("refused");
    assert_eq!(refused.kind(), RepairErrorKind::PreimageMoved);
}

#[test]
fn a_candidate_that_patches_a_file_that_is_not_there_is_refused() {
    let dir = tree();
    let said = answer(&patch("tests/gone.rs", Some(&"b".repeat(64)), "anything"));
    let refused = take(&said, dir.path(), &patterns()).expect_err("refused");
    assert_eq!(refused.kind(), RepairErrorKind::PreimageMoved);
}

#[test]
fn an_answer_this_version_does_not_understand_is_not_guessed_at() {
    let dir = tree();
    for said in [
        String::from("not json"),
        String::from(r#"{"version":2,"candidates":[]}"#),
        String::from(r#"{"version":1,"candidates":[],"extra":true}"#),
        answer(r#"{"kind":"rewrite","path":"tests/x.rs","content_base64":"AAAA"}"#),
        answer(r#"{"kind":"patch","path":"tests/x.rs","content_base64":"not base64!"}"#),
        answer(r#"{"kind":"patch","path":"tests/x.rs","content_base64":"AAA"}"#),
    ] {
        let refused = take(&said, dir.path(), &patterns()).expect_err("refused");
        assert_eq!(refused.kind(), RepairErrorKind::Protocol, "{said}");
        assert_eq!(refused.code().code, "MJ5006", "{said}");
    }
}

#[test]
fn more_candidates_than_are_read_is_refused_rather_than_truncated() {
    let dir = tree();
    let many: Vec<String> = (0..=CANDIDATE_LIMIT)
        .map(|index| patch(&format!("tests/c{index}.rs"), None, "x"))
        .collect();
    let said = answer(&many.join(","));
    let refused = take(&said, dir.path(), &patterns()).expect_err("refused");
    assert_eq!(refused.kind(), RepairErrorKind::Protocol);
}

#[test]
fn one_path_may_be_offered_once() {
    let dir = tree();
    let twice = format!(
        "{},{}",
        patch("tests/same.rs", None, "one"),
        patch("tests/same.rs", None, "two")
    );
    let refused = take(&answer(&twice), dir.path(), &patterns()).expect_err("refused");
    assert_eq!(refused.kind(), RepairErrorKind::Protocol);
}

#[test]
fn a_corpus_entry_goes_where_a_corpus_entry_goes() {
    let dir = tree();
    std::fs::create_dir_all(dir.path().join("fuzz/corpus/parse")).expect("mkdir");
    let said = answer(
        r#"{"kind":"corpus","path":"fuzz/corpus/parse/seed-1","preimage_sha256":null,"content_base64":"AAEC"}"#,
    );
    let taken = take(&said, dir.path(), &patterns()).expect("taken");
    assert_eq!(taken.first().map(|one| one.kind), Some(Kind::Corpus));
    assert_eq!(
        taken.first().map(|one| one.content.clone()),
        Some(vec![0, 1, 2])
    );
}

#[test]
fn where_a_provider_may_write_when_it_is_told_nothing_is_fixed() {
    assert_eq!(allowed(&[]), DEFAULT_ALLOWED);
    assert_eq!(
        allowed(&["tests/**".to_owned()]),
        vec!["tests/**".to_owned()]
    );
}

#[test]
fn the_preimage_of_a_file_that_is_not_there_is_nothing() {
    let dir = tree();
    assert_eq!(preimage_of(dir.path(), "tests/nothing.rs"), None);
    assert_eq!(preimage_of(Path::new("/nonexistent"), "x"), None);
}
