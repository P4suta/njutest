// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::bench::Tree;
use super::{TARGET, installed};

#[test]
fn a_tree_is_spelled_as_its_build_baked_it_in_and_places_a_directory_by_its_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("tree");
    let tree = Tree::read(&root.join(""), std::iter::empty()).expect("an empty tree");
    assert_eq!(
        Some(tree.root.as_str()),
        root.to_str(),
        "the tree is preopened at the root cargo bakes into `CARGO_MANIFEST_DIR`, with no \
         separator after it"
    );
    assert_eq!(tree.within(&root), Some(String::new()));
    assert_eq!(
        tree.within(&root.join("member").join("inner")),
        Some("member/inner".to_owned())
    );
    assert_eq!(tree.within(dir.path()), None);
    assert_eq!(tree.within(&dir.path().join("elsewhere")), None);
}

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
