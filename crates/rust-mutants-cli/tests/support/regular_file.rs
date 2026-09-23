// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs as test_regular_file_fs;
use std::path::Path as TestRegularFilePath;

#[track_caller]
#[expect(
    clippy::panic,
    reason = "filesystem metadata failures are test setup failures, not false predicates"
)]
fn test_regular_file(path: &TestRegularFilePath) -> bool {
    match test_regular_file_fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type().is_file(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("reading {}: {error}", path.display()),
    }
}
