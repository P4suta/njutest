// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use core::convert::Infallible;
use core::num::NonZeroU32;

use super::{Descent, Link, Scope, could_have_started, descent};

const HELD: u32 = 4;
const BOUND: u32 = 4;

fn symbolic_link() -> Option<Link> {
    if kani::any() {
        let parent = kani::any::<u32>();
        kani::assume(parent <= HELD + 1);
        let born = kani::any::<u64>();
        kani::assume(born < 4);
        Some(Link { parent, born })
    } else {
        None
    }
}

fn held(rows: &[Option<Link>; 4], pid: u32) -> Option<Link> {
    let [first, second, third, fourth] = *rows;
    match pid {
        1 => first,
        2 => second,
        3 => third,
        4 => fourth,
        _ => None,
    }
}

fn up(rows: &[Option<Link>; 4], pid: u32, parents: u32) -> Option<u32> {
    let mut child = held(rows, pid)?;
    for _ in 1..parents {
        let parent = held(rows, child.parent)?;
        if parent.born > child.born {
            return None;
        }
        child = parent;
    }
    Some(child.parent)
}

fn passes_neither(rows: &[Option<Link>; 4], pid: u32, parents: u32, ancestor: u32) -> bool {
    (1..parents).all(|below| up(rows, pid, below).is_some_and(|id| id != 0 && id != ancestor))
}

fn well_founded(rows: &[Option<Link>; 4], ancestor: u32) -> bool {
    (1..=HELD).all(|pid| match held(rows, pid) {
        None => true,
        Some(row) => {
            row.parent == 0
                || row.parent == ancestor
                || (row.parent < pid
                    && held(rows, row.parent).is_some_and(|parent| parent.born <= row.born))
        }
    })
}

#[kani::proof]
#[kani::unwind(7)]
fn a_descent_is_only_what_the_parents_read_say() {
    let rows = [
        symbolic_link(),
        symbolic_link(),
        symbolic_link(),
        symbolic_link(),
    ];
    let pid = kani::any::<u32>();
    kani::assume(pid >= 1 && pid <= HELD + 1);
    let ancestor = kani::any::<NonZeroU32>();
    kani::assume(ancestor.get() <= HELD + 1);
    let Ok(found) = descent(pid, ancestor, BOUND, |asked| {
        Ok::<_, Infallible>(held(&rows, asked))
    });
    let ancestor = ancestor.get();
    let reaches = match found {
        Descent::Reaches(0) => pid == ancestor,
        Descent::Reaches(parents) => {
            pid != ancestor
                && parents <= BOUND
                && up(&rows, pid, parents) == Some(ancestor)
                && passes_neither(&rows, pid, parents, ancestor)
        }
        Descent::Apart | Descent::Ended | Descent::Broken => true,
    };
    kani::assert(
        reaches,
        "njutest-law-assertion:reaches-only-through-held-ordered-parents",
    );
    let apart = found != Descent::Apart
        || (pid != ancestor
            && (1..=BOUND).any(|parents| {
                up(&rows, pid, parents) == Some(0) && passes_neither(&rows, pid, parents, ancestor)
            }));
    kani::assert(apart, "njutest-law-assertion:apart-only-past-no-ancestor");
    kani::assert(
        (found == Descent::Ended) == (pid != ancestor && held(&rows, pid).is_none()),
        "njutest-law-assertion:ended-iff-not-held",
    );
    kani::assert(
        !well_founded(&rows, ancestor)
            || held(&rows, pid).is_none()
            || matches!(found, Descent::Reaches(_) | Descent::Apart),
        "njutest-law-assertion:a-well-founded-table-decides",
    );
    kani::cover!(
        matches!(found, Descent::Reaches(parents) if parents > 1),
        "njutest-law-branch:reaches"
    );
    kani::cover!(found == Descent::Apart, "njutest-law-branch:apart");
    kani::cover!(found == Descent::Ended, "njutest-law-branch:ended");
    kani::cover!(found == Descent::Broken, "njutest-law-branch:broken");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_child_this_process_started_is_never_reaped_as_handed_over() {
    let pid = kani::any::<u32>();
    let own = Scope {
        group: kani::any(),
        session: kani::any(),
    };
    let started = could_have_started(
        pid,
        Scope {
            group: pid,
            session: own.session,
        },
        own,
    ) && could_have_started(
        pid,
        Scope {
            group: pid,
            session: pid,
        },
        own,
    ) && could_have_started(pid, own, own);
    kani::assert(started, "njutest-law-assertion:started-shapes-kept");
    let scope = Scope {
        group: kani::any(),
        session: kani::any(),
    };
    let foreign = (scope.group != pid && scope.group != own.group)
        || (scope.session != pid && scope.session != own.session);
    kani::assert(
        !foreign || !could_have_started(pid, scope, own),
        "njutest-law-assertion:foreign-scope-handed-over",
    );
    kani::cover!(
        could_have_started(pid, scope, own),
        "njutest-law-branch:kept"
    );
    kani::cover!(
        !could_have_started(pid, scope, own),
        "njutest-law-branch:handed-over"
    );
    kani::cover!(true, "njutest-law-reached");
}
