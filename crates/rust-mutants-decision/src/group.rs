// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How much of a process group a stop reached, from what the kernel answered its signals with and who else the group was seen to hold.

/// What stopping a group reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
#[must_use = "a stop that reached only the leader leaves the rest of the group running"]
pub enum Stopped {
    /// Every process of the group was signalled, or none besides its unreaped leader was left.
    Group,
    /// The kernel refused the group whole and only its leader was signalled, with other members still in the group or not seen.
    LeaderOnly,
}

/// What the kernel answered one signal with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Delivered {
    /// It was sent.
    Sent,
    /// Nothing by that id was left to send it to.
    Gone,
    /// Sending it is beyond this process's authority for some process it names.
    Refused,
    /// Any other failure.
    Failed,
}

/// Who besides its leader a group was seen to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Others {
    /// Nobody.
    Nobody,
    /// Somebody still running.
    Somebody,
    /// The group could not be looked at.
    Unseen,
}

/// What a group stop comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDecision {
    /// It reached this much.
    Reached(Stopped),
    /// It failed.
    Failed,
}

/// What a group stop comes to, from what the group's signal got, what its leader's got when the group was refused, and who else the group was seen to hold, as the engine's `tests/testdata/group-stop.tsv` lists for every combination.
#[must_use]
pub const fn decide_stop(group: Delivered, leader: Delivered, others: Others) -> StopDecision {
    match group {
        Delivered::Sent | Delivered::Gone => StopDecision::Reached(Stopped::Group),
        Delivered::Failed => StopDecision::Failed,
        Delivered::Refused => match (leader, others) {
            (Delivered::Refused | Delivered::Failed, _) => StopDecision::Failed,
            (Delivered::Sent | Delivered::Gone, Others::Nobody) => {
                StopDecision::Reached(Stopped::Group)
            }
            (Delivered::Sent | Delivered::Gone, Others::Somebody | Others::Unseen) => {
                StopDecision::Reached(Stopped::LeaderOnly)
            }
        },
    }
}

/// Whether `decision` is what a group stop can come to from what its signals got and who else it was seen to hold: the rule a stop's classification is checked by where it runs, said apart from [`decide_stop`].
#[must_use]
pub fn agrees(group: Delivered, leader: Delivered, others: Others, decision: StopDecision) -> bool {
    match decision {
        StopDecision::Reached(Stopped::Group) => {
            matches!(group, Delivered::Sent | Delivered::Gone)
                || (group == Delivered::Refused
                    && matches!(leader, Delivered::Sent | Delivered::Gone)
                    && others == Others::Nobody)
        }
        StopDecision::Reached(Stopped::LeaderOnly) => {
            group == Delivered::Refused
                && matches!(leader, Delivered::Sent | Delivered::Gone)
                && matches!(others, Others::Somebody | Others::Unseen)
        }
        StopDecision::Failed => {
            group == Delivered::Failed
                || (group == Delivered::Refused
                    && matches!(leader, Delivered::Refused | Delivered::Failed))
        }
    }
}

#[cfg(test)]
mod tests;
