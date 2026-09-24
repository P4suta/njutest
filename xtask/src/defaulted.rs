// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The places an audit reader supplies a value where its input gave none, counted per file and held to a ceiling that only falls.

use std::collections::BTreeMap;

use syn::visit::Visit;

/// The files that read a run's recordings and reports to re-decide them, by path or by directory.
pub const AUDIT_READERS: [&str; 10] = [
    "xtask/src/proofaudit.rs",
    "xtask/src/proofaudit/",
    "xtask/src/engineaudit/",
    "xtask/src/route.rs",
    "xtask/src/wire.rs",
    "xtask/src/drift.rs",
    "xtask/src/crashes.rs",
    "xtask/src/faults.rs",
    "xtask/src/knobs.rs",
    "xtask/src/concurrency.rs",
];

/// The methods that answer for an absent value with one the input never gave.
pub const DEFAULTING: [&str; 5] = [
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "map_or",
    "map_or_else",
];

/// Whether `file`, repository-relative, is an audit reader.
#[must_use]
pub fn reads_for_an_audit(file: &str) -> bool {
    AUDIT_READERS.iter().any(|reader| {
        if reader.ends_with('/') {
            file.starts_with(reader)
        } else {
            file == *reader
        }
    })
}

/// How many calls of a [`DEFAULTING`] method `source` makes outside its tests.
///
/// # Errors
/// A source this version of `syn` cannot parse.
pub fn defaulted_in(source: &str) -> Result<usize, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut counted = Counted(0);
    counted.visit_file(&file);
    Ok(counted.0)
}

/// A running count of defaulting calls.
struct Counted(usize);

impl Visit<'_> for Counted {
    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if DEFAULTING.contains(&call.method.to_string().as_str()) {
            self.0 = self.0.saturating_add(1);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_item_mod(&mut self, module: &syn::ItemMod) {
        let tests = module.attrs.iter().any(|attribute| {
            attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|ident| ident == "test")
        });
        if !tests {
            syn::visit::visit_item_mod(self, module);
        }
    }
}

/// What `counted` comes to against the ceiling `written`, one `<count> <file>` per line with `#` comments: every file at exactly its ceiling, and a file not named held to none.
///
/// # Errors
/// Every file above its ceiling, every ceiling a file has fallen below and that is not lowered to match, and a ceiling line that does not read.
pub fn held(counted: &BTreeMap<String, usize>, written: &str) -> Result<usize, Vec<String>> {
    let mut ceilings: BTreeMap<String, usize> = BTreeMap::new();
    let mut refused = Vec::new();
    for line in written.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((number, file)) = line.split_once(' ') else {
            refused.push(format!("a ceiling line is `<count> <file>`: {line:?}"));
            continue;
        };
        match number.parse::<usize>() {
            Ok(ceiling) => {
                ceilings.insert(file.trim().to_owned(), ceiling);
            }
            Err(why) => refused.push(format!("{line:?}: {why}")),
        }
    }
    for (file, count) in counted {
        let ceiling = ceilings.get(file).copied().unwrap_or(0);
        if *count > ceiling {
            refused.push(format!(
                "{file} supplies {count} value(s) its input never gave, against a ceiling of \
                 {ceiling}: read the field the schema requires, or match on its absence and say \
                 what that means"
            ));
        } else if *count == 0 && ceiling > 0 {
            refused.push(format!(
                "{file} is down to none from a ceiling of {ceiling}: remove its line so the fall \
                 stays"
            ));
        } else if *count < ceiling {
            refused.push(format!(
                "{file} is down to {count} from a ceiling of {ceiling}: lower the ceiling to \
                 {count} so the fall stays"
            ));
        }
    }
    for (file, ceiling) in &ceilings {
        if !counted.contains_key(file) && *ceiling > 0 {
            refused.push(format!(
                "{file} holds a ceiling of {ceiling} and is not an audit reader any more: remove \
                 the line"
            ));
        }
    }
    if refused.is_empty() {
        Ok(counted.values().sum())
    } else {
        Err(refused)
    }
}
