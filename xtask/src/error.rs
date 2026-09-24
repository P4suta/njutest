// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every code an xtask error carries, with what it means and what to do about it, which `docs/errors.md` lists and a test holds equal.

/// One stable code, the searchable name of one failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCode {
    /// `XT` and four digits.
    pub code: &'static str,
    /// What went wrong.
    pub meaning: &'static str,
    /// What to do about it.
    pub remedy: &'static str,
}

/// A gate refused the tree.
pub const GATE_REFUSED: ErrorCode = ErrorCode {
    code: "XT0001",
    meaning: "A gate refused the tree, and its message names every place it refused.",
    remedy: "fix each place the message names, or the rule it names, and run the gate again",
};
/// A line of the seam ledger is not one the ratchet reads.
pub const SEAM_LEDGER: ErrorCode = ErrorCode {
    code: "XT0002",
    meaning: "A line of the seam allowlist is not one the ratchet can read.",
    remedy: "write the line as the ledger's other lines are written",
};
/// The roadmap's milestone table is malformed.
pub const MILESTONE_REGISTRY: ErrorCode = ErrorCode {
    code: "XT0003",
    meaning: "The roadmap declares no milestone, or one twice.",
    remedy: "give every milestone one row in the roadmap table",
};
/// The root manifest's lint policy could not be read for the fuzz workspace.
pub const FUZZ_POLICY: ErrorCode = ErrorCode {
    code: "XT0004",
    meaning: "The root manifest could not be read, or its workspace lint table is not one `fuzz-clippy` can carry to the fuzz workspace.",
    remedy: "fix the table the message names in the root `Cargo.toml`",
};
/// Cargo could not be started for the fuzz workspace.
pub const FUZZ_CARGO: ErrorCode = ErrorCode {
    code: "XT0005",
    meaning: "`cargo clippy` could not be started for the fuzz workspace.",
    remedy: "check `cargo` is on the path and the pinned toolchain is installed",
};
/// A fixture tree could not be walked or read.
pub const FIXTURE_UNREADABLE: ErrorCode = ErrorCode {
    code: "XT1001",
    meaning: "A fixture's tree could not be walked or one of its files read.",
    remedy: "check the path the message names exists and is readable",
};
/// A fixture tree holds a symbolic link.
pub const FIXTURE_SYMLINK: ErrorCode = ErrorCode {
    code: "XT1002",
    meaning: "A fixture tree holds a symbolic link, which the checks never follow.",
    remedy: "replace the link with the file it points at",
};
/// A fixture path is not UTF-8.
pub const FIXTURE_PATH: ErrorCode = ErrorCode {
    code: "XT1003",
    meaning: "A fixture path is not UTF-8, so no protocol a fixture feeds could spell it.",
    remedy: "rename the path",
};
/// A directory under a fixture group is not a fixture.
pub const NOT_A_FIXTURE: ErrorCode = ErrorCode {
    code: "XT1004",
    meaning: "A directory under a fixture group is not a fixture.",
    remedy: "make it a fixture, with its manifest, lockfile and README, or move it out of the group",
};
/// A fixture's configuration is not TOML.
pub const FIXTURE_CONFIG: ErrorCode = ErrorCode {
    code: "XT1005",
    meaning: "A fixture's configuration is not TOML.",
    remedy: "fix the file the message names",
};
/// A run directory holds no report.
pub const PROOF_UNREADABLE: ErrorCode = ErrorCode {
    code: "XT2001",
    meaning: "The run directory holds no assurance report, or it could not be read.",
    remedy: "point `proofaudit` at the directory a completed run wrote",
};
/// A report is not JSON.
pub const PROOF_UNPARSABLE: ErrorCode = ErrorCode {
    code: "XT2002",
    meaning: "The assurance report is not JSON this audit can read.",
    remedy: "re-run the run that wrote it; a report nothing can parse is not one to re-decide",
};
/// A recording has a line it cannot read.
pub const PROOF_RECORDING: ErrorCode = ErrorCode {
    code: "XT2003",
    meaning: "The runner's recording has a line that is not JSON.",
    remedy: "re-run with `--trace`; a recording that lost a line cannot be counted as agreement",
};
/// A report is not one build measured whole.
pub const PROOF_UNPROJECTED: ErrorCode = ErrorCode {
    code: "XT2004",
    meaning: "The report is not one configured build measured whole, which is what this audit re-decides.",
    remedy: "audit each part against its own recording",
};
/// A document is not the assurance report.
pub const PROOF_UNRECOGNISED: ErrorCode = ErrorCode {
    code: "XT2005",
    meaning: "The document calls itself something other than the assurance report.",
    remedy: "point `proofaudit` at an assurance report",
};
/// An engine run directory holds no report.
pub const ENGINE_UNREADABLE: ErrorCode = ErrorCode {
    code: "XT3001",
    meaning: "The run directory holds no engine run report, or it or a document beside it could not be read.",
    remedy: "point `engine-audit` at the directory a completed engine run wrote",
};
/// An engine report is not JSON.
pub const ENGINE_UNPARSABLE: ErrorCode = ErrorCode {
    code: "XT3002",
    meaning: "The engine run report is not JSON this audit can read.",
    remedy: "re-run the run that wrote it",
};
/// An engine evidence document is malformed.
pub const ENGINE_EVIDENCE: ErrorCode = ErrorCode {
    code: "XT3003",
    meaning: "An evidence document beside the engine run report is not one this audit can read.",
    remedy: "re-run the run that wrote it",
};
/// An engine recording is malformed.
pub const ENGINE_RECORDING: ErrorCode = ErrorCode {
    code: "XT3004",
    meaning: "The engine's recording is not one this audit can read, or is of another schema.",
    remedy: "re-run with `--trace` using this release",
};
/// A ledger is malformed.
pub const ENGINE_LEDGER: ErrorCode = ErrorCode {
    code: "XT3005",
    meaning: "The configuration named as the ledger is not one this audit can read.",
    remedy: "fix the configuration file the message names",
};
/// An engine document is not the run report this release re-decides.
pub const ENGINE_UNRECOGNISED: ErrorCode = ErrorCode {
    code: "XT3006",
    meaning: "The document is not the engine run report, or is of another schema version.",
    remedy: "point `engine-audit` at a run report this release wrote",
};
/// A Kani export could not be read.
pub const KANI_UNREADABLE: ErrorCode = ErrorCode {
    code: "XT4001",
    meaning: "The Kani export could not be read, or is not the closed JSON schema of the pinned release.",
    remedy: "regenerate the export with the pinned Kani",
};
/// A Kani export is not the pinned release for this workspace.
pub const KANI_CONTRACT: ErrorCode = ErrorCode {
    code: "XT4002",
    meaning: "The Kani export's metadata, project or toolchain is not the pinned release run on this workspace.",
    remedy: "regenerate the export here with the pinned Kani and backend",
};
/// A Kani export's harnesses or ledgers are substituted.
pub const KANI_LEDGER: ErrorCode = ErrorCode {
    code: "XT4003",
    meaning: "A harness or check ledger of the Kani export is missing, duplicated, or not the selected production one.",
    remedy: "regenerate the export from the production harness list",
};
/// A Kani proof is not successful and exact.
pub const KANI_UNPROVEN: ErrorCode = ErrorCode {
    code: "XT4004",
    meaning: "A Kani proof's result, assertions, covers, properties, backend evidence or summary is not successful and exact.",
    remedy: "read the harness the message names; a proof that does not hold is a defect to fix, not a gate to relax",
};
/// Kani's counts exceed their evidence type.
pub const KANI_ARITHMETIC: ErrorCode = ErrorCode {
    code: "XT4005",
    meaning: "Kani's result arithmetic exceeded the type its evidence is held in.",
    remedy: "report it; a count that cannot be held is a count this audit refuses to guess",
};
/// A report's model evidence is malformed.
pub const MODEL_REPORT: ErrorCode = ErrorCode {
    code: "XT4101",
    meaning: "The report's model evidence is not the closed verified-v1 shape, or contradicts itself.",
    remedy: "re-run the verified run that wrote it",
};
/// A retained model artifact cannot be audited.
pub const MODEL_ARTIFACT: ErrorCode = ErrorCode {
    code: "XT4102",
    meaning: "A retained model artifact is outside the run directory or cannot be read.",
    remedy: "audit the run directory the artifacts were retained in",
};
/// A retained Kani export does not establish the reported answer.
pub const MODEL_EXPORT: ErrorCode = ErrorCode {
    code: "XT4103",
    meaning: "A retained Kani export is not the pinned schema, or does not establish the answer the report gives.",
    remedy: "read the model record the message names",
};
/// A specimen could not be laid out.
pub const SPECIMEN_UNWRITABLE: ErrorCode = ErrorCode {
    code: "XT5001",
    meaning: "An audit specimen could not be laid out in a temporary directory.",
    remedy: "check the temporary directory is writable",
};
/// A specimen's recording is malformed.
pub const SPECIMEN_EVENT: ErrorCode = ErrorCode {
    code: "XT5002",
    meaning: "An event of an audit specimen's recording is not an object, or lacks its envelope.",
    remedy: "fix the specimen in the sentinel module the gate names",
};
/// A planted text of the sentinels is malformed.
pub const SENTINEL_PLANTED: ErrorCode = ErrorCode {
    code: "XT5101",
    meaning: "A planted text of the lint sentinels is not the header-and-files shape they are read in.",
    remedy: "fix the planted text under `xtask/sentinels/` the message names",
};
/// An identity field exceeds its length prefix.
pub const IDENTITY_FIELD: ErrorCode = ErrorCode {
    code: "XT6001",
    meaning: "An identity field exceeds the length prefix of the recipe it is minted by.",
    remedy: "report it; an identity this recipe cannot spell is not one to truncate",
};
/// A recording line is not JSON.
pub const RECORDING_LINE: ErrorCode = ErrorCode {
    code: "XT6002",
    meaning: "A line of a recording is not JSON.",
    remedy: "re-run with `--trace`",
};
/// A report of `report-diff` could not be read.
pub const DIFF_UNREADABLE: ErrorCode = ErrorCode {
    code: "XT7001",
    meaning: "A report given to `report-diff` is not one this version understands.",
    remedy: "give it two reports this release wrote",
};
/// The bill of materials could not be made.
pub const SBOM_METADATA: ErrorCode = ErrorCode {
    code: "XT7002",
    meaning: "`cargo metadata` could not be read into a bill of materials.",
    remedy: "run `cargo metadata --locked` and fix what it says",
};

