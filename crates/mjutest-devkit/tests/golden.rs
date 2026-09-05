// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Contract of the golden-file harness every other suite rests on.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, and asserts with panics"
)]

use std::fs;

use mjutest_devkit::golden::{GoldenError, compare_golden};

#[test]
fn identical_bytes_pass_without_touching_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.golden");
    fs::write(&path, b"hello\n").expect("write");
    let before = fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");

    compare_golden(&path, b"hello\n", false).expect("identical bytes pass");

    let after = fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    assert_eq!(
        before, after,
        "a passing comparison must not rewrite the golden file"
    );
}

#[test]
fn a_mismatch_names_the_file_and_shows_both_sides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.golden");
    fs::write(&path, b"line one\nline two\n").expect("write");

    let error = compare_golden(&path, b"line one\nline 2\n", false).expect_err("differs");

    match error {
        GoldenError::Mismatch {
            path: reported,
            diff,
        } => {
            assert_eq!(reported, path);
            assert!(
                diff.contains("-line two"),
                "diff shows the golden side: {diff}"
            );
            assert!(
                diff.contains("+line 2"),
                "diff shows the recorded side: {diff}"
            );
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
    assert_eq!(
        fs::read(&path).expect("read"),
        b"line one\nline two\n",
        "read-only"
    );
}

#[test]
fn a_missing_file_is_a_failure_and_not_a_first_recording() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("never.golden");

    let error = compare_golden(&path, b"anything", false).expect_err("missing");

    assert!(matches!(error, GoldenError::Missing { .. }), "{error:?}");
    assert!(!path.exists(), "the comparison must not create the file");
}

#[test]
fn update_records_the_bytes_and_passes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join("new.golden");

    compare_golden(&path, b"recorded\n", true).expect("update passes");
    assert_eq!(fs::read(&path).expect("read"), b"recorded\n");

    compare_golden(&path, b"changed\n", true).expect("update overwrites");
    assert_eq!(fs::read(&path).expect("read"), b"changed\n");

    compare_golden(&path, b"changed\n", false).expect("and the recording then passes read-only");
}

#[test]
fn binary_mismatch_reports_the_first_differing_offset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bytes.golden");
    fs::write(&path, [0u8, 1, 2, 0xff, 4]).expect("write");

    let error = compare_golden(&path, &[0u8, 1, 2, 0xfe, 4], false).expect_err("differs");

    match error {
        GoldenError::Mismatch { diff, .. } => {
            assert!(
                diff.contains("offset 3"),
                "names the first differing byte: {diff}"
            );
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
}
