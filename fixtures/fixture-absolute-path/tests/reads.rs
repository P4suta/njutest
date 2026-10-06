// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[test]
fn the_setting_beside_the_manifest_is_read_by_a_relative_path() {
    assert_eq!(
        fixture_absolute_path::setting(false).as_deref(),
        Some("beside the manifest\n")
    );
}
