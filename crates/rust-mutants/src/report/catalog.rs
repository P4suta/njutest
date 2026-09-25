// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The catalog as a document: what a run proposed, refused, and passed over, in the shape a consumer reads.

use serde::{Deserialize, Serialize};

use crate::catalog::Mutant;
use crate::session::{PrepareOptions, Session};
use crate::syntax::Position;

/// The catalog as one JSON document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDocument {
    /// Names the shape, so a reader can tell versions apart.
    pub document_type: String,
    /// The version of that shape.
    pub schema_version: u32,
    /// The engine that produced it.
    pub tool_version: String,
    /// The tree that was read.
    pub workspace: WorkspaceDocument,
    /// What the run asked for.
    pub selection: CatalogSelectionDocument,
    /// Every mutant the compiler accepted.
    pub mutants: Vec<MutantDocument>,
    /// Every candidate the compiler refused.
    pub rejections: Vec<RejectionDocument>,
    /// Every place discovery passed over.
    pub skips: Vec<SkipDocument>,
}

/// What a catalog-v1 preparation asked for.
///
/// The current run-report and stream carry additional execution controls.
/// A separate type keeps those fields from silently changing the historical catalog-v1 identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSelectionDocument {
    /// The tier, when the run did not name operators.
    pub tier: String,
    /// The operators the run named.
    pub operators: Vec<String>,
    /// The include patterns.
    pub include: Vec<String>,
    /// The exclude patterns.
    pub exclude: Vec<String>,
    /// The packages.
    pub packages: Vec<String>,
    /// The cargo arguments the run compiled with, which say which program was measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<Vec<String>>,
}

/// The tree a catalog was read from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocument {
    /// The name of the directory the source root sits in.
    pub root_name: String,
    /// The toolchain, as it names itself.
    pub toolchain: String,
    /// The frozen digest of the copied tree.
    pub workspace_digest: String,
    /// The digest of the catalog itself.
    pub catalog_digest: String,
    /// Where it ran.
    pub platform: PlatformDocument,
}

/// The machine a run happened on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformDocument {
    /// The operating system.
    pub os: String,
    /// The architecture.
    pub arch: String,
    /// The target triple.
    pub target: String,
}

/// What a run asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionDocument {
    /// The tier, when the run did not name operators.
    pub tier: String,
    /// The operators the run named.
    pub operators: Vec<String>,
    /// The include patterns.
    pub include: Vec<String>,
    /// The exclude patterns.
    pub exclude: Vec<String>,
    /// The packages.
    pub packages: Vec<String>,
    /// The cargo arguments the run compiled with, which say which program was measured.
    pub build: Vec<String>,
    /// The per-process guard-take allowance, absent when disabled.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub mutant_steps: Option<u64>,
}

/// One accepted mutant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantDocument {
    /// The dense catalog index the guards name.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The package that owns the file.
    pub package: String,
    /// The family the rule belongs to.
    pub family: String,
    /// The rule's name.
    pub rule: String,
    /// The item the mutation sits in, as a reader writes it: `mod::path::Type::method`.
    pub item: String,
    /// The rule's version, which enters the identity.
    pub rule_version: u32,
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The first byte of the edit.
    pub start_byte: u32,
    /// One past the last byte of the edit.
    pub end_byte: u32,
    /// The SHA-256 of the file the edit was cut from, which is what re-minting the identity needs.
    pub source_digest: String,
    /// The bytes the edit replaces.
    pub original: String,
    /// What they become.
    pub replacement: String,
    /// The body of the branch the compiler vouched the mutation changes nothing outside, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<BranchDocument>,
}

/// The body a branch proof names, so an audit can re-derive a discharge from the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchDocument {
    /// The line the body's opening brace is on.
    pub start_line: u32,
    /// Its 1-based byte column.
    pub start_column: u32,
    /// The line the closing brace is on.
    pub end_line: u32,
    /// Its 1-based byte column.
    pub end_column: u32,
}

/// One refused candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectionDocument {
    /// The dense catalog index, which the accepted mutants share with the refused ones.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The rule that proposed it.
    pub rule: String,
    /// The compiler's error code, when it had one.
    pub code: Option<String>,
    /// What the compiler said.
    pub diagnostic: String,
    /// Whether the compiler refused it on its own, rather than only alongside another mutant.
    pub isolated: bool,
}

/// One reason places were passed over, and how many.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkipDocument {
    /// The reason's name.
    pub reason: String,
    /// The workspace-relative path.
    pub path: String,
    /// How many candidates it hid.
    pub count: u32,
    /// One sentence about the reason.
    pub explanation: String,
}

/// The catalog of a prepared session as a document.
///
/// # Errors
/// Returns an engine error when a workspace name or mutation byte sequence cannot cross the catalog's exact UTF-8 wire boundary.
pub fn document(
    session: &Session,
    options: &PrepareOptions,
) -> Result<CatalogDocument, crate::EngineError> {
    Ok(CatalogDocument {
        document_type: "rust-mutants/catalog".to_owned(),
        schema_version: 1,
        tool_version: crate::VERSION.to_owned(),
        workspace: workspace_document(session)?,
        selection: catalog_selection_document(options),
        mutants: session
            .accepted()
            .iter()
            .filter_map(|index| session.catalog().by_index(*index))
            .map(|mutant| mutant_document(session, mutant))
            .collect::<Result<Vec<_>, _>>()?,
        rejections: rejection_documents(session),
        skips: skip_documents(session),
    })
}

