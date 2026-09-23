// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fs as test_metadata_fs;
use std::path::Path as TestMetadataPath;

#[track_caller]
#[expect(
    clippy::panic,
    reason = "filesystem metadata failures are test setup failures, not false predicates"
)]
fn test_metadata(path: &TestMetadataPath) -> test_metadata_fs::Metadata {
    match test_metadata_fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => panic!("reading {}: {error}", path.display()),
    }
}
