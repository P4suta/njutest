// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rewriting a probeable site so that running the test records whether the mutation would have changed anything.
//!
//! The rewrite binds the returned expression once, asks the runtime whether
//! what it holds already equals what the replacement would write, and records
//! the mutant when it does not. Nothing about the value is otherwise touched:
//! the binding is returned unchanged, so a probed build runs the program it
//! would have run.
//!
//! Every rewrite adds no newline, so a probed file has the line numbers the
//! catalog reported.

use crate::probe::form::Question;
use crate::span::Span;

/// The name the rewrite binds the value to. It is one a program is unlikely to spell, and a program that does spell it shadows it inside its own scope rather than ours.
pub const BINDING: &str = "__rmp_value";

/// One site to probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// The mutant's dense catalog index, which is what the log records.
    pub index: u32,
    /// This site's position in the file's own table, which is what the runtime remembers.
    pub slot: usize,
    /// The bytes of the returned expression.
    pub span: Span,
    /// What the probe asks about it.
    pub question: Question,
    /// How many `super::` segments separate the site's inline module from the file root.
    pub super_depth: u32,
}

/// The text one site becomes, given the expression it covers verbatim.
///
/// The result holds no newline of its own, so the file keeps its line count
/// however many lines the expression spans.
#[must_use]
pub fn rewrite(site: &Site, expression: &str, module: &str) -> String {
    let path = path(module, site.super_depth);
    let import = import(module, site.super_depth);
    let record = format!("{path}::infect({}, {});", site.slot, site.index);
    let asked = match site.question {
        Question::Default => {
            format!("if !(&{BINDING}).probed() {{ {record} }}")
        }
        Question::True => format!("if !{BINDING} {{ {record} }}"),
        Question::OkDefault => format!(
            "match &{BINDING} {{ ::std::result::Result::Ok(inner) => if !inner.probed() {{ \
             {record} }}, ::std::result::Result::Err(_) => {{ {record} }} }}"
        ),
        Question::SomeDefault => format!(
            "match &{BINDING} {{ ::std::option::Option::Some(inner) => if !inner.probed() {{ \
             {record} }}, ::std::option::Option::None => {{ {record} }} }}"
        ),
    };
    format!("({{ {import} let {BINDING} = {expression}; {asked} {BINDING} }})")
}

/// The path the site calls the runtime module by.
#[must_use]
pub fn path(module: &str, super_depth: u32) -> String {
    let mut path = String::new();
    for _ in 0..super_depth {
        path.push_str("super::");
    }
    path.push_str(module);
    path
}

/// The import that brings the two traits into the site's scope, so that method resolution can pick between them.
///
/// A `use` path's first segment names a crate unless it is `self`, `super`, or
/// `crate`, and the runtime module sits at the root of this file rather than of
/// the crate, so the path is written relative either way.
#[must_use]
pub fn import(module: &str, super_depth: u32) -> String {
    let prefix = if super_depth == 0 {
        "self::".to_owned()
    } else {
        "super::".repeat(usize::try_from(super_depth).unwrap_or(0))
    };
    format!("use {prefix}{module}::{{FloatRefuse as _, Probe as _}};")
}
