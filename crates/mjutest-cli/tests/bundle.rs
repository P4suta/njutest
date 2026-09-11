// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What goes into the bundle somebody sends when a run went wrong, and what the bundle says is missing.

use mjutest_cli::app::diagnostics::{copy, copy_tree, record};

#[test]
fn every_part_of_a_bundle_is_listed_as_held_or_as_absent_and_never_as_neither() {
    let (mut held, mut absent) = (Vec::new(), Vec::new());
    record(true, "report.json", &mut held, &mut absent);
    record(false, "trace.jsonl", &mut held, &mut absent);
    assert_eq!(
        (held, absent),
        (
            vec!["report.json".to_owned()],
            vec!["trace.jsonl".to_owned()]
        ),
        "a bundle is what somebody sends when a run went wrong, so what is missing from \
         it is as much of the answer as what is in it: a part nobody listed either way \
         is one the reader assumes was never asked for"
    );
}

#[test]
fn a_file_that_is_not_there_is_not_copied_and_says_so() {
    let dir = tempfile::tempdir().expect("tempdir");
    let there = dir.path().join("there.txt");
    std::fs::write(&there, "one line\n").expect("a file to copy");

    assert!(
        copy(&there, &dir.path().join("copied.txt")),
        "a file that is there is copied and says it was"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("copied.txt")).expect("the copy"),
        "one line\n",
        "with its bytes, because a bundle of empty files is a bundle of nothing"
    );
    assert!(
        !copy(&dir.path().join("nowhere.txt"), &dir.path().join("out.txt")),
        "and one that is not there says so rather than leaving an empty file behind for \
         a reader to draw conclusions from"
    );
}

#[test]
fn a_directory_is_carried_whole_and_an_empty_one_is_not_carried_at_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let from = dir.path().join("outputs");
    std::fs::create_dir_all(from.join("deeper")).expect("a directory with one inside it");
    std::fs::write(from.join("top.txt"), "top\n").expect("a file at the top");
    std::fs::write(from.join("deeper/under.txt"), "under\n").expect("a file below");

    let into = dir.path().join("bundle/outputs");
    assert!(copy_tree(&from, &into), "a directory that holds something");
    assert_eq!(
        std::fs::read_to_string(into.join("deeper/under.txt")).expect("the file below"),
        "under\n",
        "a directory is carried whole, however deep: the output of the command that went \
         wrong is as likely to be two levels down as one"
    );

    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).expect("a directory with nothing in it");
    let carried = dir.path().join("bundle/empty");
    assert!(
        !copy_tree(&empty, &carried),
        "while a directory that is there and holds nothing held nothing: saying it was \
         there would put a name in the manifest with nothing behind it, and a reader \
         would go looking for what it holds"
    );
    assert!(
        !carried.exists(),
        "and nothing of it is left in the bundle: a directory that is there beside a \
         manifest that says it is absent is two answers to one question, and the one a \
         person opening the bundle reads first is the directory"
    );
    assert!(
        !copy_tree(
            &dir.path().join("nowhere"),
            &dir.path().join("bundle/nowhere")
        ),
        "and one that is not there is not there"
    );
}

#[test]
fn a_tree_that_holds_only_directories_is_carried_as_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let from = dir.path().join("shells");
    std::fs::create_dir_all(from.join("one/two")).expect("directories and no files");

    let carried = dir.path().join("bundle/shells");
    assert!(
        !copy_tree(&from, &carried),
        "a tree of empty directories held nothing, however deep it goes: a bundle that \
         said it carried this would send a reader through three levels to find out it \
         was empty"
    );
    assert!(
        !carried.exists(),
        "and none of those levels is left behind to send them"
    );

    std::fs::write(from.join("one/two/deep.txt"), "deep\n").expect("one file, three levels down");
    assert!(
        copy_tree(&from, &dir.path().join("bundle/again")),
        "while one file anywhere in it makes the whole tree worth carrying, and the \
         answer has to come back up from wherever it was"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("bundle/again/one/two/deep.txt"))
            .expect("the file three levels down"),
        "deep\n"
    );
}
