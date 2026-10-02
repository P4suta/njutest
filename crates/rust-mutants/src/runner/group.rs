// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The shared native process-group owner.

pub use njutest_process::GroupChild;
#[cfg(unix)]
pub use njutest_process::Leader;
