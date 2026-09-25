// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a claim holds: a `cfg` predicate over the names a target alone decides, judged against what the toolchain prints for that target (ADR 0042).

use std::collections::BTreeSet;

/// The `cfg` names a target alone decides, which a probe of the target reports whatever flags or profile a build adds.
pub const DECIDED: [&str; 13] = [
    "panic",
    "target_abi",
    "target_arch",
    "target_endian",
    "target_env",
    "target_family",
    "target_has_atomic",
    "target_has_atomic_primitive_alignment",
    "target_os",
    "target_pointer_width",
    "target_vendor",
    "unix",
    "windows",
];

/// The `cfg` names and values a target decides, as the run's own toolchain printed them for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    set: BTreeSet<(String, Option<String>)>,
}

impl Facts {
    /// What `rustc --print cfg --target <target>` printed, keeping only the names a target alone decides.
    #[must_use]
    pub fn printed(text: &str) -> Self {
        Self {
            set: text
                .lines()
                .filter_map(fact)
                .filter(|(name, _)| DECIDED.contains(&name.as_str()))
                .collect(),
        }
    }

    /// The facts as a report records them: `name` or `name="value"`, sorted.
    #[must_use]
    pub fn recorded(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .set
            .iter()
            .map(|(name, value)| match value {
                Some(value) => format!("{name}=\"{value}\""),
                None => name.clone(),
            })
            .collect();
        lines.sort();
        lines
    }

    /// The facts a report recorded, or nothing where a line is not one a report writes.
    #[must_use]
    pub fn recorded_back(lines: &[String]) -> Option<Self> {
        let set = lines
            .iter()
            .map(|line| fact(line).filter(|(name, _)| DECIDED.contains(&name.as_str())))
            .collect::<Option<BTreeSet<_>>>()?;
        Some(Self { set })
    }

    fn has(&self, name: &str, value: Option<&str>) -> bool {
        self.set
            .iter()
            .any(|(held, said)| held == name && said.as_deref() == value)
    }
}

/// One line of `--print cfg`: a name, or a name and a quoted value.
fn fact(line: &str) -> Option<(String, Option<String>)> {
    let line = line.trim();
    match line.split_once('=') {
        None => identifier(line).then(|| (line.to_owned(), None)),
        Some((name, value)) => {
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            (identifier(name) && !value.contains('"'))
                .then(|| (name.to_owned(), Some(value.to_owned())))
        }
    }
}

fn identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|next| next.is_ascii_alphanumeric() || next == '_')
}

/// A `cfg` predicate: `all`, `any`, `not`, a name, or a name and a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// Every one holds.
    All(Vec<Self>),
    /// At least one holds.
    Any(Vec<Self>),
    /// It does not hold.
    Not(Box<Self>),
    /// The target sets the name.
    Name(String),
    /// The target sets the name to the value.
    Pair(String, String),
}

impl Predicate {
    /// The predicate `text` writes.
    ///
    /// # Errors
    /// [`WhereError::Unparsable`] where `text` is not a predicate, and [`WhereError::Undecided`] for a name a target alone does not decide.
    pub fn parse(text: &str) -> Result<Self, WhereError> {
        let mut reading = Reading { text, rest: text };
        let predicate = reading.predicate()?;
        reading.blank();
        if !reading.rest.is_empty() {
            return Err(reading.refused("the end of the predicate"));
        }
        Ok(predicate)
    }

    /// Whether it holds of `facts`.
    #[must_use]
    pub fn holds(&self, facts: &Facts) -> bool {
        match self {
            Self::All(every) => every.iter().all(|one| one.holds(facts)),
            Self::Any(some) => some.iter().any(|one| one.holds(facts)),
            Self::Not(inner) => !inner.holds(facts),
            Self::Name(name) => facts.has(name, None),
            Self::Pair(name, value) => facts.has(name, Some(value)),
        }
    }
}

/// A predicate being read: the whole text, and what is left of it.
struct Reading<'a> {
    text: &'a str,
    rest: &'a str,
}

impl<'a> Reading<'a> {
    fn blank(&mut self) {
        self.rest = self.rest.trim_start();
    }

    fn refused(&self, what: &str) -> WhereError {
        WhereError::Unparsable {
            at: self.text.len().saturating_sub(self.rest.len()),
            what: format!("expected {what}"),
        }
    }

    fn eat(&mut self, token: char) -> bool {
        self.blank();
        match self.rest.strip_prefix(token) {
            Some(after) => {
                self.rest = after;
                true
            }
            None => false,
        }
    }

    fn name(&mut self) -> Result<String, WhereError> {
        self.blank();
        let rest: &'a str = self.rest;
        let (name, after) = rest.split_at(
            rest.find(|next: char| !(next.is_ascii_alphanumeric() || next == '_'))
                .unwrap_or(rest.len()),
        );
        if !identifier(name) {
            return Err(self.refused("a name"));
        }
        self.rest = after;
        Ok(name.to_owned())
    }

    fn value(&mut self) -> Result<String, WhereError> {
        if !self.eat('"') {
            return Err(self.refused("a quoted value"));
        }
        let Some((value, after)) = self.rest.split_once('"') else {
            return Err(self.refused("the quote that closes the value"));
        };
        self.rest = after;
        Ok(value.to_owned())
    }

    fn predicate(&mut self) -> Result<Predicate, WhereError> {
        let name = self.name()?;
        match name.as_str() {
            "all" | "any" => {
                let every = self.list()?;
                Ok(if name == "all" {
                    Predicate::All(every)
                } else {
                    Predicate::Any(every)
                })
            }
            "not" => {
                if !self.eat('(') {
                    return Err(self.refused("`(`"));
                }
                let inner = self.predicate()?;
                if !self.eat(')') {
                    return Err(self.refused("`)`, since not takes one predicate"));
                }
                Ok(Predicate::Not(Box::new(inner)))
            }
            _ if !DECIDED.contains(&name.as_str()) => Err(WhereError::Undecided { name }),
            _ if self.eat('=') => Ok(Predicate::Pair(name, self.value()?)),
            _ => Ok(Predicate::Name(name)),
        }
    }

    fn list(&mut self) -> Result<Vec<Predicate>, WhereError> {
        if !self.eat('(') {
            return Err(self.refused("`(`"));
        }
        let mut every = Vec::new();
        loop {
            if self.eat(')') {
                return Ok(every);
            }
            every.push(self.predicate()?);
            if !self.eat(',') {
                if self.eat(')') {
                    return Ok(every);
                }
                return Err(self.refused("`,` or `)`"));
            }
        }
    }
}

/// Why a `where` cannot be judged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WhereError {
    /// The text is not a `cfg` predicate.
    #[error("not a cfg predicate at byte {at}: {what}")]
    Unparsable {
        /// Where it stops being one.
        at: usize,
        /// What was expected there.
        what: String,
    },
    /// The predicate names a fact a probe of the target cannot report.
    #[error(
        "`{name}` is not decided by the target alone, so no probe of it can say whether it holds"
    )]
    Undecided {
        /// The name.
        name: String,
    },
}
