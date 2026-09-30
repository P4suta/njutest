// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a test process wrote about the tests of it that could not measure where they ran, read as the engine believes it: only where the process's tests were read whole (ADR 0043).

/// Whether the tests a run was read as passing are the harness's answer rather than the parser's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Reading {
    /// libtest's named passing tests come to its own summary's count.
    Whole,
    /// libtest's named passing tests do not come to its summary's count, or it printed no summary: a line the suite wrote past the capture read as a result, or split one.
    Short,
    /// The protocol names no tests and prints no summary, so there is nothing to fall short of.
    Unspoken,
}

/// One test that declined to measure, and the words it gave.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decline {
    /// The test, by its libtest name.
    pub test: String,
    /// Why it could not measure, in its own words.
    pub why: String,
}

/// What the engine makes of one process's notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declines {
    /// The notice is believed.
    Read {
        /// Each test that declined, by name, in name order, every one a test the process passed.
        declined: Vec<Decline>,
        /// The words of each line that names no test in a process that ran several, which a report quotes and which set nothing aside.
        quoted: Vec<String>,
    },
    /// The notice cannot be believed, and neither can a pass of the process it came from.
    Unbelieved {
        /// Why.
        because: Unbelieved,
    },
}

/// Why a notice is not believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbelieved {
    /// The process's tests were not read whole, so no name in the notice can be held to a test that ran.
    ReadingNotWhole,
    /// A line is not a name, a tab and the words.
    Malformed {
        /// The line.
        line: String,
    },
    /// A line names a test the process did not pass.
    NotATest {
        /// The name it gave.
        test: String,
    },
    /// One test declined twice, in different words.
    Contradicted {
        /// The test.
        test: String,
    },
    /// The file holds bytes that are not text.
    NotText,
    /// The file is there and could not be read.
    Unreadable {
        /// What reading it said.
        message: String,
    },
}

impl Unbelieved {
    /// What the refusal says, in words a reader of the report can act on.
    #[must_use]
    pub fn said(&self) -> String {
        match self {
            Self::ReadingNotWhole => "a test declined in a process whose tests were not read \
                                      whole, so the decline names no test that is known to have run"
                .to_owned(),
            Self::Malformed { line } => format!(
                "the decline notice holds {line:?}, which is not a test's name, a tab, and why it \
                 declined; a line appended in more than one write can have another test's land \
                 inside it, so each line is appended in one"
            ),
            Self::NotATest { test } => format!(
                "the decline notice names {test:?}, which is not a test this process passed"
            ),
            Self::Contradicted { test } => {
                format!("the decline notice gives {test:?} two different reasons")
            }
            Self::NotText => "the decline notice is not text".to_owned(),
            Self::Unreadable { message } => {
                format!("the decline notice could not be read: {message}")
            }
        }
    }
}

impl Declines {
    /// A notice that says no test declined.
    #[must_use]
    pub const fn none() -> Self {
        Self::Read {
            declined: Vec::new(),
            quoted: Vec::new(),
        }
    }

    /// What reading the notice answered, `notice`, says of the process that could write it: that no test declined where the engine named no notice or the process wrote none, and that it cannot be believed where it was there and could not be read.
    #[must_use]
    pub fn answered(
        notice: Option<std::io::Result<Vec<u8>>>,
        reading: Reading,
        passed: &[String],
    ) -> Self {
        match notice {
            None => Self::none(),
            Some(Ok(bytes)) => Self::read(&bytes, reading, passed),
            Some(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Self::none(),
            Some(Err(error)) => Self::Unbelieved {
                because: Unbelieved::Unreadable {
                    message: error.to_string(),
                },
            },
        }
    }

    /// What `bytes`, one process's notice, say of the tests the process was read as passing, `reading` being how whole that account is.
    #[must_use]
    pub fn read(bytes: &[u8], reading: Reading, passed: &[String]) -> Self {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Self::Unbelieved {
                because: Unbelieved::NotText,
            };
        };
        let lines: Vec<&str> = text
            .split('\n')
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.is_empty())
            .collect();
        if lines.is_empty() {
            return Self::none();
        }
        match reading {
            Reading::Whole => attributed(&lines, passed),
            Reading::Short | Reading::Unspoken => Self::Unbelieved {
                because: Unbelieved::ReadingNotWhole,
            },
        }
    }

    /// Each test the notice was believed to decline, and none where it was not believed.
    #[must_use]
    pub fn believed(&self) -> &[Decline] {
        match self {
            Self::Read { declined, .. } => declined,
            Self::Unbelieved { .. } => &[],
        }
    }

    /// Whether the notice says nothing: no test declined and no line was quoted.
    #[must_use]
    pub const fn is_silent(&self) -> bool {
        match self {
            Self::Read { declined, quoted } => declined.is_empty() && quoted.is_empty(),
            Self::Unbelieved { .. } => false,
        }
    }
}

/// Every line of a whole reading, held to the tests the process passed.
fn attributed(lines: &[&str], passed: &[String]) -> Declines {
    let mut declined: Vec<Decline> = Vec::new();
    let mut quoted = Vec::new();
    for line in lines {
        let Some((test, why)) = line.split_once('\t') else {
            return Declines::Unbelieved {
                because: Unbelieved::Malformed {
                    line: (*line).to_owned(),
                },
            };
        };
        let test = match (test.is_empty(), passed) {
            (true, [only]) => only.as_str(),
            (true, _) => {
                quoted.push(why.to_owned());
                continue;
            }
            (false, _) if passed.iter().any(|one| one == test) => test,
            (false, _) => {
                return Declines::Unbelieved {
                    because: Unbelieved::NotATest {
                        test: test.to_owned(),
                    },
                };
            }
        };
        match declined.iter().find(|one| one.test == test) {
            Some(earlier) if earlier.why == why => {}
            Some(_) => {
                return Declines::Unbelieved {
                    because: Unbelieved::Contradicted {
                        test: test.to_owned(),
                    },
                };
            }
            None => declined.push(Decline {
                test: test.to_owned(),
                why: why.to_owned(),
            }),
        }
    }
    declined.sort();
    Declines::Read { declined, quoted }
}

#[cfg(test)]
mod tests;
