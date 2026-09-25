// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Findings as the workflow commands a GitHub Actions runner turns into annotations on a line.

use super::run::RunMutantDocument;

/// A survivor as one `::error` command, its file named from the checkout by `prefix`.
#[must_use]
pub fn surviving(prefix: &str, mutant: &RunMutantDocument) -> String {
    format!(
        "::error file={file},line={line},col={column},title={title}::{message}",
        file = property(&format!("{prefix}{}", mutant.path)),
        line = mutant.line,
        column = mutant.column,
        title = property(&format!("surviving mutant {}", mutant.rule)),
        message = data(&format!(
            "no test noticed {} \u{2192} {}; `rust-mutants explain {}` says what it is",
            shown(&mutant.original),
            shown(&mutant.replacement),
            mutant.display_id
        )),
    )
}

/// One side of the edit.
fn shown(text: &str) -> String {
    if text.is_empty() {
        "nothing".to_owned()
    } else {
        format!("`{text}`")
    }
}

/// The text as a command's message, which a line break would end.
fn data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// The text as a command's property, which a comma or a colon would also end.
fn property(text: &str) -> String {
    data(text).replace(':', "%3A").replace(',', "%2C")
}
