// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{CONFINED_HOME, Escape, GIT_GLOBAL_CONFIG, escape};

const PLACED: u8 = 0;

const ELSEWHERE: u8 = 1;

fn symbolic_found() -> Option<u8> {
    let found = kani::any::<Option<u8>>();
    kani::assume(found.is_none_or(|value| value == PLACED || value == ELSEWHERE));
    found
}

fn confined_except(
    changed: &'static str,
    found: Option<u8>,
) -> impl Fn(&'static str) -> Option<u8> {
    move |name| {
        if name == changed {
            found
        } else if name == GIT_GLOBAL_CONFIG {
            None
        } else {
            Some(PLACED)
        }
    }
}

#[kani::proof]
#[kani::unwind(18)]
fn one_changed_name_is_the_escape() {
    let at = kani::any::<usize>();
    kani::assume(at < CONFINED_HOME.len());
    let Some(&(name, under)) = CONFINED_HOME.get(at) else {
        return;
    };
    let found = symbolic_found();
    let said = escape(confined_except(name, found), |_| PLACED);
    let ruled = if found == Some(PLACED) {
        None
    } else {
        Some(Escape::Escaped { name, under, found })
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:escapes-by-the-one-name",
    );
    kani::cover!(said.is_none(), "njutest-law-branch:confined");
    kani::cover!(said.is_some(), "njutest-law-branch:escaped");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(18)]
fn a_given_git_identity_is_an_escape() {
    let found = symbolic_found();
    let said = escape(confined_except(GIT_GLOBAL_CONFIG, found), |_| PLACED);
    kani::assert(
        said == found.map(|found| Escape::GitGlobal { found }),
        "njutest-law-assertion:git-global-escapes",
    );
    kani::cover!(said.is_none(), "njutest-law-branch:confined");
    kani::cover!(said.is_some(), "njutest-law-branch:escaped");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(18)]
fn the_first_escape_is_the_one_named() {
    let (first, second) = (kani::any::<usize>(), kani::any::<usize>());
    kani::assume(first < second && second < CONFINED_HOME.len());
    let (Some(&(earlier, under)), Some(&(later, _))) =
        (CONFINED_HOME.get(first), CONFINED_HOME.get(second))
    else {
        return;
    };
    let said = escape(
        |name| {
            if name == earlier || name == later {
                Some(ELSEWHERE)
            } else if name == GIT_GLOBAL_CONFIG {
                None
            } else {
                Some(PLACED)
            }
        },
        |_| PLACED,
    );
    kani::assert(
        said == Some(Escape::Escaped {
            name: earlier,
            under,
            found: Some(ELSEWHERE),
        }),
        "njutest-law-assertion:first-escape-named",
    );
    kani::cover!(true, "njutest-law-reached");
}
