// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one machine hands another.
//!
//! `njutest cache --import` reads a stream nobody in this repository wrote: it
//! arrives over a network, out of a CI cache, from a machine on a different
//! release. A reader of that has one job — refuse everything that is not an
//! answer this machine may keep — and it must do it without panicking, because
//! a panic here is a job that fails on the shape of a file rather than on
//! anything about the code under test.

#![no_main]

use std::sync::OnceLock;
use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use njutest_cli::cache::store::Store;

/// One store for the whole process: a directory made per iteration would fuzz the operating system.
fn store() -> &'static Store {
    static HELD: OnceLock<(tempfile::TempDir, Store)> = OnceLock::new();
    &HELD
        .get_or_init(|| {
            let root = tempfile::tempdir().expect("a directory to keep answers in");
            let store = Store::new(root.path(), 1 << 30, Duration::from_secs(3600));
            (root, store)
        })
        .1
}

fuzz_target!(|data: &[u8]| {
    let store = store();
    let before = store.status().expect("the store says what it holds").entries;
    let mut arriving = data;
    let Ok(read) = store.import(&mut arriving) else {
        return;
    };
    let after = store.status().expect("the store says what it holds").entries;
    assert!(
        after >= before,
        "an import that came back with an answer took nothing away"
    );

    let mut carried = Vec::new();
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
