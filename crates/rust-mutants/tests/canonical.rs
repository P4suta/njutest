// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One spelling for a directory, whatever the platform calls it.

use std::path::{Path, PathBuf};

use rust_mutants::canonical::{canonical, plainly};

#[test]
fn a_resolved_directory_is_spelled_the_way_the_rest_of_a_run_spells_one() {
    let temp = tempfile::tempdir();
    assert!(temp.is_ok(), "a temporary directory: {temp:?}");
    let Ok(temp) = temp else { return };
    let resolved = canonical(temp.path());
    assert!(resolved.is_ok(), "the directory resolves: {resolved:?}");
    let Ok(resolved) = resolved else { return };
    let exact = resolved.to_str();
    assert!(exact.is_some(), "the temporary path is exact UTF-8");
    let Some(exact) = exact else { return };
    assert!(
        !exact.starts_with(r"\\?\"),
        "cargo prints a manifest path plainly and a person types one plainly, so a run \
         that kept the extended-length form would hold two spellings of one place: {resolved:?}",
    );
    let metadata = std::fs::metadata(&resolved);
    assert!(metadata.is_ok(), "{resolved:?}: {metadata:?}");
    let Ok(metadata) = metadata else { return };
    assert!(metadata.is_dir(), "{resolved:?}");
}

#[test]
fn an_extended_length_path_on_a_drive_is_the_plain_one() {
    assert_eq!(
        plainly(Path::new(r"\\?\C:\Users\somebody\project")),
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\somebody\project")
        } else {
            PathBuf::from(r"\\?\C:\Users\somebody\project")
        },
        "the prefix is Windows' own way of saying a drive path, and nowhere else is it \
         anything but part of a name"
    );
}

#[test]
fn a_share_keeps_the_prefix_because_without_it_it_names_somewhere_else() {
    let unc = Path::new(r"\\?\UNC\server\share\project");
    assert_eq!(
        plainly(unc),
        unc.to_path_buf(),
        "`UNC\\server\\share` without the prefix is a relative path called UNC, which is \
         a different place rather than a plainer spelling of the same one"
    );
}

#[test]
fn a_path_that_was_never_extended_is_left_as_it_is() {
    for plain in ["relative/path", "/absolute/path"] {
        assert_eq!(
            plainly(Path::new(plain)),
            PathBuf::from(plain),
            "{plain} says what it says, and a spelling nobody asked about is not one to change"
        );
    }
}
