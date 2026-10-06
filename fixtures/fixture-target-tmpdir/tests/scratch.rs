// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[test]
fn what_is_kept_in_the_targets_scratch_reads_back() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("kept");
    std::fs::write(&path, fixture_target_tmpdir::kept(41)).expect("the scratch takes a write");
    let read = std::fs::read_to_string(&path).expect("the scratch reads back");
    assert_eq!(read, "42");
}
