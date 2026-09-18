import pathlib

p = pathlib.Path("crates/njutest-cli/src/report/mod.rs")
s = p.read_text()

old = """    /// What the question asks of the exchange.
    pub rule: crate::wire::rule::Rule,
    /// Who decided it, which is one of the four a seam question can be decided by.
    pub decision: SeamDecision,
    /// The target that noticed, or the proof that discharged it. `None` where neither did.
    pub noticed_by: Option<String>,
}"""
new = """    /// What the question asks of the exchange.
    pub rule: crate::wire::rule::Rule,
    /// Who decided it, and — where somebody did — who that was.
    #[serde(flatten)]
    pub decision: SeamDecision,
}"""
assert old in s
s = s.replace(old, new, 1)

s = s.replace(
    """/// One question a seam's recording licensed, and what the run made of it.
///
/// A `wire-unnoticed` finding names a question by its identity, and a reader
/// who cannot look that identity up has been handed a name and no way to know
/// what it stands for. This is what they look it up in: ADR 0002 keeps a finding
/// off the recording, so what the finding rests on has to be in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamRecord {""",
    """/// One question a seam's recording licensed, and what the run made of it.
///
/// A `wire-unnoticed` finding names a question by its identity, and a reader
/// who cannot look that identity up has been handed a name and no way to know
/// what it stands for. This is what they look it up in: ADR 0002 keeps a finding
/// off the recording, so what the finding rests on has to be in the report.
///
/// Not `deny_unknown_fields`: serde cannot refuse an unknown field and flatten
/// one in the same breath, and who decided a question has to be a field of the
/// record rather than a table under it. The published schema is what refuses a
/// document with something extra in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeamRecord {""",
    1,
)

old = """#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SeamDecision {
    /// A test noticed.
    Tests,
    /// No observer could have noticed, proved rather than run.
    Proved,
    /// The suite ran with it in place and nothing noticed.
    Unnoticed,
    /// The run could not put the question, so it established nothing.
    Unreached,
}

impl SeamDecision {
    /// Every way a seam question can be decided.
    pub const ALL: [Self; 4] = [
        Self::Tests,
        Self::Proved,
        Self::Unnoticed,
        Self::Unreached,
    ];

    /// The decision this is one of.
    #[must_use]
    pub const fn decision(self) -> Decision {
        match self {
            Self::Tests => Decision::Tests,
            Self::Proved => Decision::Proved,
            Self::Unnoticed => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.decision().name()
    }

    /// How much a question decided this way stands on, where less is a weaker run.
    #[must_use]
    pub const fn standing(self) -> u8 {
        self.decision().standing()
    }
}"""
new = """#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SeamDecision {
    /// A test noticed, and this is the one that did.
    Tests {
        /// The target that noticed.
        noticed_by: String,
    },
    /// No observer could have noticed, and this is the proof that says so.
    Proved {
        /// The proof's name.
        proof: String,
    },
    /// The suite ran with it in place and nothing noticed.
    Unnoticed,
    /// The run could not put the question, so it established nothing.
    Unreached,
}

impl SeamDecision {
    /// The decision this is one of.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        match self {
            Self::Tests { .. } => Decision::Tests,
            Self::Proved { .. } => Decision::Proved,
            Self::Unnoticed => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.decision().name()
    }

    /// How much a question decided this way stands on, where less is a weaker run.
    #[must_use]
    pub const fn standing(&self) -> u8 {
        self.decision().standing()
    }

    /// The target that noticed, or the proof that discharged it, where either did.
    #[must_use]
    pub fn by(&self) -> Option<&str> {
        match self {
            Self::Tests { noticed_by } => Some(noticed_by),
            Self::Proved { proof } => Some(proof),
            Self::Unnoticed | Self::Unreached => None,
        }
    }
}"""
assert old in s
s = s.replace(old, new, 1)
p.write_text(s)
print("ok")
