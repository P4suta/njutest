// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that holds the text of one of its own modules, as a build that embeds its source does.

pub mod twice;

/// The module `twice` as its file holds it, named beside this file.
pub const TWICE_SOURCE: &str = include_str!("twice.rs");

/// The same file's bytes, named from the package's own directory.
pub const TWICE_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/twice.rs"));
