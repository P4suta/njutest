// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How far each layer of an independent re-decision got, which every audit states for every layer it has.

use std::fmt;

/// How far one layer's re-decision got with one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Everything the run owes the layer was re-decided.
    Rederived,
    /// Something the run owes the layer could not be re-decided, and an unaudited line says what.
    Partly,
    /// The run holds nothing this layer re-decides, and why.
    Absent(&'static str),
}

impl fmt::Display for Coverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rederived => f.write_str("re-decided"),
            Self::Partly => f.write_str("partly re-decided; the unaudited lines say what was not"),
            Self::Absent(why) => write!(f, "nothing to re-decide: {why}"),
        }
    }
}

/// What a layer's closed scope derives about one subject it could be owed, which is the only way a reader may decide that a subject is not there: a scope the recording itself establishes in the complete, or evidence that is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// The recording's own complete scope derives that the subject cannot be there: a catalogue that catalogued none, an accounting that claims none, a configuration that asked for none.
    NothingOwed(&'static str),
    /// The subject is owed and the recording does not carry it, which is neither a pass nor a failure.
    Missing,
}
