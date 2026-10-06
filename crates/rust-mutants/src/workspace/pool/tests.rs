// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::Path;

use crate::cargo::{CAPTURED_COMPILER, products_name};
use crate::keyed::{Binding, binding};
use crate::session::{CAPTURE, COMPILED_MUTANT, DOCTESTS, IGNORED, KEPT, LISTED};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHARING: &str = "0123456789abcdeffedcba9876543210fedcba9876543210fedcba9876543210";

/// The most characters the engine's own names add below a fixture build root, besides a package's name.
const BUDGET: usize = 181;

/// Windows' `MAX_PATH`, which holds 259 characters and a terminator.
const MAX_PATH: usize = 260;

/// The characters a Windows path holds.
const HELD: usize = MAX_PATH - 1;

/// What the budget leaves of a Windows path for the root, the package's name and what cargo and rustc name below.
const ROOM: usize = HELD - BUDGET;

/// The separators the root and the package's name add to the engine's layout.
const SEPARATORS: usize = 2;

#[test]
fn a_slot_another_key_keeps_is_passed_over_and_left_as_it_was() {
    let root = tempfile::tempdir().expect("a fixture build root");
    let taken = super::slot(root.path(), SHARING, 0).expect("a key");
    std::fs::create_dir_all(&taken).expect("the other key's slot");
    assert_eq!(
        binding(&taken, SHARING).expect("the other key's record"),
        Binding::Bound
    );
    let mut owner = super::lease(root.path(), KEY, jiff::Timestamp::now()).expect("a lease");
    assert_eq!(
        owner.dir(),
        super::slot(root.path(), KEY, 1).expect("a key"),
        "a slot whose record keeps another key that shares the name is never handed out"
    );
    assert_eq!(
        std::fs::read(taken.join(crate::keyed::RECORD_NAME)).expect("the other key's record"),
        SHARING.as_bytes(),
        "the other key's slot keeps its record"
    );
    owner.release().expect("the lease is let go");
    let mut again = super::lease(root.path(), KEY, jiff::Timestamp::now()).expect("a lease");
    assert_eq!(
        again.dir(),
        super::slot(root.path(), KEY, 1).expect("a key"),
        "the key's own slot is found again"
    );
    again.release().expect("the lease is let go");
}

#[test]
fn a_retained_directory_is_named_by_a_short_prefix_of_its_key() {
    let root = Path::new("root");
    assert_eq!(
        super::graph(root, KEY).expect("a key"),
        root.join("source-graphs-v1").join("0123456789abcdef")
    );
    assert_eq!(
        super::slot(root, KEY, 3).expect("a key"),
        root.join("0123456789abcdef").join("3")
    );
    assert_eq!(
        super::inputs(root, KEY).expect("a key"),
        root.join("inputs-0123456789abcdef")
    );
}

/// The deepest directory the engine names below a fixture build root: an opaque graph's doctests, captured for an active mutant's sealed build and kept as verified products.
#[test]
fn the_deepest_layout_below_a_fixture_build_root_leaves_windows_room() {
    let observation = KEY.get(..32).expect("an observation identity is 32 digits");
    let slot = super::slot(Path::new(""), KEY, 99).expect("a key");
    let target = super::super::target_of(
        &super::inputs(&slot, KEY).expect("a key"),
        Path::new("/tree"),
    );
    let longest = [CAPTURE, LISTED, IGNORED, KEPT]
        .into_iter()
        .max_by_key(|name| name.len())
        .expect("a capture directory");
    let deepest = super::super::sealed_target_of(&target, KEY)
        .expect("a key")
        .join(super::super::SEALED_BUILD)
        .join(COMPILED_MUTANT)
        .join(DOCTESTS)
        .join(longest)
        .join(products_name(KEY, observation).expect("both are keys"))
        .join(CAPTURED_COMPILER);
    let added = deepest
        .as_os_str()
        .len()
        .checked_add(SEPARATORS)
        .expect("a path length");
    assert!(
        added <= BUDGET,
        "{} adds {added} characters below the root, over its budget of {BUDGET}: \
         Windows' MAX_PATH of {MAX_PATH} holds {HELD} characters, and {HELD} - {BUDGET} \
         leaves {ROOM} for the root, the package's name and what cargo and rustc name below",
        deepest.display()
    );
}
