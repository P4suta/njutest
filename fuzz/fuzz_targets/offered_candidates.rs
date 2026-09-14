// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What somebody else's program offers to write into your tree.
//!
//! A generation provider is a command the configuration names, so its output
//! is the least trusted thing this program reads, and what the reader decides
//! is where bytes land on somebody's disk. Two things must hold for every
//! string there is: it does not panic, and no candidate that comes back out
//! of it names a place outside what the configuration allowed. A reader that
//! accepted `../../.ssh/authorized_keys` would be a program that writes there
//! the moment somebody runs `fix --apply`.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use njutest_cli::repair;

fuzz_target!(|said: &str| {
    let root = Path::new("/workspace");
    let allowed = repair::allowed(&[]);
    let Ok(offered) = repair::take(said, root, &allowed) else {
        return;
    };
    for proposal in &offered {
        assert!(
            !proposal.path.is_empty(),
            "a candidate with no path is one nothing could apply"
        );
        assert!(
            !proposal.path.starts_with('/') && !proposal.path.starts_with('\\'),
            "a candidate that came back absolute would be written wherever it said, and \
             not under the workspace: {}",
            proposal.path
        );
        assert!(
            !proposal.path.split(['/', '\\']).any(|part| part == ".."),
            "and one that walks upward leaves the tree whatever the prefix looked like: {}",
            proposal.path
        );
        assert!(
            allowed
                .iter()
                .filter_map(|pattern| rust_mutants::glob::Pattern::compile(pattern).ok())
                .any(|pattern| pattern.matches(&proposal.path)),
            "and every one that comes back is somewhere the configuration allowed, \
             because what comes back is what `fix --apply` writes: {}",
            proposal.path
        );
        assert_eq!(
            proposal.digest.len(),
            64,
            "and it is named by the digest of its own bytes"
        );
    }
});
