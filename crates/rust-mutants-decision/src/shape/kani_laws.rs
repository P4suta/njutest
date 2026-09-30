// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{OPENERS, opens_a_block};

#[kani::proof]
#[kani::unwind(12)]
fn a_block_opener_is_held_and_a_word_that_is_no_opener_is_not() {
    let index = kani::any::<usize>();
    kani::assume(index < OPENERS.len());
    let opener = OPENERS[index];
    let mut held = [0u8; 8];
    let mut length = 0;
    for &byte in opener.as_bytes() {
        held[length] = byte;
        length += 1;
    }
    let word = core::str::from_utf8(&held[..length]).expect("an opener is ASCII");
    kani::assert(
        opens_a_block(word),
        "njutest-law-assertion:opener-held-alone",
    );
    let tail = kani::any::<u8>();
    kani::assume(tail.is_ascii());
    kani::assume(!tail.is_ascii_alphanumeric() && tail != b'_');
    let mut followed = held;
    followed[length] = tail;
    let followed = core::str::from_utf8(&followed[..length + 1]).expect("an ASCII suffix");
    kani::assert(
        opens_a_block(followed),
        "njutest-law-assertion:opener-held-before-anything",
    );
    let mut word = [0u8; 8];
    for byte in &mut word {
        *byte = kani::any::<u8>();
        kani::assume(byte.is_ascii_alphanumeric() || *byte == b'_');
    }
    let alone = core::str::from_utf8(&word).expect("an ASCII word");
    kani::assert(
        !opens_a_block(alone),
        "njutest-law-assertion:word-of-eight-held-not",
    );
    kani::cover!(opens_a_block(followed), "njutest-law-branch:held");
    kani::cover!(!opens_a_block(alone), "njutest-law-branch:not-held");
    kani::cover!(true, "njutest-law-reached");
}
