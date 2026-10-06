// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[test]
fn a_file_kept_where_tmpdir_names_reads_back() {
    let kept = fixture_temporary::next(41);
    let read = fixture_temporary::round_trip("kept", &kept, true).expect("the round trip");
    assert_eq!(read, "42");
}

#[test]
fn a_file_kept_in_the_temporary_directory_of_std_reads_back() {
    let read = fixture_temporary::round_trip("std", "std", false).expect("the round trip");
    assert_eq!(read, "std");
}
