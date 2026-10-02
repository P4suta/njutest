// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Each test counts up from whatever the previous run of it left, through one kind of call that writes.

use std::path::{Path, PathBuf};

fn kept(name: &str) -> PathBuf {
    let directory = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .expect("every run gives its tests a temporary directory")
        .join("fixture-durable-calls");
    std::fs::create_dir_all(&directory).expect("a directory to keep the count in");
    directory.join(name)
}

fn load(path: &Path) -> u32 {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .strip_prefix("count=")
            .expect("the file holds a count")
            .parse()
            .expect("the count is a number"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("the count reads: {error}"),
    }
}

fn goes_up(name: &str, save: fn(&Path, &str)) {
    let path = kept(name);
    let count = load(&path);
    save(&path, &format!("count={}", count + 1));
    assert_eq!(load(&path), count + 1);
}

#[test]
fn a_count_copied_into_place_goes_up() {
    goes_up("copied", fixture_durable_calls::save_by_copy);
}

#[test]
fn a_count_synced_goes_up() {
    goes_up("synced", fixture_durable_calls::save_synced);
}

#[test]
fn a_count_cut_and_written_again_goes_up() {
    goes_up("cut", fixture_durable_calls::save_cut);
}

#[test]
fn a_count_kept_through_a_buffer_goes_up() {
    goes_up("buffered", fixture_durable_calls::save_buffered);
}

#[test]
fn a_count_kept_beside_a_guard_goes_up() {
    goes_up("guarded", fixture_durable_calls::save_guarded);
}
