// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every code an xtask error carries, with what it means and what to do about it, which `docs/errors.md` lists and a test holds equal.

/// One stable code, the searchable name of one failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum XtCode {
    /// A gate refused the tree.
    GateRefused,
    /// A line of the seam ledger is not one the ratchet reads.
    SeamLedger,
    /// The roadmap's milestone table is malformed.
    MilestoneRegistry,
    /// The root manifest's lint policy could not be read for the fuzz workspace.
    FuzzPolicy,
    /// Cargo could not be started for the fuzz workspace.
    FuzzCargo,
    /// A published schema does not compile.
    SchemaUncompilable,
    /// A fixture tree could not be walked or read.
    FixtureUnreadable,
    /// A fixture tree holds a symbolic link.
    FixtureSymlink,
    /// A fixture path is not UTF-8.
    FixturePath,
    /// A directory under a fixture group is not a fixture.
    NotAFixture,
    /// A fixture's configuration is not TOML.
    FixtureConfig,
    /// A run directory holds no report.
    ProofUnreadable,
    /// A report is not JSON.
    ProofUnparsable,
    /// A recording has a line it cannot read.
    ProofRecording,
    /// A report is not one build measured whole.
    ProofUnprojected,
    /// A complete report departs from its published schema.
    ProofOffSchema,
    /// A report given as a merge is not one.
    ProofNotMerged,
    /// A document given as a shard is not one.
    ProofNotAShard,
    /// A shard was given twice.
    ProofShardTwice,
    /// A shard the merge does not name was given.
    ProofShardNotMerged,
    /// A document on its schema is not one this audit can read.
    ProofUnshaped,
    /// An engine run directory holds no report.
    EngineUnreadable,
    /// An engine report is not JSON.
    EngineUnparsable,
    /// An engine evidence document is malformed.
    EngineEvidence,
    /// An engine recording is malformed.
    EngineRecording,
    /// A ledger is malformed.
    EngineLedger,
    /// An engine document is not the run report this release re-decides.
    EngineUnrecognised,
    /// An engine run report departs from its published schema.
    EngineOffSchema,
    /// A Kani export could not be read.
    KaniUnreadable,
    /// A Kani export is not the pinned release for this workspace.
    KaniContract,
    /// A Kani export's harnesses or ledgers are substituted.
    KaniLedger,
    /// A Kani proof is not successful and exact.
    KaniUnproven,
    /// Kani's counts exceed their evidence type.
    KaniArithmetic,
    /// A report's model evidence is malformed.
    ModelReport,
    /// A retained model artifact cannot be audited.
    ModelArtifact,
    /// A retained Kani export does not establish the reported answer.
    ModelExport,
    /// A specimen could not be laid out.
    SpecimenUnwritable,
    /// A specimen's recording is malformed.
    SpecimenEvent,
    /// A specimen report could not be completed.
    SpecimenIncomplete,
    /// A planted text of the sentinels is malformed.
    SentinelPlanted,
    /// An identity field exceeds its length prefix.
    IdentityField,
    /// A recording line is not JSON.
    RecordingLine,
    /// A report of `report-diff` could not be read.
    DiffUnreadable,
    /// The bill of materials could not be made.
    SbomMetadata,
}

