// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tell rustc about cfg values owned by the product crates rather than this harness.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"testkit\"))");
}
