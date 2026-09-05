// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which target directory a command writes into, and why the rule is one function with a test that names every command.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::path::Path;

use mjutest_cli::build_cache::{
    BuildCache, COMMANDS, Command, Destination, How, LAYOUT_DIR, Layer, MARKER_SCHEMA, layer_for,
};

fn cache(root: &Path) -> BuildCache {
    BuildCache::new(root, "0123456789abcdef0123456789abcdef01234567")
}

#[test]
fn every_command_the_runner_issues_says_which_side_of_the_rule_it_is_on() {
    let expected: [(Command, Destination); 10] = [
        (Command::Metadata, Destination::Base(Layer::Native)),
        (Command::TestBuild, Destination::Base(Layer::Native)),
        (Command::DoctestBuild, Destination::Base(Layer::Native)),
        (Command::CoverageBuild, Destination::Base(Layer::Coverage)),
        (Command::EngineBuild, Destination::Base(Layer::Mutants)),
        (Command::TargetProcess, Destination::Scratch),
        (Command::ControlProcess, Destination::Scratch),
        (Command::MutantProcess, Destination::Scratch),
        (Command::ProviderProcess, Destination::Scratch),
        (Command::LlvmTool, Destination::Nowhere),
    ];
    assert_eq!(
        COMMANDS.len(),
        expected.len(),
        "a new command must choose a side here, not inherit one"
    );
    for (command, destination) in expected {
        assert_eq!(layer_for(command), destination, "{command:?}");
        assert!(COMMANDS.contains(&command), "{command:?} is not enumerated");
    }
}

#[test]
fn nothing_that_runs_the_project_s_tests_writes_where_the_run_does_not_end() {
    for command in COMMANDS {
        let runs_tests = matches!(
            command,
            Command::TargetProcess
                | Command::ControlProcess
                | Command::MutantProcess
                | Command::ProviderProcess
        );
        if runs_tests {
            assert_eq!(layer_for(command), Destination::Scratch, "{command:?}");
        }
    }
}

#[test]
fn the_instrumented_build_never_shares_a_layer_with_the_plain_one() {
    assert_ne!(
        layer_for(Command::CoverageBuild),
        layer_for(Command::TestBuild),
        "RUSTFLAGS invalidate every fingerprint, so sharing would rebuild both every time"
    );
}

#[test]
fn a_layer_is_named_for_the_layout_the_flavour_and_the_compiler() {
    let root = tempfile::tempdir().expect("a temporary root");
    let cache = cache(root.path());
    let dir = cache.dir(Layer::Coverage);

    assert_eq!(LAYOUT_DIR, "build-v1");
    assert!(dir.starts_with(root.path()), "{}", dir.display());
    let tail: Vec<String> = dir
        .strip_prefix(root.path())
        .expect("below the root")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        tail,
        [
            LAYOUT_DIR,
            "coverage",
            "0123456789abcdef0123456789abcdef01234567"
        ],
        "the layout is versioned in the name, so a later one is a new directory"
    );
}

#[test]
fn preparing_a_layer_makes_it_and_leaves_the_marker_that_says_who_made_it() {
    let root = tempfile::tempdir().expect("a temporary root");
    let cache = cache(root.path());

    let dir = cache.prepare(Layer::Native).expect("the layer");
    assert!(dir.is_dir());
    let marker: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dir.join(mjutest_cli::build_cache::MARKER_NAME)).expect("a marker"),
    )
    .expect("JSON");
    assert_eq!(MARKER_SCHEMA, "mjutest-build-cache-v1");
    assert_eq!(marker["schema"], MARKER_SCHEMA);
    assert_eq!(marker["layer"], "native");
    assert_eq!(cache.prepare(Layer::Native).expect("again"), dir);
}

#[test]
fn a_directory_full_of_somebody_else_s_files_is_refused_untouched() {
    let root = tempfile::tempdir().expect("a temporary root");
    let cache = cache(root.path());
    let dir = cache.dir(Layer::Mutants);
    fs::create_dir_all(&dir).expect("the directory");
    fs::write(dir.join("important"), b"not ours").expect("their file");

    let error = cache
        .prepare(Layer::Mutants)
        .expect_err("a directory carrying none of our names is refused");
    assert_eq!(error.code().code, "MJ8002");
    assert!(
        fs::read(dir.join("important")).expect("still there") == b"not ours",
        "refused without writing anything into it"
    );
}

