// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What somebody else's program offers to write into your tree.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use njutest::repair;

#[expect(
    clippy::manual_ok_err,
    reason = "invalid configured globs are explicitly absent here; Result::ok is forbidden because it hides that policy"
)]
fn compiled(pattern: &str) -> Option<rust_mutants::glob::Pattern> {
    match rust_mutants::glob::Pattern::compile(pattern) {
        Ok(pattern) => Some(pattern),
        Err(_error) => None,
    }
}

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
                .filter_map(|pattern| compiled(pattern))
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
