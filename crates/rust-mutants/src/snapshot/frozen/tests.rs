// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::tempowner::{self, Role};

/// The copy's own lock is moved aside, so the lock at the directory's path is another holder's, as a watcher's is once it has waited for a claim to let go.
#[test]
fn a_graph_is_retained_under_the_claim_that_copied_it_whoever_holds_its_lock_path() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let tree = base.path().join("tree");
    std::fs::create_dir_all(tree.join("src")).expect("the tree");
    std::fs::write(tree.join("src").join("lib.rs"), "pub fn f() {}\n").expect("a source");
    let parent = base.path().join("graphs");
    std::fs::create_dir(&parent).expect("the graph parent");
    let rules = super::Options::new(super::Layout::plan(&tree, &[]).expect("a layout"), parent);
    let copied = super::super::create(&rules, jiff::Timestamp::now()).expect("a copy");
    let directory = copied.dir().to_path_buf();
    std::fs::rename(
        tempowner::lock_path(&directory),
        directory.join("owner.lock.held-by-the-copy"),
    )
    .expect("the copy's lock moves aside under its open handle");
    let mut holder = tempowner::acquire(&tempowner::lock_path(&directory))
        .expect("the lock path opens")
        .expect("nobody else holds the lock at the path");
    let taken = super::FrozenGraph::take(copied, &rules);
    holder.release().expect("the other holder lets go");
    let graph = match taken {
        Ok(graph) => graph,
        Err(refused) => panic!(
            "publication declares the graph under the claim that copied it, so a holder of the \
             lock at its path, which takes it whenever a claim lets go, cannot refuse it: \
             {refused}"
        ),
    };
    let marker = tempowner::read_marker(&directory).expect("the retained graph's marker");
    assert_eq!(
        (
            marker.schema.as_str(),
            marker.role,
            marker.released,
            graph.directory == directory
        ),
        ("rust-mutants-frozen-source-v1", Role::Cache, true, true),
        "the retained graph is a released cache a sweep spares: {marker:?}"
    );
}
