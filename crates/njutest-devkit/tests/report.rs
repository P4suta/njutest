// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Complete original archives retain source bytes while refusing absent, hidden and changed inputs.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "an original evidence control reports unreadable setup and JSON fields by panicking"
)]

use std::path::{Path, PathBuf};

use njutest_devkit::report::OriginalTree;

fn corpus() -> PathBuf {
    njutest_devkit::paths::workspace_root().join("xtask/tests/testdata")
}

fn originals() -> Vec<PathBuf> {
    let root = corpus();
    let mut originals = vec![root.join("specimen-original")];
    for entry in std::fs::read_dir(&root).expect("the complete original run catalog") {
        let entry = entry.expect("an original run catalog entry");
        let name = entry.file_name();
        let name = name.to_str().expect("a portable original run identity");
        if name.starts_with("engine-run-") || name.starts_with("sharded-run") {
            originals.push(entry.path().join("original"));
        }
    }
    for entry in std::fs::read_dir(root.join("reader-runs"))
        .expect("the complete retained reader recording catalog")
    {
        originals.push(entry.expect("a reader recording").path().join("original"));
    }
    originals.sort();
    originals
}

#[test]
fn every_retained_original_extracts_its_complete_source_bytes_and_permissions() {
    let originals = originals();
    assert!(!originals.is_empty(), "the original catalog is present");
    for directory in originals {
        let original = OriginalTree::read(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        let root = original
            .extract()
            .expect("extract immutable verified original bytes");
        original
            .check_source(root.path())
            .expect("all extracted bytes and permissions agree");
        assert!(original.digests().contains_key("Cargo.toml"));
        let recorded = tempfile::tempdir().expect("a second original archive");
        OriginalTree::record(root.path(), recorded.path()).expect("record the complete held tree");
        let held = OriginalTree::read(recorded.path()).expect("read every re-encoded source entry");
        assert_eq!(
            original.digests(),
            held.digests(),
            "{}",
            directory.display()
        );
        eprintln!(
            "original: {} files: {}",
            directory.display(),
            held.digests().len()
        );
    }
}

fn changed_original() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a negative original evidence copy");
    njutest_devkit::fixture::copy_tree(
        &corpus().join("engine-run-simple/original"),
        directory.path(),
    );
    directory
}

fn binding(directory: &Path) -> serde_json::Value {
    njutest_devkit::strictjson::decode_slice(
        &std::fs::read(directory.join("source.json")).expect("the original source binding"),
    )
    .expect("a strict original binding")
}

fn write_binding(directory: &Path, value: &serde_json::Value) {
    std::fs::write(
        directory.join("source.json"),
        serde_json::to_vec(value).expect("a planted original binding"),
    )
    .expect("retain the negative binding");
}

#[test]
fn original_source_binding_refuses_missing_hidden_and_mismatched_inventory() {
    for (field, planted) in [
        ("sha256", serde_json::json!("0".repeat(64))),
        ("mode", serde_json::json!(0)),
    ] {
        let directory = changed_original();
        let mut value = binding(directory.path());
        value["files"]["Cargo.toml"][field] = planted;
        write_binding(directory.path(), &value);
        assert!(
            OriginalTree::read(directory.path()).is_err(),
            "changed {field}"
        );
    }
    let directory = changed_original();
    let mut value = binding(directory.path());
    assert!(
        value["files"]
            .as_object_mut()
            .expect("files")
            .remove("Cargo.toml")
            .is_some()
    );
    write_binding(directory.path(), &value);
    OriginalTree::read(directory.path()).expect_err("missing original file inventory");

    let directory = changed_original();
    let mut value = binding(directory.path());
    value["files"]["hidden.rs"] = value["files"]["Cargo.toml"].clone();
    write_binding(directory.path(), &value);
    OriginalTree::read(directory.path()).expect_err("hidden original file inventory");

    let directory = changed_original();
    let mut value = binding(directory.path());
    value["schema"] = "another-source".into();
    write_binding(directory.path(), &value);
    OriginalTree::read(directory.path()).expect_err("mismatched original schema");
}

