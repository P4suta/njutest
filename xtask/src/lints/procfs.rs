// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one crate that reads `/proc`, where a process reaped at any point of the read is gone rather than a failure.

use syn::visit::Visit;

use super::{Finding, Kind};

/// Why a read of `/proc` outside the door is refused, in the words a person needs to fix it.
pub(super) const REMEDY: &str = "ask `njutest_process::procfs`, whose every answer is \
    `Asked::Answered` or `Asked::Gone`. A query about a process by its id races that process's \
    end, and `/proc` answers the race as `ENOENT` before the open and as `ESRCH` on the read, \
    so a reader that knows only the first turns a process reaped mid-read into a failure or an \
    unreadable one: the engine's census of escaped producers, the temporary-tree owner's lock \
    and the lanes each did. `/proc` itself, a path beneath it however its separators and dot \
    segments fall, and one built from the listing are refused alike; only the kernel settings \
    under `/proc/sys` name no process";

/// The crate that reads `/proc` for everybody else, through `njutest_process::procfs`.
pub(super) const DOOR: &str = "crates/njutest-process/";

/// Whether `file` is held to the rule, which is every file the gate reads outside the door.
pub(super) fn held(file: &str) -> bool {
    !file.starts_with(DOOR)
}

/// Every literal of `parsed` naming the process table, each at its line, in code and among the tokens a macro is handed.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut scan = Named {
        file,
        found: Vec::new(),
    };
    scan.visit_file(parsed);
    scan.found
}

struct Named<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

/// Whether `path` resolves to `/proc` or a path beneath it, however its separators and dot segments fall, other than the kernel settings under `/proc/sys`, which name no process.
fn names_the_table(path: &[u8]) -> bool {
    let Some(rooted) = path.strip_prefix(b"/") else {
        return false;
    };
    let mut resolved: Vec<&[u8]> = Vec::new();
    for segment in rooted.split(|byte| *byte == b'/') {
        match segment {
            b"" | b"." => {}
            b".." => {
                resolved.pop();
            }
            named => resolved.push(named),
        }
    }
    match resolved.as_slice() {
        [b"proc", b"sys", ..] => false,
        [b"proc", ..] => true,
        _ => false,
    }
}

/// The text `literal` spells, where it is a string of any kind.
fn spelled(literal: &syn::Lit) -> Option<Vec<u8>> {
    match literal {
        syn::Lit::Str(text) => Some(text.value().into_bytes()),
        syn::Lit::ByteStr(bytes) => Some(bytes.value()),
        syn::Lit::CStr(text) => Some(text.value().into_bytes()),
        _ => None,
    }
}

impl Named<'_> {
    fn note(&mut self, literal: &syn::Lit) {
        if spelled(literal).is_some_and(|text| names_the_table(&text)) {
            self.found.push(Finding {
                kind: Kind::RawProcfs,
                file: self.file.to_owned(),
                line: literal.span().start().line,
            });
        }
    }

    fn scan_tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        for tree in tokens.clone() {
            match tree {
                proc_macro2::TokenTree::Literal(literal) => self.note(&syn::Lit::new(literal)),
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(&group.stream()),
                proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Named<'_> {
    fn visit_lit(&mut self, literal: &'ast syn::Lit) {
        self.note(literal);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.scan_tokens(&invocation.tokens);
        syn::visit::visit_macro(self, invocation);
    }
}