#[test]
fn a_command_that_compiles_is_given_the_flag_and_a_test_process_the_variable() {
    let root = tempfile::tempdir().expect("a temporary root");
    let scratch = root.path().join("run/build");
    let cache = cache(root.path());

    let compiling = cache
        .placement(Command::TestBuild, &scratch)
        .expect("a placement");
    assert_eq!(compiling.how, How::Flag);
    assert_eq!(compiling.dir, cache.dir(Layer::Native));

    let running = cache
        .placement(Command::TargetProcess, &scratch)
        .expect("a placement");
    assert_eq!(
        running.how,
        How::Environment,
        "--target-dir outranks the environment, which is what lets one command \
         compile into the base layer while its children stay in scratch"
    );
    assert_eq!(running.dir, scratch);

    assert!(
        cache.placement(Command::LlvmTool, &scratch).is_none(),
        "a tool that builds nothing is told nothing"
    );
}

#[test]
fn a_layer_over_its_size_gives_up_its_least_recently_used_artifacts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = BuildCache::new(dir.path(), "abc");
    let layer = cache.prepare(Layer::Native).expect("a layer");
    let deps = layer.join("debug/deps");
    fs::create_dir_all(&deps).expect("mkdir");

    let mut written = Vec::new();
    for (index, name) in ["oldest", "middle", "newest"].into_iter().enumerate() {
        let path = deps.join(format!("{name}.rlib"));
        fs::write(&path, vec![b'x'; 1000]).expect("write");
        let seconds = 1_700_000_000u64.saturating_add(u64::try_from(index).unwrap_or(0) * 60);
        let when = std::time::SystemTime::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(seconds))
            .expect("a time");
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the file opens")
            .set_modified(when)
            .expect("the time is set");
        written.push(path);
    }
    assert!(cache.size() >= 3000);

    let swept = cache.collect(1500);
    assert_eq!(swept.files, 2, "{swept:?}");
    assert_eq!(swept.removed, 2000);
    assert!(swept.busy.is_empty());
    assert!(!written[0].exists(), "the oldest went first");
    assert!(!written[1].exists());
    assert!(written[2].exists(), "the newest is what a build wants");
    assert!(layer.exists(), "the layer itself stays");
}

#[test]
fn a_layer_under_its_size_keeps_everything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = BuildCache::new(dir.path(), "abc");
    let layer = cache.prepare(Layer::Native).expect("a layer");
    let deps = layer.join("debug/deps");
    fs::create_dir_all(&deps).expect("mkdir");
    fs::write(deps.join("one.rlib"), vec![b'x'; 100]).expect("write");

    let swept = cache.collect(1_000_000);
    assert_eq!(swept.files, 0);
    assert_eq!(swept.removed, 0);
    assert!(swept.before >= 100);
    assert!(deps.join("one.rlib").exists());
}

#[test]
fn a_layer_a_build_is_using_gives_up_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = BuildCache::new(dir.path(), "abc");
    let layer = cache.prepare(Layer::Native).expect("a layer");
    let deps = layer.join("debug/deps");
    fs::create_dir_all(&deps).expect("mkdir");
    fs::write(deps.join("one.rlib"), vec![b'x'; 4000]).expect("write");

    let mut held = rust_mutants::tempowner::acquire(&layer.join(".cargo-lock"))
        .expect("the lock opens")
        .expect("nobody holds it");
    let swept = cache.collect(0);
    assert_eq!(swept.files, 0, "{swept:?}");
    assert_eq!(swept.busy, std::slice::from_ref(&layer));
    assert!(deps.join("one.rlib").exists());

    held.release().expect("released");
    let swept = cache.collect(0);
    assert_eq!(swept.files, 1, "{swept:?}");
    assert!(swept.busy.is_empty());
}

#[test]
fn nothing_outside_the_directories_cargo_rebuilds_is_ever_removed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = BuildCache::new(dir.path(), "abc");
    let layer = cache.prepare(Layer::Native).expect("a layer");
    fs::create_dir_all(layer.join("debug/deps")).expect("mkdir");
    fs::write(layer.join("debug/deps/one.rlib"), vec![b'x'; 100]).expect("write");
    fs::write(layer.join("debug/mine.txt"), vec![b'x'; 100]).expect("write");
    fs::write(layer.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172").expect("write");

    let swept = cache.collect(0);
    assert_eq!(swept.files, 1, "only what cargo rebuilds: {swept:?}");
    assert!(layer.join("debug/mine.txt").exists());
    assert!(layer.join("CACHEDIR.TAG").exists());
}
