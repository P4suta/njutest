// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A binary is proven single-threaded only where every premise holds, and each premise that fails is named.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest::concurrency::proof::{
    Because, Evidence, Harness, PackageScan, Reach, Standing, Threads, Unproven, standing,
};
use njutest::concurrency::scan::{Starts, scanned};

fn package(name: &str, files: &[(&str, &str)], links: bool) -> PackageScan {
    let mut found = Vec::new();
    for (path, source) in files {
        for one in scanned(path, source).expect("Rust") {
            found.push(((*path).to_owned(), one));
        }
    }
    PackageScan {
        package: name.to_owned(),
        links,
        found,
        unread: Vec::new(),
    }
}

fn quiet() -> PackageScan {
    package(
        "quiet@1.0.0",
        &[("src/lib.rs", "pub fn add(a: u8, b: u8) -> u8 { a + b }")],
        false,
    )
}

#[test]
fn a_binary_whose_baseline_stayed_on_its_tests_and_whose_closure_starts_nothing_is_single_threaded()
{
    let quiet = quiet();
    assert_eq!(
        standing(Evidence {
            reach: Reach::OnItsTests,
            harness: Harness::Libtest(Threads::One),
            packages: &[&quiet],
        }),
        Standing::SingleThreaded
    );
}

#[test]
fn a_spawn_anywhere_in_the_closure_makes_it_concurrent_and_says_where() {
    let quiet = quiet();
    let spawning = package(
        "pool@2.0.0",
        &[(
            "src/work.rs",
            "pub fn go() {\n    std::thread::spawn(|| {});\n}",
        )],
        false,
    );
    let told = standing(Evidence {
        reach: Reach::OnItsTests,
        harness: Harness::Libtest(Threads::One),
        packages: &[&quiet, &spawning],
    });
    assert_eq!(
        told,
        Standing::Concurrent {
            because: vec![Because::Starts {
                package: "pool@2.0.0".to_owned(),
                path: "src/work.rs".to_owned(),
                line: 2,
                what: Starts::Spawn,
            }]
        },
        "a dependency that can start a thread starts it in the binary that links it"
    );
}

#[test]
fn reach_off_the_tests_threads_is_concurrency_the_sources_did_not_show() {
    let quiet = quiet();
    assert_eq!(
        standing(Evidence {
            reach: Reach::OffItsTests,
            harness: Harness::Libtest(Threads::One),
            packages: &[&quiet],
        }),
        Standing::Concurrent {
            because: vec![Because::LooseReach]
        }
    );
}

#[test]
fn native_code_unread_sources_no_touch_and_another_harness_prove_nothing() {
    let native = package(
        "sys@0.1.0",
        &[("src/lib.rs", "extern \"C\" { fn work(); }")],
        false,
    );
    let linked = package("zlib-sys@1.0.0", &[], true);
    let mut unread = quiet();
    unread.unread.push("src/broken.rs".to_owned());
    let told = standing(Evidence {
        reach: Reach::NotRecorded,
        harness: Harness::Other,
        packages: &[&native, &linked, &unread],
    });
    let Standing::NotProven { why } = told else {
        panic!("nothing about this binary could be looked at whole: {told:?}");
    };
    assert_eq!(
        why,
        [
            Unproven::NoTouch,
            Unproven::NotLibtest,
            Unproven::Unread {
                package: "quiet@1.0.0".to_owned(),
                path: "src/broken.rs".to_owned(),
            },
            Unproven::NativeCode {
                package: "sys@0.1.0".to_owned(),
                by: "src/lib.rs:1".to_owned(),
            },
            Unproven::NativeCode {
                package: "zlib-sys@1.0.0".to_owned(),
                by: "links".to_owned(),
            },
        ],
        "every premise that could not be looked at is named, in order"
    );
}

#[test]
fn a_known_spawn_is_concurrent_even_where_something_else_could_not_be_read() {
    let spawning = package(
        "pool@2.0.0",
        &[("src/lib.rs", "fn a() { rayon::join(|| 1, || 2); }")],
        false,
    );
    let native = package(
        "sys@0.1.0",
        &[("src/lib.rs", "extern \"C\" { fn work(); }")],
        false,
    );
    let told = standing(Evidence {
        reach: Reach::OnItsTests,
        harness: Harness::Libtest(Threads::One),
        packages: &[&spawning, &native],
    });
    assert!(
        matches!(told, Standing::Concurrent { .. }),
        "what is known to start a thread is said, rather than hidden behind what could not be read: {told:?}"
    );
}

