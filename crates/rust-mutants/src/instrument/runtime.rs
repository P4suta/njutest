// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The module appended to every instrumented file, which decides at run time
//! which mutant is live.
//!
//! It lives at the end of the file rather than in a crate of its own, and
//! that placement is the whole design. Nothing above it moves, so line
//! numbers hold; no `mod` declaration, no manifest entry, and no crate
//! boundary is created, so a file reached through `#[path]`, compiled by
//! both a library and a binary, or belonging to any member of a workspace is
//! handled by the same rule. Each file carries the table of its own mutants
//! and nothing else.
//!
//! What it says on the way out goes to the standard error file descriptor
//! rather than through `eprintln!`: a test harness captures the print
//! macros, and a process that exits before the harness flushes would take
//! the message with it.
//!
//! Activation is one environment variable read once per process. The catalog
//! digest is checked at the same time: an identity this file does not know is
//! a mutant of another file and activates nothing here, but a *catalog* that
//! is not the one this tree was built from ends the process with
//! [`STALE_CATALOG_EXIT`] rather than reporting survivors for mutants that
//! were never live.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::Placement;

/// Selects the active mutant by its full identity.
pub const ACTIVE_ENV: &str = "RUST_MUTANTS_ACTIVE";

/// Names the catalog the activating run holds.
pub const CATALOG_ENV: &str = "RUST_MUTANTS_CATALOG";

/// The exit status of a test process whose tree was built from a different
/// catalog than the one activating it.
pub const STALE_CATALOG_EXIT: i32 = 97;

/// The name the generated module takes when the file does not already spell
/// it; otherwise a digit is appended until one is free.
pub const MODULE_STEM: &str = "__rm";

/// Marks the generated module, for a person reading the snapshot and for
/// the drift gate.
pub const RUNTIME_MARKER: &str = "rust-mutants-runtime-v1";

/// The lowest value the runtime reserves: `u32::MAX` means "no mutant of
/// this file is active" and `u32::MAX - 1` "the environment has not been
/// read yet", so a catalog index must stay below both.
pub(super) const LOWEST_SENTINEL: u32 = u32::MAX - 1;

/// The name the generated module can take in `text`: [`MODULE_STEM`] with
/// `path`'s digest appended, or that with the lowest free number after it.
///
/// Two things are being dodged. The file's own identifiers, because a name the
/// file spells anywhere — a module of its own, a variable, a macro — would be
/// shadowed by or would shadow the generated one, and the guards would then
/// call something else entirely. And the other files' modules: `include!` at
/// item position pastes one file's items into another's module, so two files
/// that both called their module `__rm` would define it twice in one scope,
/// and every mutant of both files would come back refused by a compiler error
/// that names none of them. The path is what tells them apart, so the name
/// carries its digest.
#[must_use]
pub fn module_name(path: &str, text: &str) -> String {
    module_named(text, &format!("{MODULE_STEM}_{}", short_digest(path)))
}

/// The first eight hex characters of the path's SHA-256, which is what makes one file's runtime module a different item from another's.
fn short_digest(path: &str) -> String {
    use sha2::Digest;
    let full = hex::encode(sha2::Sha256::digest(path.as_bytes()));
    full.get(..8).unwrap_or(&full).to_owned()
}

