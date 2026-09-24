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
    let scan = njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        root,
        (&[], &njutest::concurrency::read::Compiled::default()),
    )
    .expect("nothing ran short");
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

#[test]
fn a_file_the_compiler_read_is_scanned_wherever_it_lives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("pkg");
    let generated = dir.path().join("out");
    for (at, text) in [
        (root.join("src/lib.rs"), "pub fn quiet() {}\n"),
        (
            root.join(".gen/hidden.rs"),
            "pub fn hidden() { std::thread::spawn(|| {}); }\n",
        ),
        (
            generated.join("gen.rs"),
            "pub fn generated() { std::thread::spawn(|| {}); }\n",
        ),
    ] {
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("created");
        std::fs::write(at, text).expect("written");
    }
    let mut compiled = njutest::concurrency::read::Compiled::default();
    compiled.sources.insert(
        "pkg".to_owned(),
        [
            root.join("src/lib.rs"),
            root.join(".gen/hidden.rs"),
            generated.join("gen.rs"),
            generated.join("gone.rs"),
        ]
        .into_iter()
        .collect(),
    );
    let scan = njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        &root,
        (&["pkg".to_owned()], &compiled),
    )
    .expect("nothing ran short");
    let files: std::collections::BTreeSet<&str> = scan
        .found
        .iter()
        .map(|(path, _found)| path.as_str())
        .collect();
    assert!(
        files.contains(".gen/hidden.rs") && files.iter().any(|path| path.ends_with("out/gen.rs")),
        "a file the compiler read is a source of the crate whatever directory it is in: a \
         hidden directory `#[path]` points into, or the OUT_DIR a build script wrote into and \
         `include!` pulled in. {files:?}"
    );
    assert!(
        scan.unread.iter().any(|path| path.ends_with("out/gone.rs")),
        "and a file the compiler read that is not there to scan is named as unread: {:?}",
        scan.unread
    );
}

#[test]
fn a_build_script_that_asks_the_linker_for_a_library_links_native_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("pkg");
    std::fs::create_dir_all(root.join("src")).expect("created");
    std::fs::write(root.join("src/lib.rs"), "pub fn quiet() {}\n").expect("written");
    let mut compiled = njutest::concurrency::read::Compiled::default();
    compiled.linking.insert("pkg".to_owned());
    let scan = njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        &root,
        (&["pkg".to_owned()], &compiled),
    )
    .expect("nothing ran short");
    assert!(
        scan.links,
        "an object a build script links can start a thread from its constructor with no \
         `extern` and no `links` key anywhere in the package"
    );
}

#[test]
fn a_file_read_as_data_is_not_a_source_and_one_read_as_code_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("pkg");
    std::fs::create_dir_all(root.join("src")).expect("created");
    std::fs::write(
        root.join("src/lib.rs"),
        "#![doc = include_str!(\"../README.md\")]\npub fn quiet() {}\n",
    )
    .expect("written");
    std::fs::write(
        root.join("README.md"),
        "Call `std::thread::spawn` yourself.\n",
    )
    .expect("written");
    let mut compiled = njutest::concurrency::read::Compiled::default();
    compiled.sources.insert(
        "pkg".to_owned(),
        [root.join("src/lib.rs"), root.join("README.md")]
            .into_iter()
            .collect(),
    );
    let scan = njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        &root,
        (&["pkg".to_owned()], &compiled),
    )
    .expect("nothing ran short");
    assert!(
        scan.found.is_empty() && scan.unread.is_empty(),
        "a README a crate documents itself with is data the compiler read, not code: {scan:?}"
    );
    std::fs::write(
        root.join("src/lib.rs"),
        "include!(\"../gen.in\");\npub fn quiet() {}\n",
    )
    .expect("written");
    std::fs::write(
        root.join("gen.in"),
        "pub fn go() { std::thread::spawn(|| {}); }\n",
    )
    .expect("written");
    compiled.sources.insert(
        "pkg".to_owned(),
        [root.join("src/lib.rs"), root.join("gen.in")]
            .into_iter()
            .collect(),
    );
    let scan = njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        &root,
        (&["pkg".to_owned()], &compiled),
    )
    .expect("nothing ran short");
    assert!(
        scan.found.iter().any(|(path, _found)| path == "gen.in"),
        "while a file `include!` pulls in is code, whatever it is named: {scan:?}"
    );
}

fn quiet_package(dir: &std::path::Path) -> std::path::PathBuf {
    let root = dir.join("pkg");
    std::fs::create_dir_all(root.join("src")).expect("created");
    std::fs::write(root.join("src/lib.rs"), "pub fn quiet() {}\n").expect("written");
    root
}

fn scanned_against(root: &std::path::Path, target: &std::path::Path) -> PackageScan {
    let compiled = njutest::concurrency::read::Compiled::read(target).expect("nothing ran short");
    njutest::concurrency::read::compiled_directory(
        ("pkg@1.0.0", false),
        root,
        (&["pkg".to_owned()], &compiled),
    )
    .expect("nothing ran short")
}

