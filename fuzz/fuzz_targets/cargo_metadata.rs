// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The document `cargo metadata` writes, which is where a run learns what the workspace holds. A reader that accepts a malformed one selects the wrong packages or the wrong targets, and a run measures something other than what was asked for. It never panics, and every package and target it accepts is named and rooted.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::cargo::Metadata;

fuzz_target!(|data: &[u8]| {
    let Ok(metadata) = Metadata::parse(data) else {
        return;
    };
    for package in &metadata.packages {
        assert!(!package.name.is_empty(), "a package with no name");
        assert!(
            !package.manifest_path.as_os_str().is_empty(),
            "a package with no manifest: {:?}",
            package.name
        );
        assert!(
            package.manifest_dir().as_os_str().len() < package.manifest_path.as_os_str().len(),
            "the directory of a manifest is shorter than the manifest: {}",
            package.manifest_path.display()
        );
        for target in &package.targets {
            assert!(
                !target.name.is_empty(),
                "a target with no name in {:?}",
                package.name
            );
        }
    }
});
