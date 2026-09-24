// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one machine hands another.

#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use njutest::cache::store::Store;

/// One store for the whole process: a directory made per iteration would fuzz the operating system.
fn store() -> &'static Store {
    static HELD: OnceLock<(tempfile::TempDir, Store)> = OnceLock::new();
    &HELD
        .get_or_init(|| {
            #[expect(
                clippy::expect_used,
                reason = "without its one process store this fuzz target cannot execute"
            )]
            let root = tempfile::tempdir().expect("a directory to keep answers in");
            let store = Store::new(root.path(), 1 << 30);
            (root, store)
        })
        .1
}

fuzz_target!(|data: &[u8]| {
    let store = store();
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when its own store becomes unreadable"
    )]
    let before = store.status().expect("the store says what it holds").entries;
    let mut arriving = data;
    let Ok(read) = store.import(&mut arriving) else {
        return;
    };
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when an accepted import corrupts its store"
    )]
    let after = store.status().expect("the store says what it holds").entries;
    assert!(
        after >= before,
        "an import that came back with an answer took nothing away"
    );

    let mut carried = Vec::new();
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when a readable store cannot export its own entries"
    )]
    let written = store
        .export(&mut carried)
        .expect("what a machine holds, written out");
    assert_eq!(
        written, after,
        "everything a machine holds is everything it hands on, or the next machine is \
         given a smaller claim under the same name"
    );
    assert!(
        read <= written,
        "and nothing arrived that is not there afterwards"
    );
});
