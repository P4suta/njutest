// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{TARGET, installed};

#[test]
fn a_sysroot_without_the_targets_library_directory_does_not_hold_it() {
    let sysroot = tempfile::tempdir().expect("tempdir");
    assert!(!installed(sysroot.path()).expect("an absent directory is an answer"));
}

#[test]
fn a_library_directory_without_a_standard_library_does_not_hold_it() {
    let sysroot = tempfile::tempdir().expect("tempdir");
    let libdir = sysroot.path().join("lib/rustlib").join(TARGET).join("lib");
    std::fs::create_dir_all(&libdir).expect("mkdir");
    std::fs::write(libdir.join("libcore-0.rlib"), b"").expect("a library");
    assert!(!installed(sysroot.path()).expect("listed"));
    std::fs::write(libdir.join("libstd-0.rlib"), b"").expect("the standard library");
    assert!(installed(sysroot.path()).expect("listed"));
}
