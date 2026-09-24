// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Something a feature decides.

/// How loud the library is.
pub fn volume() -> u32 {
    if cfg!(feature = "loud") { 11 } else { 1 }
}
