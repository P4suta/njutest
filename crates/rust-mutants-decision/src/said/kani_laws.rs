// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Record, STEP_CHECKS, canonical, names};

fn spelled() -> ([u8; 11], usize, i32) {
    let negative = kani::any::<bool>();
    let count = kani::any::<usize>();
    kani::assume((1..=10).contains(&count));
    let mut held = [0u8; 11];
    let mut length = usize::from(negative);
    if negative {
        held[0] = b'-';
    }
    let mut value = 0u64;
    for at in 0..count {
        let digit = kani::any::<u8>();
        kani::assume(digit < 10);
        kani::assume(at != 0 || count == 1 || digit != 0);
        held[length] = b'0' + digit;
        length += 1;
        value = value * 10 + u64::from(digit);
    }
    kani::assume(!negative || value != 0);
    let number = if negative {
        kani::assume(value <= 2_147_483_648);
        -(value as i64)
    } else {
        kani::assume(value <= 2_147_483_647);
        value as i64
    };
    (held, length, number as i32)
}

#[kani::proof]
#[kani::unwind(12)]
fn every_number_spelled_as_the_runtime_spells_one_reads_back_itself() {
    let (digits, length, status) = spelled();
    let text = core::str::from_utf8(&digits[..length]).expect("a canonical number is ASCII");
    let said = canonical(text);
    kani::assert(
        said == Some(status),
        "njutest-law-assertion:spelled-number-reads-back",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(36)]
fn a_stated_stop_requires_its_own_status_and_a_known_check() {
    let index = kani::any::<usize>();
    kani::assume(index < STEP_CHECKS.len());
    let known = kani::any::<bool>();
    let check = if known {
        STEP_CHECKS[index]
    } else {
        "unknown-step"
    };
    let said = Record {
        status: kani::any::<i32>(),
        check,
        os: kani::any::<i32>(),
    };
    let status = kani::any::<i32>();
    kani::assert(
        names(said, status) == (known && said.status == status),
        "njutest-law-assertion:stated-only-for-its-check-and-status",
    );
    kani::cover!(true, "njutest-law-reached");
}
