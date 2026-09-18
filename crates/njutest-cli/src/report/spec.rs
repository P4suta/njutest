// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the seams a run watched say the system does, and who is holding each sentence up.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Report, SeamDecision, SeamRecord};

/// What one exchange the run observed does, and who would notice if it stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// The capability the seam serves.
    pub capability: String,
    /// Where it falls in the order on that seam.
    pub seq: u64,
    /// What was asked over it, or empty where the wire says nothing about how to read one.
    pub asked: String,
    /// What the upstream answered, where the wire says how to read one.
    pub answered: Option<u16>,
    /// The targets that noticed when it was perturbed, in the order they were named.
    pub guarded_by: Vec<String>,
    /// How many questions about it nothing noticed.
    pub unguarded: u32,
    /// How many questions about it the run could not put.
    pub unasked: u32,
}

impl Sentence {
    /// Whether anything at all would notice this exchange behaving differently.
    #[must_use]
    pub const fn is_guarded(&self) -> bool {
        !self.guarded_by.is_empty()
    }

    /// Whether the run put a question about this exchange that anybody could answer.
    ///
    /// A sentence nothing was asked about is neither held up nor a gap. The
    /// tests were never given the chance, so counting it among the ones
    /// nothing would notice changing would tell a reviewer the suite is blind
    /// where the run established nothing at all.
    #[must_use]
    pub const fn was_asked(&self) -> bool {
        self.unguarded > 0 || !self.guarded_by.is_empty()
    }

    /// What a person reads.
    #[must_use]
    pub fn worded(&self) -> String {
        let what = if self.asked.is_empty() {
            format!("exchange {} of {}", self.seq, self.capability)
        } else {
            format!("{} on {}", self.asked, self.capability)
        };
        let answers = self
            .answered
            .map_or_else(String::new, |status| format!(" answers {status}"));
        let held = if self.guarded_by.is_empty() {
            "nobody holds this up".to_owned()
        } else {
            format!("held up by {}", self.guarded_by.join(", "))
        };
        let short = match (self.unguarded, self.unasked) {
            (0, 0) => String::new(),
            (gaps, 0) => format!("; {gaps} question(s) nothing noticed"),
            (0, unasked) => format!("; {unasked} question(s) the run could not put"),
            (gaps, unasked) => {
                format!("; {gaps} question(s) nothing noticed and {unasked} the run could not put")
            }
        };
        format!("{what}{answers} — {held}{short}")
    }
}

/// One sentence per exchange the run watched, in the order the seams were recorded.
///
/// The sentences are made of what went past and nothing else. A specification
/// derived this way is always true of the system as it ran, which is what makes
/// the annotation worth reading: the question is never whether the behaviour is
/// real, only whether anybody would notice it changing.
#[must_use]
pub fn spoken(report: &Report) -> Vec<Sentence> {
    let mut by_exchange: BTreeMap<(&str, u64), Sentence> = BTreeMap::new();
    for one in &report.seams {
        let sentence = by_exchange
            .entry((one.capability.as_str(), one.seq))
            .or_insert_with(|| began(one));
        counted(sentence, one);
    }
    by_exchange.into_values().collect()
}

/// The page a person reads, or a line saying the run watched nothing.
#[must_use]
pub fn page(report: &Report) -> String {
    let sentences = spoken(report);
    if sentences.is_empty() {
        return "This run watched no seam, so there is nothing it observed the system \
                doing.\n"
            .to_owned();
    }
    let unguarded = sentences
        .iter()
        .filter(|one| one.was_asked() && !one.is_guarded())
        .count();
    let unasked = sentences.iter().filter(|one| !one.was_asked()).count();
    let mut out = format!(
        "What {} did, as the seams this run watched saw it.\n\n",
        report.run_id
    );
    for sentence in &sentences {
        let _written = writeln!(out, "  {}", sentence.worded());
    }
    let _written = write!(
        out,
        "\n{} observed, {unguarded} that nothing would notice changing",
        sentences.len()
    );
    if unasked > 0 {
        let _written = write!(out, ", {unasked} this run established nothing about");
    }
    let _written = writeln!(out, ".");
    out
}

/// A sentence about `one`'s exchange, with nothing counted yet.
fn began(one: &SeamRecord) -> Sentence {
    Sentence {
        capability: one.capability.clone(),
        seq: one.seq,
        asked: one.asked.clone(),
        answered: one.answered,
        guarded_by: Vec::new(),
        unguarded: 0,
        unasked: 0,
    }
}

/// Folds what became of one question into the sentence about its exchange.
///
/// A question a proof discharged holds nothing up and leaves nothing wanting:
/// nobody could have noticed it, so counting it against the tests would ask
/// them for something no test can give.
fn counted(sentence: &mut Sentence, one: &SeamRecord) {
    match &one.decision {
        SeamDecision::Tests { noticed_by } => {
            if !sentence.guarded_by.contains(noticed_by) {
                sentence.guarded_by.push(noticed_by.clone());
            }
        }
        SeamDecision::Unnoticed => sentence.unguarded = sentence.unguarded.saturating_add(1),
        SeamDecision::Unreached => sentence.unasked = sentence.unasked.saturating_add(1),
        SeamDecision::Proved { .. } => {}
    }
}
