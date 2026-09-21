// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs as test_directory_fs;
use std::path::Path as TestDirectoryPath;

#[track_caller]
#[expect(
    clippy::panic,
    reason = "filesystem metadata failures are test setup failures, not false predicates"
)]
fn test_directory(path: &TestDirectoryPath) -> bool {
    match test_directory_fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type().is_dir(),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.kind() == std::io::ErrorKind::NotADirectory =>
        {
            false
        }
        Err(error) => panic!("reading {}: {error}", path.display()),
    }
}