#[test]
fn what_the_compiler_read_is_taken_from_the_dependency_files_of_the_build() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = quiet_package(dir.path());
    let generated = dir.path().join("out/gen.rs");
    std::fs::create_dir_all(generated.parent().expect("a parent")).expect("created");
    std::fs::write(&generated, "pub fn g() { std::thread::spawn(|| {}); }\n").expect("written");
    let deps = dir.path().join("target/debug/deps");
    std::fs::create_dir_all(&deps).expect("created");
    std::fs::write(
        deps.join("pkg-0123abcd.d"),
        format!(
            "{}: {} {}\n",
            deps.join("libpkg-0123abcd.rlib").display(),
            root.join("src/lib.rs").display(),
            generated.display()
        ),
    )
    .expect("written");
    let scan = scanned_against(&root, &dir.path().join("target"));
    assert!(
        scan.found
            .iter()
            .any(|(path, _)| path.ends_with("out/gen.rs"))
            && scan.unread.is_empty(),
        "the file the dependency file lists is read: {scan:?}"
    );
}

#[test]
fn a_build_with_no_dependency_file_proves_nothing_about_what_it_compiled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = quiet_package(dir.path());
    std::fs::create_dir_all(dir.path().join("target/debug/deps")).expect("created");
    let scan = scanned_against(&root, &dir.path().join("target"));
    assert!(
        !scan.unread.is_empty(),
        "with no dependency file, nothing says which files outside the package the compiler \
         read, so the package cannot be said to start nothing: {scan:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_dependency_file_that_cannot_be_read_leaves_its_crate_unread() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = quiet_package(dir.path());
    let deps = dir.path().join("target/debug/deps");
    std::fs::create_dir_all(&deps).expect("created");
    let sealed = deps.join("pkg-0123abcd.d");
    std::fs::write(&sealed, "x: y\n").expect("written");
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).expect("sealed");
    let scan = scanned_against(&root, &dir.path().join("target"));
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o600)).expect("unsealed");
    assert!(
        scan.unread
            .iter()
            .any(|path| path.ends_with("pkg-0123abcd.d")),
        "a dependency file that exists and cannot be read hides what its crate compiled, so it \
         is named as unread rather than read as a list of nothing: {scan:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_build_script_output_that_cannot_be_read_is_taken_to_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = quiet_package(dir.path());
    let deps = dir.path().join("target/debug/deps");
    std::fs::create_dir_all(&deps).expect("created");
    std::fs::write(
        deps.join("pkg-0123abcd.d"),
        format!("x: {}\n", root.join("src/lib.rs").display()),
    )
    .expect("written");
    let run = dir.path().join("target/debug/build/pkg-4567cdef");
    std::fs::create_dir_all(&run).expect("created");
    let sealed = run.join("output");
    std::fs::write(&sealed, "cargo:rustc-link-lib=ctor\n").expect("written");
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).expect("sealed");
    let scan = scanned_against(&root, &dir.path().join("target"));
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o600)).expect("unsealed");
    assert!(
        scan.links,
        "what a build script told the linker cannot be read, so it may have linked native code: \
         {scan:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_failure_to_look_says_absent_unreadable_or_that_the_process_ran_out() {
    use njutest::observe::{Observed, SourceReadError, failed};
    let path = std::path::Path::new("somewhere");
    let meaning = |errno: rustix::io::Errno| {
        failed::<()>(
            path,
            std::io::Error::from_raw_os_error(errno.raw_os_error()),
        )
    };
    for out in [rustix::io::Errno::MFILE, rustix::io::Errno::NFILE] {
        assert!(
            matches!(meaning(out), Err(SourceReadError::Exhausted { .. })),
            "a process out of descriptors says nothing about the path, so it is an error and never \
             an unread file, and whether a binary is proven cannot depend on how many read at \
             once: {out:?}"
        );
    }
    assert!(
        matches!(
            failed::<()>(path, std::io::Error::from(std::io::ErrorKind::OutOfMemory)),
            Err(SourceReadError::Exhausted { .. })
        ),
        "nor does a process out of memory"
    );
    assert_eq!(
        meaning(rustix::io::Errno::NOENT).expect("a fact about the path"),
        Observed::Absent
    );
    for there in [
        rustix::io::Errno::ACCESS,
        rustix::io::Errno::NOTDIR,
        rustix::io::Errno::ISDIR,
    ] {
        assert_eq!(
            meaning(there).expect("a fact about the path"),
            Observed::Unreadable,
            "something is there that could not be looked at: {there:?}"
        );
    }
}

#[test]
fn the_files_cargo_and_the_engine_leave_in_a_target_directory_are_not_profiles() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = quiet_package(dir.path());
    let target = dir.path().join("target");
    let deps = target.join("debug/deps");
    std::fs::create_dir_all(&deps).expect("created");
    std::fs::write(
        deps.join("pkg-0123abcd.d"),
        format!("x: {}\n", root.join("src/lib.rs").display()),
    )
    .expect("written");
    for file in [
        ".rustc_info.json",
        "CACHEDIR.TAG",
        "owner.json",
        "owner.lock",
        "debug/.cargo-lock",
        "debug/.cargo-build-lock",
        "debug/.cargo-artifact-lock",
        "witness/CACHEDIR.TAG",
        "witness/.rustc_info.json",
    ] {
        let at = target.join(file);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("created");
        std::fs::write(at, "{}").expect("written");
    }
    let scan = scanned_against(&root, &target);
    assert!(
        scan.unread.is_empty(),
        "a file beside the profiles is not a profile to look inside, and looking inside it is \
         not a directory that could not be read: {:?}",
        scan.unread
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
    let alone = njutest::assure::concurrency::scans(
        &metadata,
        (&ids, &njutest::concurrency::read::Compiled::default()),
        1,
    )
    .expect("read by one worker");
    let together = njutest::assure::concurrency::scans(
        &metadata,
        (&ids, &njutest::concurrency::read::Compiled::default()),
        3,
    )
    .expect("read by three workers");
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
