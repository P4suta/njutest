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
    /// A decision record is misnamed, shares a number, carries another heading, is listed wrongly, or is named by a dangling link.
    AdrRecord,
    /// The documented workflows could not be checked.
    DocflowsUnchecked,
    /// actionlint refused a documented workflow.
    DocflowsRefused,
    /// The registry of critical decisions names what the tree does not define, leaves a hole nobody owns, or its ledger of holes grew.
    InvariantRegistry,
    /// The pre-push gate cannot check what the push names.
    PushUnverifiable,
    /// The pre-push tree moved or was changed during its check.
    PushTreeMoved,
    /// The pre-push check failed.
    PushCheckFailed,
    /// The pre-push check was stopped.
    PushCheckStopped,
    /// The pre-push gate could not run its check.
    PushGateUnrun,
    /// A run's lane could not be held.
    LaneUnavailable,
    /// A run was stopped while it waited for its lane.
    LaneInterrupted,
    /// A gate's program could not be run.
    WorkUnrun,
    /// The other machines could not be asked.
    RemoteUnrun,
    /// Another machine refused the commit.
    RemoteRefused,
    /// git could not list the repository.
    RepositoryUnlisted,
    /// git listed a repository path that is not UTF-8.
    RepositoryPath,
    /// The repository holds a symbolic link.
    RepositorySymlink,
    /// A path git listed could not be read.
    RepositoryUnreadable,
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
    /// A report on its schema lacks a field a layer reads.
    ProofUnreadReport,
    /// A thread standing contradicts the recording.
    ThreadsContradicted,
    /// An exploration contradicts its recorded controls.
    ExplorationContradicted,
    /// Recorded controls are no schedule an exploration runs.
    ScheduleUnreplayable,
    /// A thread standing has no record to rest on.
    ThreadsUnwitnessed,
    /// A fault decision contradicts its executions.
    FaultContradicted,
    /// A repair contradicts its recorded execution.
    RepairContradicted,
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
    /// The carry page lacks a block the carry audit reads.
    CarryPage,
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
    /// A recording line departs from its producer's schema.
    RecordingOffSchema,
    /// A recording line on its schema lacks a field a reader reads.
    RecordingUnread,
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
            Self::AdrRecord => (
                "XT0007",
                "A decision record under the ADR directory is misnamed, shares its number, carries another's heading, is listed wrongly in the book, or is named by a link to no record.",
                "fix the record, the book, or the link the message names",
            ),
            Self::DocflowsUnchecked => (
                "XT0008",
                "`docflows` could not check the workflows the documentation shows: a page could not be read, or actionlint could not be run or said something other than which workflows it refused.",
                "check `actionlint` is installed, or name it with `--actionlint`, and read what it said",
            ),
            Self::DocflowsRefused => (
                "XT0009",
                "actionlint refused a workflow the documentation shows.",
                "fix the snippet the message names, so a reader who copies it has a workflow that runs",
            ),
            Self::InvariantRegistry => (
                "XT0010",
                "The registry of critical decisions names an item the tree does not define, leaves a layer open that the gaps ledger does not give an owner, lists a hole it does not have, or the ledger grew past its ceiling.",
                "fix the cell, the ledger line, or the item the message names; a new critical decision arrives with what holds it",
            ),
            Self::PushUnverifiable => (
                "XT0101",
                "The push names something the pre-push gate cannot check: an update Git did not give whole, an object other than the checked-out commit, a remote commit that is not here, a move that is not a fast-forward, or only deletions.",
                "fetch the remote ref and push the checked-out commit as a fast-forward of it",
            ),
            Self::PushTreeMoved => (
                "XT0102",
                "The tree the pre-push gate checks stopped being the pushed commit while it ran, or the check changed it.",
                "leave the worktree alone while a push runs, then push again",
            ),
            Self::PushCheckFailed => (
                "XT0103",
                "The check the pre-push gate runs failed.",
                "read the check's own output above, fix what it names, and push again",
            ),
            Self::PushCheckStopped => (
                "XT0104",
                "The check the pre-push gate runs was stopped: it outlived its budget, said nothing for longer than the gate allows, or the gate was asked to stop.",
                "push again when the machine is less loaded, or raise the budget the message names",
            ),
            Self::PushGateUnrun => (
                "XT0105",
                "The pre-push gate could not run its check: a program, Git, one of its own files, a setting, its lane, or its progress output failed it.",
                "fix what the message names and push again",
            ),
            Self::LaneUnavailable => (
                "XT0201",
                "The lane a whole-workspace run waits in could not be found, written, locked, or reported on.",
                "set `NJUTEST_SLOT_DIR` to a writable directory, or fix the one the message names",
            ),
            Self::LaneInterrupted => (
                "XT0202",
                "The run was asked to stop while it waited for its lane.",
                "nothing is wrong with the tree; run it again",
            ),
            Self::WorkUnrun => (
                "XT0301",
                "A program a gate runs could not be started or watched, or the signals that stop it could not be armed.",
                "check the program the message names is installed and that this process may be signalled",
            ),
            Self::RemoteUnrun => (
                "XT0401",
                "`remote-check` could not ask the other machines: its machines file could not be read or names none, or a program, Git, a log, or the thread asking a machine failed it.",
                "fix the machines file or what the message names, and run it again",
            ),
            Self::RemoteRefused => (
                "XT0402",
                "At least one other machine refused the commit.",
                "read each machine's answer and fix what it names",
            ),
            Self::RepositoryUnlisted => (
                "XT0501",
                "git could not list what the repository holds, so no gate can say what it read.",
                "run the gate inside the repository's checkout, with git on the path",
            ),
            Self::RepositoryPath => (
                "XT0502",
                "git listed a path of the repository that is not UTF-8, which no path this repository holds is.",
                "rename the path",
            ),
            Self::RepositorySymlink => (
                "XT0503",
                "The repository holds a symbolic link, which a gate never follows.",
                "replace the link with the file it points at",
            ),
            Self::RepositoryUnreadable => (
                "XT0504",
                "A path git listed could not be read.",
                "check the path the message names exists and is readable",
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
            Self::ProofUnreadReport => (
                "XT2011",
                "The report passed its schema and still lacks a field a layer of this audit reads, so the schema and the reader disagree.",
                "report it; either the schema should require the field or the reader should not demand it",
            ),
            Self::ThreadsContradicted => (
                "XT2101",
                "A report's thread standing for a test binary contradicts what the engine recording witnesses, or is no standing a run gives.",
                "the runner decided what its own recording does not support: re-run, and report it if it recurs",
            ),
            Self::ExplorationContradicted => (
                "XT2102",
                "A report's exploration of a binary's schedules comes to something other than its recorded controls do.",
                "the runner decided what its own recording does not support: re-run, and report it if it recurs",
            ),
            Self::ScheduleUnreplayable => (
                "XT2103",
                "The recorded controls of an exploration are not a schedule the exploration could have run.",
                "re-run with `--trace`; a recording that cannot be replayed cannot be counted as agreement",
            ),
            Self::ThreadsUnwitnessed => (
                "XT2104",
                "The recording lacks the build or baseline record a thread standing is derived from.",
                "re-run with `--trace` using this release",
            ),
            Self::FaultContradicted => (
                "XT2105",
                "A report's decision about a fault contradicts the fault's recorded executions.",
                "the runner decided what its own recording does not support: re-run, and report it if it recurs",
            ),
            Self::RepairContradicted => (
                "XT2106",
                "A report's repair of a disposition contradicts the recorded execution of that repair.",
                "the runner decided what its own recording does not support: re-run, and report it if it recurs",
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
            Self::CarryPage => (
                "XT3008",
                "The carry page, `docs/engine/carry.md`, lacks a closed block the carry audit reads its lists from.",
                "restore the fenced block the message names on the page",
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
            Self::RecordingOffSchema => (
                "XT6003",
                "A line of a recording departs from its producer's published schema, so a reader could meet an absent required field.",
                "re-run with `--trace` using this release; a recording off its schema is not one to re-decide",
            ),
            Self::RecordingUnread => (
                "XT6004",
                "A line of a recording passed its producer's schema and still lacks a field a reader of this audit reads, so the schema and the reader disagree.",
                "report it; either the schema should require the field or the reader should not demand it",
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
