// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What stops a guest because whoever runs it stopped, which says nothing about the guest.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Flags any one of which, once raised, stops a guest at its next epoch or host call.
#[derive(Debug, Clone)]
pub struct Interrupt {
    /// The flags, any one of which stops the guest.
    flags: Vec<Arc<AtomicBool>>,
}

impl Interrupt {
    /// An interrupt raised whenever any of `flags` is, and never where there is none.
    #[must_use]
    pub const fn of(flags: Vec<Arc<AtomicBool>>) -> Self {
        Self { flags }
    }

    /// Whether any of its flags is raised.
    #[must_use]
    pub fn raised(&self) -> bool {
        self.flags.iter().any(|flag| flag.load(Ordering::SeqCst))
    }
}
