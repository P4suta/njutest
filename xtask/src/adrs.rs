// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The decision records, held to one number each, to the heading that carries it, to the one line the book lists each under, and to every link that names one.

use std::collections::BTreeMap;

/// One decision record, as its file names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Its number.
    pub number: u16,
    /// Its file name under `docs/adr/`.
    pub file: String,
}

/// Why the decision records do not hold together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecordError {
    /// A file under `docs/adr/` is not named by a four-digit number and a slug.
    #[error("docs/adr/{file}: a decision record is named NNNN-slug.md, by its number and a slug")]
    Unnamed {
        /// The file.
        file: String,
    },
    /// Two records share a number.
    #[error(
        "docs/adr/{first} and docs/adr/{second} are both decision {number:04}, and a number names \
         one decision"
    )]
    Duplicate {
        /// The number.
        number: u16,
        /// One of them.
        first: String,
        /// The other.
        second: String,
    },
    /// A record's heading does not carry its own number.
    #[error(
        "docs/adr/{file}: its heading is {heading:?}, where it is `# {number:04} — ` and its title"
    )]
    Heading {
        /// The file.
        file: String,
        /// The number its name gives it.
        number: u16,
        /// The heading it has, or nothing where it has none.
        heading: String,
    },
    /// The book lists a record under a number that is not its own, lists it twice, or leaves it out.
    #[error("docs/SUMMARY.md: {detail}")]
    Summary {
        /// What is wrong with the list.
        detail: String,
    },
    /// A link names a record that is not there.
    #[error("{page}: names {link}, and no decision record has that name")]
    Dangling {
        /// The page.
        page: String,
        /// What it names.
        link: String,
    },
}

/// The number a decision record's file name gives it, or nothing where the name is not `NNNN-slug.md`.
#[must_use]
pub fn numbered(file: &str) -> Option<u16> {
    let (digits, rest) = file.split_at_checked(4)?;
    let slug = rest.strip_prefix('-')?.strip_suffix(".md")?;
    let named = digits.bytes().all(|byte| byte.is_ascii_digit())
        && !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    named.then(|| digits.parse().ok()).flatten()
}

/// Every record of `files`, each given as its file name under `docs/adr/` and its text.
///
/// # Errors
/// [`RecordError::Unnamed`] for a file not named by a number and a slug, [`RecordError::Duplicate`] for two files of one number, and [`RecordError::Heading`] for a heading that does not carry the file's number.
pub fn records(files: &[(String, String)]) -> Result<Vec<Record>, RecordError> {
    let mut by_number: BTreeMap<u16, Record> = BTreeMap::new();
    for (file, text) in files {
        let number = numbered(file).ok_or_else(|| RecordError::Unnamed { file: file.clone() })?;
        let heading = text
            .lines()
            .find(|line| line.starts_with("# "))
            .unwrap_or_default();
        if !heading.starts_with(&format!("# {number:04} — ")) {
            return Err(RecordError::Heading {
                file: file.clone(),
                number,
                heading: heading.to_owned(),
            });
        }
        let record = Record {
            number,
            file: file.clone(),
        };
        if let Some(first) = by_number.insert(number, record) {
            return Err(RecordError::Duplicate {
                number,
                first: first.file,
                second: file.clone(),
            });
        }
    }
    Ok(by_number.into_values().collect())
}

/// The book's list of decision records, held to `records`: each listed once, under its own number, at its own path.
///
/// # Errors
/// [`RecordError::Summary`] naming the first record listed wrongly, twice, or not at all.
pub fn summary(text: &str, records: &[Record]) -> Result<(), RecordError> {
    let mut listed: BTreeMap<String, u16> = BTreeMap::new();
    for line in text.lines() {
        let Some((label, path)) = line
            .trim_start()
            .strip_prefix("- [")
            .and_then(|rest| rest.split_once("]("))
        else {
            continue;
        };
        let Some(file) = path
            .strip_suffix(')')
            .and_then(|path| path.strip_prefix("adr/"))
        else {
            continue;
        };
        let said = label
            .split_whitespace()
            .next()
            .and_then(|number| number.parse::<u16>().ok());
        let Some(record) = records.iter().find(|record| record.file == file) else {
            return Err(RecordError::Summary {
                detail: format!("lists adr/{file}, and no decision record has that name"),
            });
        };
        if said != Some(record.number) || !label.starts_with(&format!("{:04} ", record.number)) {
            return Err(RecordError::Summary {
                detail: format!(
                    "lists adr/{file} as {label:?}, where it is decision {:04}",
                    record.number
                ),
            });
        }
        if listed.insert(file.to_owned(), record.number).is_some() {
            return Err(RecordError::Summary {
                detail: format!("lists adr/{file} twice"),
            });
        }
    }
    match records
        .iter()
        .find(|record| !listed.contains_key(&record.file))
    {
        Some(record) => Err(RecordError::Summary {
            detail: format!("does not list adr/{}", record.file),
        }),
        None => Ok(()),
    }
}

/// Every name of a decision record in `text` that no record has, where `text` is the page `page`; a name is a path whose last part is `NNNN-slug.md`, under `adr/` or on a page in `docs/adr/`.
#[must_use]
pub fn dangling(page: &str, text: &str, records: &[Record]) -> Vec<RecordError> {
    let beside = page.starts_with("docs/adr/");
    text.split(|character: char| character.is_whitespace() || "()[]<>`\"',;".contains(character))
        .map(|token| {
            token
                .split_once('#')
                .map_or(token, |(path, _)| path)
                .trim_end_matches(['.', ':'])
        })
        .filter(|token| {
            let file = token.rsplit('/').next().unwrap_or(token);
            numbered(file).is_some() && (beside || token.contains("adr/"))
        })
        .filter(|token| {
            let file = token.rsplit('/').next().unwrap_or(token);
            !records.iter().any(|record| record.file == file)
        })
        .map(|token| RecordError::Dangling {
            page: page.to_owned(),
            link: token.to_owned(),
        })
        .collect()
}
