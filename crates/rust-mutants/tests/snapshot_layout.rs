// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a copy puts the tree and what it reads beside itself, as path arithmetic and nothing else.
//!
//! Every fixture here is a POSIX absolute path, and a leading slash is not absolute on Windows without a drive: `Layout::plan` refuses one as "a relative path names no place a copy can reproduce", which is the right answer to the wrong question.
//! The arithmetic is the same on both, so what Windows needs is drive-rooted and UNC fixtures of its own rather than these ones bent to fit.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a table by position"
)]

use std::path::{Component, Path, PathBuf};

use proptest::prelude::*;
use rust_mutants::snapshot::{Layout, Placed, Placement, SnapshotErrorKind};

/// The relative path a reader would write to reach `to` from `from`, folded the way cargo folds one.
fn between(from: &Path, to: &Path) -> PathBuf {
    let mine: Vec<Component<'_>> = from.components().collect();
    let theirs: Vec<Component<'_>> = to.components().collect();
    let shared = mine
        .iter()
        .zip(&theirs)
        .take_while(|(one, other)| one == other)
        .count();
    let mut found = PathBuf::new();
    for _climbed in shared..mine.len() {
        found.push("..");
    }
    for part in &theirs[shared..] {
        found.push(part);
    }
    found
}

/// The layout of `root` reading `allowed`, placed under `/snap/tree`.
fn placed(root: &str, allowed: &[&str]) -> Placement {
    let allowed: Vec<PathBuf> = allowed.iter().map(PathBuf::from).collect();
    Layout::plan(Path::new(root), &allowed)
        .expect("a root and directories with a place of their own")
        .under(PathBuf::from("/snap/tree"))
}

#[test]
fn no_allowed_directory_leaves_the_tree_where_it_has_always_been() {
    let placement = placed("/w", &[]);
    assert_eq!(
        placement.root(),
        Path::new("/snap/tree"),
        "a run that reads nothing outside the tree is nearly every run, and its copy is \
         the copy it has always been: the ancestor of one path is that path, so the \
         relative part is empty and the tree is the stage"
    );
    assert!(
        placement.scaffolding().is_empty(),
        "and the copy makes no directory the tree did not come with"
    );
}

#[test]
fn every_relative_path_between_the_tree_and_what_it_reads_is_the_one_it_had() {
    for (root, allowed) in [
        ("/p/proj", vec!["/p/lib"]),
        ("/t/nested/proj", vec!["/t/lib"]),
        ("/p/proj", vec!["/p/shared/lib"]),
        ("/a/b/c/proj", vec!["/a/lib", "/a/b/other"]),
    ] {
        let placement = placed(root, &allowed);
        for one in placement.beside() {
            assert_eq!(
                between(Path::new(root), one.source()),
                between(placement.root(), one.destination()),
                "a manifest inside {root} reaches {} by a path it writes down, and the \
                 copy has to be a tree that same path still crosses",
                one.source().display()
            );
        }
    }
}

#[test]
fn every_destination_is_under_the_stage() {
    let placement = placed("/t/nested/proj", &["/t/lib"]);
    assert!(placement.root().starts_with(placement.stage()));
    for one in placement.beside() {
        assert!(
            one.destination().starts_with(placement.stage()),
            "{} is placed outside the directory the copy owns, which is a run writing \
             where nothing sweeps",
            one.destination().display()
        );
    }
}

#[test]
fn the_copy_names_the_root_and_what_was_allowed_and_nothing_else() {
    let placement = placed("/a/b/c/proj", &["/a/lib", "/a/b/other"]);
    let copied: Vec<&Path> = placement.beside().iter().map(Placed::source).collect();
    assert_eq!(
        copied,
        [Path::new("/a/b/other"), Path::new("/a/lib")],
        "a high ancestor decides where each copied directory lands and never what is \
         copied: an intermediate is an empty directory, not a tree to walk"
    );
}

#[test]
fn a_directory_inside_another_allowed_one_is_copied_once_with_the_one_that_holds_it() {
    let placement = placed("/p/proj", &["/p/shared", "/p/shared/lib"]);
    let copied: Vec<&Path> = placement.beside().iter().map(Placed::source).collect();
    assert_eq!(
        copied,
        [Path::new("/p/shared")],
        "the inner one is already inside the outer one's copy, and copying it twice \
         would put two trees where the manifest names one"
    );
}

#[test]
fn the_directories_the_copy_makes_are_named_once_each_and_shallowest_first() {
    let placement = placed("/a/b/c/proj", &["/a/lib", "/a/b/other"]);
    let made = placement.scaffolding();
    assert_eq!(
        made,
        [
            PathBuf::from("/snap/tree"),
            PathBuf::from("/snap/tree/b"),
            PathBuf::from("/snap/tree/b/c"),
        ],
        "each is created exactly once with an exclusive create, so a directory already \
         there stays a fact to react to rather than one to paper over"
    );
    for one in
        std::iter::once(placement.root()).chain(placement.beside().iter().map(Placed::destination))
    {
        let parent = one.parent().expect("a destination under the stage");
        assert!(
            parent == placement.stage() || made.contains(&parent.to_path_buf()),
            "{} is created into a directory nothing made",
            one.display()
        );
    }
}

#[test]
fn a_directory_that_is_the_tree_or_holds_it_or_is_inside_it_is_refused() {
    for allowed in ["/p/proj", "/p", "/p/proj/vendor"] {
        let refused = Layout::plan(Path::new("/p/proj"), &[PathBuf::from(allowed)])
            .expect_err("a directory with no place of its own beside the tree");
        assert_eq!(refused.kind(), SnapshotErrorKind::Layout);
        assert!(refused.to_string().contains("RM1019"), "{refused}");
    }
}

#[test]
fn a_path_that_is_relative_or_still_climbs_is_refused() {
    for allowed in ["relative/lib", "/p/../lib"] {
        let refused = Layout::plan(Path::new("/p/proj"), &[PathBuf::from(allowed)])
            .expect_err("a path naming no one place");
        assert_eq!(refused.kind(), SnapshotErrorKind::Layout);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The placement is one prefix substitution, so it is an isometry for relative paths.
    #[test]
    fn a_copy_preserves_every_path_between_the_things_it_moves(
        ancestor in prop::collection::vec("[a-z]{1,3}", 0..3),
        down_to_root in prop::collection::vec("[a-z]{1,3}", 1..4),
        down_to_lib in prop::collection::vec("[a-z]{1,3}", 1..4),
        inside in prop::collection::vec("[a-z]{1,3}", 0..3),
    ) {
        let base = PathBuf::from("/").join(ancestor.join("/"));
        let root = base.join(down_to_root.join("/"));
        let lib = base.join(down_to_lib.join("/"));
        prop_assume!(!root.starts_with(&lib) && !lib.starts_with(&root));

        let placement = Layout::plan(&root, std::slice::from_ref(&lib))
            .expect("two directories under one ancestor")
            .under(PathBuf::from("/snap/tree"));
        let reached = lib.join(inside.join("/"));
        let copied = placement
            .beside()
            .first()
            .expect("the one allowed directory")
            .destination()
            .join(inside.join("/"));
        prop_assert_eq!(
            between(&root, &reached),
            between(placement.root(), &copied),
            "the path a manifest writes is the path the copy has to hold"
        );
    }
}
