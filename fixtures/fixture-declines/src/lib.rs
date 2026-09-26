// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tests that decline to measure where the machine cannot hold what they ask of it, as ADR 0043 lets them.

/// Whether this machine can share blocks between two files; none this fixture runs on can.
pub fn can_share() -> bool {
    1 > 2
}

/// Whether this machine can hold what the arithmetic tests ask of it; every one can.
pub fn can_measure() -> bool {
    2 > 1
}

/// Twice `value`, which only a test that then declines reaches.
pub fn shared(value: u32) -> u32 {
    value * 3 - value
}

/// `value` and one more, which a declining test and a test that never looks at the answer reach.
pub fn counted(value: u32) -> u32 {
    value + 1
}

/// `value` and ten more, which a test holds wherever it measures.
pub fn measured(value: u32) -> u32 {
    value + 10
}

/// Half of `value`, which a unit test that then declines reaches, and an integration test that never looks at the answer measures.
pub fn halved(value: u32) -> u32 {
    value / 2
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    /// Says on the engine's channel, when a run opened one, that the test on this thread did not measure here, and why: its last act, in one write, under the name libtest gave its thread.
    fn decline(why: &str) {
        println!("skipping: {why}");
        if let Some(path) = std::env::var_os("RUST_MUTANTS_DECLINE_NOTICE") {
            let name = std::thread::current().name().unwrap_or_default().to_owned();
            let mut notice = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("the notice the engine named opens");
            notice
                .write_all(format!("{name}\t{why}\n").as_bytes())
                .expect("the decline is written");
        }
    }

    #[test]
    fn doubles_what_it_shares() {
        let doubled = super::shared(3);
        if !super::can_share() {
            decline("this machine cannot share blocks");
            return;
        }
        assert_eq!(doubled, 6);
    }

    #[test]
    fn counts_what_it_shares() {
        let counted = super::counted(1);
        if !super::can_share() {
            decline("this machine cannot share blocks");
            return;
        }
        assert_eq!(counted, 2);
    }

    #[test]
    fn counts_without_looking() {
        std::hint::black_box(super::counted(1));
    }

    #[test]
    fn halves_what_it_shares() {
        let halved = super::halved(8);
        if !super::can_share() {
            decline("this machine cannot share blocks");
            return;
        }
        assert_eq!(halved, 4);
    }

    #[test]
    fn adds_ten() {
        if !super::can_measure() {
            decline("this machine cannot add");
            return;
        }
        assert_eq!(super::measured(1), 11);
    }
}