/// [`module_name`] for a module of another stem, so the witness tree can have one of its own without either shadowing the other.
#[must_use]
pub(super) fn module_named(text: &str, stem: &str) -> String {
    let taken: BTreeSet<String> = text
        .parse::<proc_macro2::TokenStream>()
        .map(|tokens| {
            let mut names = BTreeSet::new();
            collect_identifiers(tokens, &mut names);
            names
        })
        .unwrap_or_default();
    if !taken.contains(stem) {
        return stem.to_owned();
    }
    // Bounded: a file cannot spell more names than it has identifiers, and
    // the table is what every candidate came from.
    let limit = u32::try_from(taken.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    (1u32..=limit)
        .map(|suffix| format!("{stem}{suffix}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_else(|| format!("{stem}_generated"))
}

fn collect_identifiers(tokens: proc_macro2::TokenStream, names: &mut BTreeSet<String>) {
    for tree in tokens {
        match tree {
            proc_macro2::TokenTree::Ident(ident) => {
                names.insert(ident.to_string());
            }
            proc_macro2::TokenTree::Group(group) => collect_identifiers(group.stream(), names),
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
}

/// The invariant text of the runtime, with the per-file parts as
/// placeholders. Written as one literal so a reader sees the generated
/// module exactly as it will appear in the snapshot.
const TEMPLATE: &str = r#"#[doc(hidden)]
#[allow(dead_code, let_underscore_drop, missing_docs, non_snake_case, non_upper_case_globals, unfulfilled_lint_expectations, unreachable_pub, unused, warnings, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)]
mod {{MODULE}} {
    // {{MARKER}} - generated by rust-mutants; DO NOT EDIT.
    extern crate std as __rm_std;
    const CATALOG: &str = "{{CATALOG}}";
    const IDS: &[(&str, u32)] = &[
{{IDS}}    ];
    const UNINIT: u32 = u32::MAX - 1;
    const NONE: u32 = u32::MAX;
    static ACTIVE: __rm_std::sync::atomic::AtomicU32 = __rm_std::sync::atomic::AtomicU32::new(UNINIT);

    #[inline(always)]
    pub(crate) fn active(index: u32) -> bool {
        let selected = ACTIVE.load(__rm_std::sync::atomic::Ordering::Relaxed);
        if selected != UNINIT {
            return selected == index;
        }
        let resolved = resolve();
        ACTIVE.store(resolved, __rm_std::sync::atomic::Ordering::Relaxed);
        resolved == index
    }

    #[cold]
    fn resolve() -> u32 {
        let wanted = match __rm_std::env::var("{{ACTIVE_ENV}}") {
            __rm_std::result::Result::Ok(value) => value,
            __rm_std::result::Result::Err(_) => return NONE,
        };
        if wanted.is_empty() {
            return NONE;
        }
        let catalog = __rm_std::env::var("{{CATALOG_ENV}}").unwrap_or_default();
        if catalog != CATALOG {
            let said = __rm_std::format!("rust-mutants: this binary was built from catalog {} but {} is active\n", CATALOG, catalog);
            let _ = __rm_std::io::Write::write_all(&mut __rm_std::io::stderr(), said.as_bytes());
            __rm_std::process::exit({{EXIT}});
        }
        let mut at = 0;
        while at < IDS.len() {
            if IDS[at].0 == wanted {
                return IDS[at].1;
            }
            at += 1;
        }
        NONE
    }
}
"#;

/// Renders the runtime module for one file.
///
/// Every path goes through the module's own `extern crate std as __rm_std`, so
/// a crate which renamed or shadowed a prelude name still compiles, and so does
/// a `#![no_std]` crate: `#![no_std]` withholds the implicit link and the
/// prelude, and forbids neither an explicit link nor an explicit path. Nothing
/// in the module is `unsafe`, so a crate that forbids unsafe code still does.
pub(super) fn render(
    module: &str,
    catalog_digest: &str,
    placements: &[Placement],
    newline: &str,
) -> String {
    let mut ids: Vec<(&str, u32)> = placements
        .iter()
        .map(|placement| (placement.id.as_str(), placement.index))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut table = String::new();
    for (id, index) in &ids {
        let written = writeln!(table, "        ({id:?}, {index}),");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }

    let text = TEMPLATE
        .replace("{{MODULE}}", module)
        .replace("{{MARKER}}", RUNTIME_MARKER)
        .replace("{{CATALOG}}", catalog_digest)
        .replace("{{IDS}}", &table)
        .replace("{{ACTIVE_ENV}}", ACTIVE_ENV)
        .replace("{{CATALOG_ENV}}", CATALOG_ENV)
        .replace("{{EXIT}}", &STALE_CATALOG_EXIT.to_string());
    if newline == "\n" {
        text
    } else {
        text.replace('\n', newline)
    }
}
