// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assumes a file it creates can be read by the group.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;

#[test]
fn a_new_file_is_readable_by_the_group() {
    let path = std::env::temp_dir().join(format!("environment-umask-{}", std::process::id()));
    std::fs::write(&path, "x").expect("a file");
    let mode = std::fs::metadata(&path).expect("its metadata").permissions().mode();
    std::fs::remove_file(&path).expect("the file removed");
    assert_ne!(mode & 0o040, 0);
}
