// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Located, located, moved, narrows};

#[kani::proof]
fn a_locator_names_what_its_count_says() {
    let held = kani::any::<usize>();
    let count = kani::any::<Option<u32>>();
    let said = located(held, count);
    let stated = match count {
        Some(wanted) => usize::try_from(wanted) == Ok(held),
        None => held == 1,
    };
    let ruled = match count {
        _ if held == 0 => Located::Nothing,
        _ if stated => Located::Named,
        Some(wanted) => Located::Counted { wanted },
        None => Located::Several,
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:located-as-the-count-says",
    );
    kani::cover!(said == Located::Named, "njutest-law-branch:named");
    kani::cover!(said == Located::Nothing, "njutest-law-branch:nothing");
    kani::cover!(said == Located::Several, "njutest-law-branch:several");
    kani::cover!(
        matches!(said, Located::Counted { .. }),
        "njutest-law-branch:counted"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_line_narrows_only_a_choice() {
    let matching = kani::any::<usize>();
    kani::assert(
        narrows(matching) == (matching >= 2),
        "njutest-law-assertion:narrows-only-several",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_claim_moved_only_off_its_line() {
    let line = kani::any::<Option<u32>>();
    let first = kani::any::<Option<u32>>();
    let said = moved(line, first);
    let ruled = match (line, first) {
        (Some(from), Some(to)) if from != to => Some((from, to)),
        (Some(_) | None, Some(_) | None) => None,
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:moved-iff-another-line",
    );
    kani::cover!(said.is_some(), "njutest-law-branch:moved");
    kani::cover!(said.is_none(), "njutest-law-branch:stayed");
    kani::cover!(true, "njutest-law-reached");
}