#[test]
fn original_source_pair_refuses_missing_corrupt_and_unbound_files() {
    for file in ["source.json", "source.tar"] {
        let directory = changed_original();
        std::fs::remove_file(directory.path().join(file)).expect("remove required evidence");
        assert!(
            OriginalTree::read(directory.path()).is_err(),
            "missing {file}"
        );
    }
    let directory = changed_original();
    std::fs::write(directory.path().join("source.tar"), "changed original")
        .expect("a corrupted original archive");
    OriginalTree::read(directory.path()).expect_err("the archive digest changed");
    let directory = changed_original();
    std::fs::write(directory.path().join("hidden"), "unbound evidence")
        .expect("an unbound source pair member");
    OriginalTree::read(directory.path()).expect_err("the source pair contains an unbound member");
}

#[test]
fn original_source_comparison_refuses_absent_hidden_and_changed_configuration() {
    let original = OriginalTree::read(&corpus().join("engine-run-simple/original"))
        .expect("the genuine pre-producer source");
    let root = original.extract().expect("the genuine positive control");
    original
        .check_source(root.path())
        .expect("the complete original agrees");
    std::fs::write(root.path().join("hidden.toml"), "unrequested = true")
        .expect("plant hidden configuration");
    assert!(original.check_source(root.path()).is_err());
    std::fs::remove_file(root.path().join("hidden.toml")).expect("remove only the planted input");
    std::fs::write(root.path().join("Cargo.toml"), "[workspace]\n")
        .expect("plant mismatched configuration");
    assert!(original.check_source(root.path()).is_err());
    std::fs::remove_file(root.path().join("Cargo.toml")).expect("plant missing configuration");
    assert!(original.check_source(root.path()).is_err());
}

fn unsafe_archive(name: &str, kind: tar::EntryType, mode: u32, repeated: bool) -> Vec<u8> {
    let mut header = tar::Header::new_ustar();
    header.set_mode(mode);
    header.set_size(0);
    header.set_entry_type(kind);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
        .as_mut_bytes()
        .get_mut(..100)
        .expect("the name field")
        .fill(0);
    header
        .as_mut_bytes()
        .get_mut(..name.len())
        .expect("the planted name")
        .copy_from_slice(name.as_bytes());
    header.set_cksum();
    let mut archive = tar::Builder::new(Vec::new());
    archive
        .append(&header, &[][..])
        .expect("a negative archive entry");
    if repeated {
        archive
            .append(&header, &[][..])
            .expect("a repeated negative path");
    }
    archive.into_inner().expect("the planted malformed archive")
}

#[test]
fn original_archive_refuses_unsafe_paths_kinds_duplicates_and_permissions() {
    use sha2::Digest as _;

    for (name, kind, mode, repeated) in [
        ("../outside", tar::EntryType::Regular, 0o644, false),
        ("/outside", tar::EntryType::Regular, 0o644, false),
        ("src\\outside", tar::EntryType::Regular, 0o644, false),
        ("Cargo.toml", tar::EntryType::Symlink, 0o644, false),
        ("Cargo.toml", tar::EntryType::Regular, 0o644, true),
        ("Cargo.toml", tar::EntryType::Regular, 0o4644, false),
    ] {
        let directory = changed_original();
        let planted = unsafe_archive(name, kind, mode, repeated);
        std::fs::write(directory.path().join("source.tar"), &planted)
            .expect("a malformed negative archive");
        let mut manifest = binding(directory.path());
        manifest["archive"] = hex::encode(sha2::Sha256::digest(&planted)).into();
        write_binding(directory.path(), &manifest);
        let error = OriginalTree::read(directory.path()).expect_err("unsafe original refused");
        assert!(
            error.to_string().contains("safe relative path")
                || error.to_string().contains("not a regular file")
                || error.to_string().contains("repeats a portable path")
                || error.to_string().contains("special permission"),
            "{name}: {kind:?}, {mode:o}, repeated={repeated}: {error}"
        );
    }
}
