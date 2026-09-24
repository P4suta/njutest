// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A binary is proven single-threaded only where every premise holds, and each premise that fails is named.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use njutest::concurrency::proof::{
    Because, Evidence, PackageScan, Reach, Standing, Unproven, standing,
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
            libtest: true,
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
        libtest: true,
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
            libtest: true,
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
        libtest: false,
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
        libtest: true,
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
fn every_package_of_a_closure_is_read_once_and_alike_however_many_read_at_once() {
    let dir = tempfile::tempdir().expect("a directory");
    let mut packages = Vec::new();
    for (name, text) in [
        ("quiet", "pub fn go() {}"),
        ("spawns", "pub fn go() { std::thread::spawn(|| {}); }"),
        ("broken", "fn a( {"),
    ] {
        let root = dir.path().join(name);
        std::fs::create_dir_all(root.join("src")).expect("a directory");
        std::fs::write(root.join("src/lib.rs"), text).expect("written");
        packages.push(serde_json::json!({
            "id": format!("{name} 1.0.0"),
            "name": name,
            "version": "1.0.0",
            "manifest_path": root.join("Cargo.toml"),
        }));
    }
    let document = serde_json::json!({
        "version": 1,
        "workspace_root": dir.path(),
        "target_directory": dir.path().join("target"),
        "workspace_members": [],
        "packages": packages,
    })
    .to_string();
    let metadata =
        rust_mutants::cargo::Metadata::parse(document.as_bytes()).expect("the document parses");
    let ids = ["broken 1.0.0", "gone 1.0.0", "quiet 1.0.0", "spawns 1.0.0"];
    let alone =
        njutest::assure::concurrency::scans(&metadata, &ids, 1).expect("read by one worker");
    let together =
        njutest::assure::concurrency::scans(&metadata, &ids, 3).expect("read by three workers");
    assert_eq!(
        alone, together,
        "how many packages are read at once changes nothing about what is read"
    );
    let summary: Vec<(&str, usize, &[String])> = ids
        .iter()
        .map(|id| {
            let scan = alone.get(*id).expect("every id is read");
            (
                scan.package.as_str(),
                scan.found.len(),
                scan.unread.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("broken@1.0.0", 0, &["src/lib.rs".to_owned()][..]),
            ("gone 1.0.0", 0, &["Cargo.toml".to_owned()][..]),
            ("quiet@1.0.0", 0, &[][..]),
            ("spawns@1.0.0", 1, &[][..]),
        ],
        "each package is read once, and one the metadata lacks is named unread rather than skipped"
    );
}