impl XtCode {
    /// `XT` and four digits.
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.entry().0
    }

    /// What went wrong.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn meaning(self) -> &'static str {
        self.entry().1
    }

    /// What to do about it.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn remedy(self) -> &'static str {
        self.entry().2
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per code: the table is the function, and splitting it would split the one match that keeps it total"
    )]
    const fn entry(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::GateRefused => (
                "XT0001",
                "A gate refused the tree, and its message names every place it refused.",
                "fix each place the message names, or the rule it names, and run the gate again",
            ),
            Self::SeamLedger => (
                "XT0002",
                "A line of the seam allowlist is not one the ratchet can read.",
                "write the line as the ledger's other lines are written",
            ),
            Self::MilestoneRegistry => (
                "XT0003",
                "The roadmap declares no milestone, or one twice.",
                "give every milestone one row in the roadmap table",
            ),
            Self::FuzzPolicy => (
                "XT0004",
                "The root manifest could not be read, or its workspace lint table is not one `fuzz-clippy` can carry to the fuzz workspace.",
                "fix the table the message names in the root `Cargo.toml`",
            ),
            Self::FuzzCargo => (
                "XT0005",
                "`cargo clippy` could not be started for the fuzz workspace.",
                "check `cargo` is on the path and the pinned toolchain is installed",
            ),
            Self::SchemaUncompilable => (
                "XT0006",
                "A published JSON schema under `schema/` does not compile, so nothing can be validated against it.",
                "fix the schema the message names; `cargo xtask all` compiles every one",
            ),
            Self::FixtureUnreadable => (
                "XT1001",
                "A fixture's tree could not be walked or one of its files read.",
                "check the path the message names exists and is readable",
            ),
            Self::FixtureSymlink => (
                "XT1002",
                "A fixture tree holds a symbolic link, which the checks never follow.",
                "replace the link with the file it points at",
            ),
            Self::FixturePath => (
                "XT1003",
                "A fixture path is not UTF-8, so no protocol a fixture feeds could spell it.",
                "rename the path",
            ),
            Self::NotAFixture => (
                "XT1004",
                "A directory under a fixture group is not a fixture.",
                "make it a fixture, with its manifest, lockfile and README, or move it out of the group",
            ),
            Self::FixtureConfig => (
                "XT1005",
                "A fixture's configuration is not TOML.",
                "fix the file the message names",
            ),
            Self::ProofUnreadable => (
                "XT2001",
                "The run directory holds no assurance report, or it could not be read.",
                "point `proofaudit` at the directory a completed run wrote",
            ),
            Self::ProofUnparsable => (
                "XT2002",
                "The assurance report is not JSON this audit can read.",
                "re-run the run that wrote it; a report nothing can parse is not one to re-decide",
            ),
            Self::ProofRecording => (
                "XT2003",
                "The runner's recording has a line that is not JSON.",
                "re-run with `--trace`; a recording that lost a line cannot be counted as agreement",
            ),
            Self::ProofUnprojected => (
                "XT2004",
                "The report is not one configured build measured whole, which is what this audit re-decides.",
                "audit each part against its own recording",
            ),
            Self::ProofOffSchema => (
                "XT2005",
                "The report departs from the published assurance-report schema, so a reader could meet an absent required field.",
                "re-run with this release; a report off its schema is not one to re-decide",
            ),
            Self::ProofNotMerged => (
                "XT2006",
                "A document given as a merged report is not a merge of shards.",
                "give `proofaudit` the report `njutest merge` wrote, with `--shard` for each part",
            ),
            Self::ProofNotAShard => (
                "XT2007",
                "A document given with `--shard` is not a shard of a catalog.",
                "give each shard's own report or run directory to `--shard`",
            ),
            Self::ProofShardTwice => (
                "XT2008",
                "The same shard was given twice with `--shard`.",
                "give each shard once; counting one part twice is an operator's mistake, not a merge",
            ),
            Self::ProofShardNotMerged => (
                "XT2009",
                "A shard was given that the merged report does not name among its sources.",
                "give only the shards the merged report names in its composition",
            ),
            Self::ProofUnshaped => (
                "XT2010",
                "The document is on its published schema and is not one this audit can read into a complete report or a shard.",
                "report it; a document on its schema that this audit cannot read is a gap in the audit",
            ),
            Self::EngineUnreadable => (
                "XT3001",
                "The run directory holds no engine run report, or it or a document beside it could not be read.",
                "point `engine-audit` at the directory a completed engine run wrote",
            ),
            Self::EngineUnparsable => (
                "XT3002",
                "The engine run report is not JSON this audit can read.",
                "re-run the run that wrote it",
            ),
            Self::EngineEvidence => (
                "XT3003",
                "An evidence document beside the engine run report is not one this audit can read.",
                "re-run the run that wrote it",
            ),
            Self::EngineRecording => (
                "XT3004",
                "The engine's recording is not one this audit can read, or is of another schema.",
                "re-run with `--trace` using this release",
            ),
            Self::EngineLedger => (
                "XT3005",
                "The configuration named as the ledger is not one this audit can read.",
                "fix the configuration file the message names",
            ),
            Self::EngineUnrecognised => (
                "XT3006",
                "The document is not the engine run report, or is of another schema version.",
                "point `engine-audit` at a run report this release wrote",
            ),
            Self::EngineOffSchema => (
                "XT3007",
                "The engine run report departs from the published run-report schema, so a reader could meet an absent required field or a value of another shape.",
                "re-run with this release; a report off its schema is not one to re-decide",
            ),
            Self::KaniUnreadable => (
                "XT4001",
                "The Kani export could not be read, or is not the closed JSON schema of the pinned release.",
                "regenerate the export with the pinned Kani",
            ),
            Self::KaniContract => (
                "XT4002",
                "The Kani export's metadata, project or toolchain is not the pinned release run on this workspace.",
                "regenerate the export here with the pinned Kani and backend",
            ),
            Self::KaniLedger => (
                "XT4003",
                "A harness or check ledger of the Kani export is missing, duplicated, or not the selected production one.",
                "regenerate the export from the production harness list",
            ),
            Self::KaniUnproven => (
                "XT4004",
                "A Kani proof's result, assertions, covers, properties, backend evidence or summary is not successful and exact.",
                "read the harness the message names; a proof that does not hold is a defect to fix, not a gate to relax",
            ),
            Self::KaniArithmetic => (
                "XT4005",
                "Kani's result arithmetic exceeded the type its evidence is held in.",
                "report it; a count that cannot be held is a count this audit refuses to guess",
            ),
            Self::ModelReport => (
                "XT4101",
                "The report's model evidence is not the closed verified-v1 shape, or contradicts itself.",
                "re-run the verified run that wrote it",
            ),
            Self::ModelArtifact => (
                "XT4102",
                "A retained model artifact is outside the run directory or cannot be read.",
                "audit the run directory the artifacts were retained in",
            ),
            Self::ModelExport => (
                "XT4103",
                "A retained Kani export is not the pinned schema, or does not establish the answer the report gives.",
                "read the model record the message names",
            ),
            Self::SpecimenUnwritable => (
                "XT5001",
                "An audit specimen could not be laid out in a temporary directory.",
                "check the temporary directory is writable",
            ),
            Self::SpecimenEvent => (
                "XT5002",
                "An event of an audit specimen's recording is not an object, or lacks its envelope.",
                "fix the specimen in the sentinel module the gate names",
            ),
            Self::SpecimenIncomplete => (
                "XT5003",
                "A flat audit specimen could not be completed into the document a run writes.",
                "fix the specimen in the sentinel module the gate names",
            ),
            Self::SentinelPlanted => (
                "XT5101",
                "A planted text of the lint sentinels is not the header-and-files shape they are read in.",
                "fix the planted text under `xtask/sentinels/` the message names",
            ),
            Self::IdentityField => (
                "XT6001",
                "An identity field exceeds the length prefix of the recipe it is minted by.",
                "report it; an identity this recipe cannot spell is not one to truncate",
            ),
            Self::RecordingLine => (
                "XT6002",
                "A line of a recording is not JSON.",
                "re-run with `--trace`",
            ),
            Self::DiffUnreadable => (
                "XT7001",
                "A report given to `report-diff` is not one this version understands.",
                "give it two reports this release wrote",
            ),
            Self::SbomMetadata => (
                "XT7002",
                "`cargo metadata` could not be read into a bill of materials.",
                "run `cargo metadata --locked` and fix what it says",
            ),
        }
    }
}

/// A failure that carries one stable code.
pub trait Coded: core::fmt::Display {
    /// The stable code this failure carries, which `docs/errors.md` explains.
    fn code(&self) -> XtCode;

    /// The failure as a person reads it: its code, then what it says.
    fn coded(&self) -> String {
        format!("{}: {self}", self.code().code())
    }
}