#[test]
fn a_package_is_read_whole_outside_its_build_output_and_what_cannot_be_read_is_named() {
    let dir = tempfile::tempdir().expect("a directory");
    let root = dir.path();
    for (path, text) in [
        ("src/lib.rs", "pub fn go() { std::thread::spawn(|| {}); }"),
        ("tests/quiet.rs", "#[test] fn t() {}"),
        (
            "target/debug/build/out.rs",
            "fn a() { std::thread::spawn(|| {}); }",
        ),
        (".git/hook.rs", "fn a() { std::thread::spawn(|| {}); }"),
        ("src/broken.rs", "fn a( {"),
    ] {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("a directory");
        std::fs::write(at, text).expect("written");
    }
    std::fs::write(root.join("src/latin1.rs"), [0xff_u8, 0xfe]).expect("written");
    let scan = njutest::concurrency::read::directory("pkg@1.0.0", false, root);
    let places: Vec<(&str, usize)> = scan
        .found
        .iter()
        .map(|(path, found)| (path.as_str(), found.line))
        .collect();
    assert_eq!(
        places,
        [("src/lib.rs", 1)],
        "build output and hidden directories are not the package's sources"
    );
    assert_eq!(
        scan.unread,
        ["src/broken.rs", "src/latin1.rs"],
        "a file that was not read is named, never read as one that starts nothing"
    );
}

#[test]
fn a_harness_that_runs_tests_side_by_side_is_concurrent_however_quiet_its_closure() {
    let quiet = quiet();
    assert_eq!(
        standing(Evidence {
            reach: Reach::OnItsTests,
            harness: Harness::Libtest(Threads::Many),
            packages: &[&quiet],
        }),
        Standing::Concurrent {
            because: vec![Because::ParallelTests]
        },
        "two tests on two threads of one process interleave over whatever they share"
    );
}

#[test]
fn a_doctest_binary_is_never_proven_whatever_its_reach_says() {
    let quiet = quiet();
    assert_eq!(
        standing(Evidence {
            reach: Reach::OnItsTests,
            harness: Harness::Doctest,
            packages: &[&quiet],
        }),
        Standing::NotProven {
            why: vec![Unproven::Doctest]
        },
        "rustdoc runs code the scan does not read, where no reach is recorded"
    );
}

#[test]
fn only_an_explicit_single_test_thread_is_one_thread() {
    let threads = |args: &[&str]| {
        njutest::concurrency::proof::threads_of(
            &args.iter().map(|one| (*one).to_owned()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(threads(&["--test-threads=1"]), Threads::One);
    assert_eq!(
        threads(&["--nocapture", "--test-threads", "1"]),
        Threads::One
    );
    assert_eq!(
        threads(&[]),
        Threads::Many,
        "libtest's default is every processor"
    );
    assert_eq!(threads(&["--test-threads=4"]), Threads::Many);
    assert_eq!(
        threads(&["--test-threads=1", "--test-threads=1"]),
        Threads::Many,
        "a flag given twice is one libtest refuses, and nothing is read from it"
    );
    assert_eq!(threads(&["--test-threads"]), Threads::Many);
    assert_eq!(
        threads(&["--skip", "--test-threads=1"]),
        Threads::Many,
        "the value of `--skip` is a name to skip, not a thread count"
    );
    assert_eq!(
        threads(&["--", "--test-threads=1"]),
        Threads::Many,
        "after `--` every word is a filter"
    );
    assert_eq!(
        threads(&["--exact", "tests::a", "--test-threads=1"]),
        Threads::One,
        "a filter and a flag without a value leave the count alone"
    );
    assert_eq!(
        threads(&["--unheard-of", "--test-threads=1"]),
        Threads::Many,
        "an option this reading does not know may take the next word, so nothing is read past it"
    );
}
