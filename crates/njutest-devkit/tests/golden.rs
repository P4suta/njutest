// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Contract of the golden-file harness every other suite rests on.

include!("support/err.rs");
include!("support/fail.rs");
include!("support/metadata.rs");
include!("support/missing.rs");
include!("support/ok.rs");

use std::fs;

use njutest_devkit::golden::{GoldenError, compare_golden};

#[test]
fn identical_bytes_pass_without_touching_the_file() {
    let dir = test_ok(tempfile::tempdir(), "tempdir");
    let path = dir.path().join("out.golden");
    test_ok(fs::write(&path, b"hello\n"), "write");
    let before = test_ok(test_metadata(&path).modified(), "mtime");

    test_ok(
        compare_golden(&path, b"hello\n", false),
        "identical bytes pass",
    );

    let after = test_ok(test_metadata(&path).modified(), "mtime");
    assert_eq!(
        before, after,
        "a passing comparison must not rewrite the golden file"
    );
}

#[test]
fn a_mismatch_names_the_file_and_shows_both_sides() {
    let dir = test_ok(tempfile::tempdir(), "tempdir");
    let path = dir.path().join("out.golden");
    test_ok(fs::write(&path, b"line one\nline two\n"), "write");

    let error = test_err(
        compare_golden(&path, b"line one\nline 2\n", false),
        "differs",
    );

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
        other => test_fail(format_args!("expected a mismatch, got {other:?}")),
    }
    assert_eq!(
        test_ok(fs::read(&path), "read"),
        b"line one\nline two\n",
        "read-only"
    );
}

#[test]
fn a_missing_file_is_a_failure_and_not_a_first_recording() {
    let dir = test_ok(tempfile::tempdir(), "tempdir");
    let path = dir.path().join("never.golden");

    let error = test_err(compare_golden(&path, b"anything", false), "missing");

    assert!(matches!(error, GoldenError::Missing { .. }), "{error:?}");
    assert!(
        test_missing(&path),
        "the comparison must not create the file"
    );
}

#[test]
fn update_records_the_bytes_and_passes() {
    let dir = test_ok(tempfile::tempdir(), "tempdir");
    let path = dir.path().join("nested").join("new.golden");

    test_ok(compare_golden(&path, b"recorded\n", true), "update passes");
    assert_eq!(test_ok(fs::read(&path), "read"), b"recorded\n");

    test_ok(
        compare_golden(&path, b"changed\n", true),
        "update overwrites",
    );
    assert_eq!(test_ok(fs::read(&path), "read"), b"changed\n");

    test_ok(
        compare_golden(&path, b"changed\n", false),
        "and the recording then passes read-only",
    );
}

#[test]
fn binary_mismatch_reports_the_first_differing_offset() {
    let dir = test_ok(tempfile::tempdir(), "tempdir");
    let path = dir.path().join("bytes.golden");
    test_ok(fs::write(&path, [0u8, 1, 2, 0xff, 4]), "write");

    let error = test_err(
        compare_golden(&path, &[0u8, 1, 2, 0xfe, 4], false),
        "differs",
    );

    let GoldenError::Mismatch { diff, .. } = error else {
        test_fail(format_args!("expected a mismatch, got {error:?}"));
    };
    assert!(
        diff.contains("offset 3"),
        "names the first differing byte: {diff}"
    );
}
