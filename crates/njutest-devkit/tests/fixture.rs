// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A throwaway copy of a fixture project, and the directories a run of one needs beside it.

use njutest_devkit::fixture::{Fixture, copy_tree};

#[test]
fn a_fixture_copy_is_a_throwaway_tree_with_temp_and_cache_beside_it() {
    let (root, temp, cache) = {
        let fixture = Fixture::copy("fixture-simple");
        assert!(fixture.root().join("Cargo.toml").is_file());
        assert!(fixture.root().join("src/lib.rs").is_file());
        assert!(fixture.temp().is_dir(), "a temporary directory of its own");
        assert!(fixture.cache().is_dir(), "a cache directory of its own");
        assert!(
            !fixture.temp().starts_with(fixture.root()),
            "the temporary directory is beside the tree, never inside it"
        );
        assert!(
            !fixture.cache().starts_with(fixture.root()),
            "the cache is beside the tree, never inside it"
        );
        (
            fixture.root().to_path_buf(),
            fixture.temp().to_path_buf(),
            fixture.cache().to_path_buf(),
        )
    };
    assert!(!root.exists(), "the copy goes away with the fixture");
    assert!(!temp.exists(), "and so does its temporary directory");
    assert!(!cache.exists(), "and so does its cache");
}

#[test]
fn a_fixture_copy_has_a_resolved_root_spelled_the_way_a_run_answers() {
    let fixture = Fixture::copy("fixture-simple");
    let canonical = fixture.root().canonicalize().expect("canonicalize");
    #[cfg(not(windows))]
    assert_eq!(
        fixture.root(),
        canonical,
        "a path a run reports has to compare equal to the one the test holds"
    );
    #[cfg(windows)]
    assert_eq!(
        std::path::Path::new(&format!(r"\\?\{}", fixture.root().display())),
        canonical,
        "the root is what resolving it answers, in the spelling the products put a \
         resolved path back into: a run that answered in one and a test that held the \
         other would be two names for one directory"
    );
}

#[test]
fn a_fixture_reads_and_writes_its_own_files() {
    let fixture = Fixture::copy("fixture-simple");
    let before = fixture.read("src/lib.rs");
    fixture.write("src/added.rs", b"// added\n");
    assert_eq!(fixture.read("src/added.rs"), b"// added\n");
    assert_eq!(
        fixture.read("src/lib.rs"),
        before,
        "writing one file leaves the others alone"
    );
}

#[test]
fn a_fingerprint_names_every_file_and_changes_with_one_of_them() {
    let fixture = Fixture::copy("fixture-simple");
    let before = fixture.fingerprint();
    assert!(
        before.iter().any(|(path, _)| path == "src/lib.rs"),
        "{before:?}"
    );
    fixture.write("src/lib.rs", b"pub fn nothing() {}\n");
    let after = fixture.fingerprint();
    assert_ne!(before, after);
    assert_eq!(before.len(), after.len(), "the same files, one of them new");
}

#[test]
fn copy_tree_skips_a_target_directory_and_keeps_bytes() {
    let source = tempfile::Builder::new()
        .prefix("devkit-copy-source-")
        .tempdir()
        .expect("tempdir");
    let destination = tempfile::Builder::new()
        .prefix("devkit-copy-destination-")
        .tempdir()
        .expect("tempdir");
    let root = source.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::create_dir_all(root.join("target/debug")).expect("mkdir");
    std::fs::create_dir_all(root.join("nested/target")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), "fn a() {}\r\nfn b() {}\r\n").expect("write");
    std::fs::write(root.join("target/debug/artifact"), "built").expect("write");
    std::fs::write(root.join("nested/target/artifact"), "built").expect("write");

    let into = destination.path().join("copy");
    copy_tree(root, &into);

    assert_eq!(
        std::fs::read(into.join("src/lib.rs")).expect("read"),
        b"fn a() {}\r\nfn b() {}\r\n",
        "the bytes are the bytes, line endings included"
    );
    assert!(
        !into.join("target").exists(),
        "a target directory is skipped"
    );
    assert!(
        !into.join("nested/target").exists(),
        "at every depth, not only at the root"
    );
    assert!(into.join("nested").is_dir(), "and its parent still comes");
}

#[test]
fn copy_with_siblings_places_the_library_beside_the_tree() {
    let fixture = Fixture::copy_with_siblings("fixture-simple", &["fixture-doctest"]);
    let sibling = fixture
        .root()
        .parent()
        .expect("a parent")
        .join("fixture-doctest");
    assert!(
        sibling.join("Cargo.toml").is_file(),
        "a sibling a path dependency could reach: {sibling:?}"
    );
    assert!(fixture.root().join("Cargo.toml").is_file());
}

#[cfg(unix)]
#[test]
fn a_symlinked_entry_is_copied_as_what_it_points_at_not_as_a_link() {
    let source = tempfile::Builder::new()
        .prefix("devkit-copy-link-")
        .tempdir()
        .expect("tempdir");
    let destination = tempfile::Builder::new()
        .prefix("devkit-copy-link-into-")
        .tempdir()
        .expect("tempdir");
    let root = source.path();
    std::fs::write(root.join("real.rs"), "fn real() {}\n").expect("write");
    std::os::unix::fs::symlink(root.join("real.rs"), root.join("linked.rs")).expect("symlink");

    let into = destination.path().join("copy");
    copy_tree(root, &into);

    let linked = into.join("linked.rs");
    assert_eq!(
        std::fs::read(&linked).expect("read"),
        b"fn real() {}\n",
        "what the link stood for is what the copy holds"
    );
    assert!(
        !std::fs::symlink_metadata(&linked)
            .expect("metadata")
            .file_type()
            .is_symlink(),
        "and the copy is not itself a link out of the tree"
    );
}
