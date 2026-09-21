// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs as test_missing_fs;
use std::path::Path as TestMissingPath;

#[track_caller]
#[expect(
    clippy::panic,
    reason = "filesystem metadata failures are test setup failures, not false predicates"
)]
fn test_missing(path: &TestMissingPath) -> bool {
    match test_missing_fs::symlink_metadata(path) {
        Ok(_present) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => panic!("reading {}: {error}", path.display()),
    }
}