/// A failure that carries one stable code.
pub trait Coded: core::fmt::Display {
    /// The stable code this failure carries, which `docs/errors.md` explains.
    fn code(&self) -> ErrorCode;

    /// The failure as a person reads it: its code, then what it says.
    fn coded(&self) -> String {
        format!("{}: {self}", self.code().code)
    }
}

/// Every code an xtask error carries, in code order.
#[cfg(feature = "testkit")]
pub const ERROR_CODES: [ErrorCode; 36] = [
    GATE_REFUSED,
    SEAM_LEDGER,
    MILESTONE_REGISTRY,
    FUZZ_POLICY,
    FUZZ_CARGO,
    FIXTURE_UNREADABLE,
    FIXTURE_SYMLINK,
    FIXTURE_PATH,
    NOT_A_FIXTURE,
    FIXTURE_CONFIG,
    PROOF_UNREADABLE,
    PROOF_UNPARSABLE,
    PROOF_RECORDING,
    PROOF_UNPROJECTED,
    PROOF_UNRECOGNISED,
    ENGINE_UNREADABLE,
    ENGINE_UNPARSABLE,
    ENGINE_EVIDENCE,
    ENGINE_RECORDING,
    ENGINE_LEDGER,
    ENGINE_UNRECOGNISED,
    KANI_UNREADABLE,
    KANI_CONTRACT,
    KANI_LEDGER,
    KANI_UNPROVEN,
    KANI_ARITHMETIC,
    MODEL_REPORT,
    MODEL_ARTIFACT,
    MODEL_EXPORT,
    SPECIMEN_UNWRITABLE,
    SPECIMEN_EVENT,
    SENTINEL_PLANTED,
    IDENTITY_FIELD,
    RECORDING_LINE,
    DIFF_UNREADABLE,
    SBOM_METADATA,
];
