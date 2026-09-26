// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test that passes only on schedules where a thread it spawns answers in time.

/// One more, which a thread the test spawns computes.
#[must_use]
pub fn work(n: u32) -> u32 {
    n + 1
}

/// Two more, which nothing reaches.
#[must_use]
pub fn unreached(n: u32) -> u32 {
    n + 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_message_arrives_in_time() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let begun = std::time::Instant::now();
            let answer = super::work(1);
            sender
                .send((answer, begun.elapsed()))
                .expect("the receiver waits");
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        let (answer, took) = receiver.recv().expect("the thread answers");
        assert_eq!(answer, 2);
        assert!(
            took < std::time::Duration::from_millis(90),
            "the answer took {took:?}"
        );
    }
}
