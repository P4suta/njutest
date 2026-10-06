// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether one process descends from another, read a parent at a time from a process table that can change while it is read, and whether an ended child could be one its parent started.

use core::num::NonZeroU32;

/// What a process table says of one process: its parent, and when it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// Its parent's id, or zero where the table shows it none.
    pub parent: u32,
    /// When it started, by the table's clock.
    pub born: u64,
}

/// Where the parents of a process lead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Descent {
    /// To the ancestor, this many parents up.
    Reaches(u32),
    /// To a process with no parent, without passing the ancestor.
    Apart,
    /// The process asked about is not in the table: it has ended.
    Ended,
    /// A parent was not in the table, started after its child, or lay past the bound: the table changed while it was read.
    Broken,
}

/// Where the parents of `pid` lead, read through `link` at most `bound` parents up.
///
/// # Errors
/// Whatever `link` failed with.
pub fn descent<E>(
    pid: u32,
    ancestor: NonZeroU32,
    bound: u32,
    mut link: impl FnMut(u32) -> Result<Option<Link>, E>,
) -> Result<Descent, E> {
    if pid == ancestor.get() {
        return Ok(Descent::Reaches(0));
    }
    let Some(mut child) = link(pid)? else {
        return Ok(Descent::Ended);
    };
    let mut links = 0_u32;
    loop {
        let Some(up) = links.checked_add(1).filter(|up| *up <= bound) else {
            return Ok(Descent::Broken);
        };
        links = up;
        if child.parent == ancestor.get() {
            return Ok(Descent::Reaches(links));
        }
        if child.parent == 0 {
            return Ok(Descent::Apart);
        }
        let Some(parent) = link(child.parent)? else {
            return Ok(Descent::Broken);
        };
        if parent.born > child.born {
            return Ok(Descent::Broken);
        }
        child = parent;
    }
}

/// The process group and the session a process is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    /// Its process group's id.
    pub group: u32,
    /// Its session's id.
    pub session: u32,
}

/// Whether an ended child `pid` in `scope` could be one a process in `own` started, rather than one the kernel handed it when the child's own parent ended: every child a process starts leads its own process group or stays in the starter's, and leads its own session or stays in the starter's.
#[must_use]
pub const fn could_have_started(pid: u32, scope: Scope, own: Scope) -> bool {
    (scope.group == pid || scope.group == own.group)
        && (scope.session == pid || scope.session == own.session)
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