/// The tree a session read, as a document.
///
/// # Errors
/// Returns an engine error when the workspace name cannot cross the catalog's exact UTF-8 wire boundary.
pub fn workspace_document(session: &Session) -> Result<WorkspaceDocument, crate::EngineError> {
    let host = session.toolchain().host().to_owned();
    let (arch, os) = host.split_once('-').unwrap_or((&host, ""));
    Ok(WorkspaceDocument {
        root_name: session.root_name()?,
        toolchain: session.toolchain().rustc_version().summary.clone(),
        workspace_digest: session.workspace_digest().to_owned(),
        catalog_digest: session.catalog().digest().to_owned(),
        platform: PlatformDocument {
            os: os.rsplit('-').next().unwrap_or_default().to_owned(),
            arch: arch.to_owned(),
            target: host.clone(),
        },
    })
}

/// What a command asked for, as a document.
#[must_use]
pub fn selection_document(options: &PrepareOptions) -> SelectionDocument {
    let spelled = |patterns: &[crate::glob::Pattern]| -> Vec<String> {
        patterns.iter().map(ToString::to_string).collect()
    };
    SelectionDocument {
        tier: options.tier.name().to_owned(),
        operators: options.operators.clone(),
        include: spelled(&options.include),
        exclude: spelled(&options.exclude),
        packages: options.packages.clone(),
        build: options.build.arguments(),
        mutant_steps: options.mutant_steps.filter(|steps| *steps > 0),
    }
}

/// What a catalog-v1 document can say without changing that wire identity.
#[must_use]
pub fn catalog_selection_document(options: &PrepareOptions) -> CatalogSelectionDocument {
    let selection = selection_document(options);
    CatalogSelectionDocument {
        tier: selection.tier,
        operators: selection.operators,
        include: selection.include,
        exclude: selection.exclude,
        packages: selection.packages,
        build: (!selection.build.is_empty()).then_some(selection.build),
    }
}

/// Every candidate the compiler refused, as documents.
#[must_use]
pub fn rejection_documents(session: &Session) -> Vec<RejectionDocument> {
    session
        .rejections()
        .iter()
        .map(|rejection| RejectionDocument {
            index: rejection.index,
            id: rejection.id.clone(),
            display_id: rejection.display_id.clone(),
            path: rejection.path.clone(),
            rule: rejection.rule.clone(),
            code: rejection.code.clone(),
            diagnostic: rejection.diagnostic.clone(),
            isolated: rejection.isolated,
        })
        .collect()
}

/// Every place discovery passed over, as documents.
#[must_use]
pub fn skip_documents(session: &Session) -> Vec<SkipDocument> {
    session
        .skips()
        .iter()
        .map(|skip| SkipDocument {
            reason: skip.reason.name().to_owned(),
            path: skip.path.clone(),
            count: skip.count,
            explanation: skip.reason.explanation().to_owned(),
        })
        .collect()
}

/// One accepted mutant, as a document.
///
/// # Errors
/// Returns an engine error when the original or replacement bytes are not exact UTF-8 and therefore cannot inhabit the catalog wire type.
pub fn mutant_document(
    session: &Session,
    mutant: &Mutant,
) -> Result<MutantDocument, crate::EngineError> {
    let position = session.position(mutant).unwrap_or(Position {
        line: 0,
        byte_column: 0,
        char_column: 0,
    });
    let exact = |field: &'static str, bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|source| {
                crate::EngineError::from(crate::workspace::SessionError::CatalogTextNotUtf8 {
                    mutant: mutant.id.to_string(),
                    field,
                    source,
                })
            })
    };
    Ok(MutantDocument {
        index: mutant.index,
        id: mutant.id.to_string(),
        display_id: mutant.display_id.to_string(),
        path: mutant.candidate.path.clone(),
        item: session.item_of(mutant.index).unwrap_or_default().to_owned(),
        package: session
            .package_of(mutant.index)
            .unwrap_or_default()
            .to_owned(),
        family: mutant.candidate.rule.family.name().to_owned(),
        rule: mutant.candidate.rule.name.to_owned(),
        rule_version: mutant.candidate.rule.version,
        line: position.line,
        column: position.byte_column,
        start_byte: mutant.candidate.span.start,
        end_byte: mutant.candidate.span.end,
        source_digest: mutant.candidate.source_digest.clone(),
        original: exact("original", &mutant.candidate.original)?,
        replacement: exact("replacement", &mutant.candidate.replacement)?,
        branch: session.branch(mutant.index).map(|proof| BranchDocument {
            start_line: proof.body_start.line,
            start_column: proof.body_start.byte_column,
            end_line: proof.body_end.line,
            end_column: proof.body_end.byte_column,
        }),
    })
}

impl MutantDocument {
    /// Whether `asked` names this mutant: a prefix of its identity, or a locator as a reader writes it and as `explain` and a run print it.
    #[must_use]
    pub fn answers_to(&self, asked: &str) -> bool {
        self.id.starts_with(asked)
            || crate::session::Locator::parse(asked).is_some_and(|locator| {
                locator.describes_place(&crate::session::Place {
                    path: &self.path,
                    rule: &self.rule,
                    original: self.original.as_bytes(),
                    item: Some(&self.item),
                    line: self.line,
                })
            })
    }
}
