// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rendering what the engine established, for a person and for a program.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rust_mutants::catalog::Mutant;
use rust_mutants::discover::Discovery;
use rust_mutants::execute::MutantResult;
use rust_mutants::session::Session;
use rust_mutants::syntax::{Position, Skip};
use serde::Serialize;

/// `path:line:column`, the spelling every editor and every `::warning`
/// consumer already understands.
#[must_use]
pub fn at(path: &str, position: Position) -> String {
    format!("{path}:{}:{}", position.line, position.byte_column)
}

/// The position of a mutant's edit, found by counting the lines of the file
/// it came from.
#[must_use]
pub fn position_in(source: &str, offset: u32) -> Position {
    rust_mutants::syntax::LineIndex::new(source).position(source, offset)
}

/// One line per candidate: what it is, where it is, and what it does.
#[must_use]
pub fn list(discovery: &Discovery, sources: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for located in &discovery.candidates {
        let candidate = &located.found.candidate;
        let mutant = discovery
            .catalog
            .by_id(&candidate.id().unwrap_or_default())
            .map(|mutant| mutant.display_id.clone())
            .unwrap_or_default();
        let position = sources
            .get(&candidate.path)
            .map_or(located.found.position, |source| {
                position_in(source, candidate.span.start)
            });
        let written = writeln!(
            text,
            "{mutant}  {rule:<26}  {where_}  {original:?} => {replacement:?}",
            rule = candidate.rule.to_string(),
            where_ = at(&candidate.path, position),
            original = String::from_utf8_lossy(&candidate.original),
            replacement = String::from_utf8_lossy(&candidate.replacement),
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text
}

/// The skip tallies, widest reason first.
#[must_use]
pub fn why_skipped(skips: &[Skip]) -> String {
    let mut totals: BTreeMap<&str, (u32, String)> = BTreeMap::new();
    for skip in skips {
        let entry = totals
            .entry(skip.reason.name())
            .or_insert_with(|| (0, skip.reason.explanation().to_owned()));
        entry.0 = entry.0.saturating_add(skip.count);
    }
    let mut rows: Vec<(&str, u32, String)> = totals
        .into_iter()
        .map(|(reason, (count, why))| (reason, count, why))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let mut text = String::new();
    for (reason, count, why) in rows {
        let written = write!(text, "{count:>6}  {reason}\n          {why}\n");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    if text.is_empty() {
        text.push_str("nothing was passed over\n");
    }
    text
}

/// What preparation established, for a person.
#[must_use]
pub fn catalog(session: &Session) -> String {
    let mut text = String::new();
    let written = write!(
        text,
        "workspace {}\ncatalog   {}\n{} accepted, {} refused, {} skipped\n\n",
        session.workspace_digest(),
        session.catalog().digest(),
        session.accepted().len(),
        session.rejections().len(),
        session
            .skips()
            .iter()
            .map(|skip| u64::from(skip.count))
            .sum::<u64>(),
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    for index in session.accepted() {
        if let Some(mutant) = session.catalog().by_index(*index) {
            let written = writeln!(text, "{}", one_line(mutant));
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if !session.rejections().is_empty() {
        text.push_str("\nrefused by the compiler:\n");
        for rejection in session.rejections() {
            let written = writeln!(
                text,
                "{}  {:<26}  {}  {}",
                rejection.display_id,
                rejection.rule,
                rejection.path,
                rejection
                    .diagnostic
                    .lines()
                    .next()
                    .unwrap_or("no diagnostic"),
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    text
}

fn one_line(mutant: &Mutant) -> String {
    format!(
        "{}  {:<26}  {}  {:?} => {:?}",
        mutant.display_id,
        mutant.candidate.rule.to_string(),
        mutant.candidate.path,
        String::from_utf8_lossy(&mutant.candidate.original),
        String::from_utf8_lossy(&mutant.candidate.replacement),
    )
}

/// The catalog as one JSON document.
///
/// The shape is the contract in `docs/engine/json-schema.md`, and
/// `schema/rust-mutants-catalog-v1.json` is the schema a test validates it
/// against. Every object closes with `additionalProperties: false`, so a
/// field added without a version bump fails that test rather than a
/// consumer.
#[derive(Debug, Serialize)]
pub struct CatalogDocument {
    /// Names the shape, so a reader can tell versions apart.
    pub document_type: &'static str,
    /// The version of that shape.
    pub schema_version: u32,
    /// The engine that produced it.
    pub tool_version: String,
    /// The tree that was read.
    pub workspace: WorkspaceDocument,
    /// What the run asked for.
    pub selection: SelectionDocument,
    /// Every mutant the compiler accepted.
    pub mutants: Vec<MutantDocument>,
    /// Every candidate the compiler refused.
    pub rejections: Vec<RejectionDocument>,
    /// Every place discovery passed over.
    pub skips: Vec<SkipDocument>,
}

/// The tree a catalog was read from.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct PlatformDocument {
    /// The operating system.
    pub os: String,
    /// The architecture.
    pub arch: String,
    /// The target triple.
    pub target: String,
}

/// What a run asked for.
#[derive(Debug, Serialize)]
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
}

/// One accepted mutant.
#[derive(Debug, Serialize)]
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
    /// The bytes the edit replaces.
    pub original: String,
    /// What they become.
    pub replacement: String,
}

/// One refused candidate.
#[derive(Debug, Serialize)]
pub struct RejectionDocument {
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
}

/// One reason places were passed over, and how many.
#[derive(Debug, Serialize)]
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
#[must_use]
pub fn document(session: &Session, scope: &crate::cli::Scope) -> CatalogDocument {
    let host = session.toolchain().host().to_owned();
    let (arch, os) = host.split_once('-').unwrap_or((&host, ""));
    CatalogDocument {
        document_type: "rust-mutants/catalog",
        schema_version: 1,
        tool_version: rust_mutants::VERSION.to_owned(),
        workspace: WorkspaceDocument {
            root_name: session.root_name(),
            toolchain: session.toolchain().rustc_version().summary.clone(),
            workspace_digest: session.workspace_digest().to_owned(),
            catalog_digest: session.catalog().digest().to_owned(),
            platform: PlatformDocument {
                os: os.rsplit('-').next().unwrap_or_default().to_owned(),
                arch: arch.to_owned(),
                target: host.clone(),
            },
        },
        selection: SelectionDocument {
            tier: format!("{:?}", scope.tier).to_lowercase(),
            operators: scope.operators.clone(),
            include: scope.include.clone(),
            exclude: scope.exclude.clone(),
            packages: scope.packages.clone(),
        },
        mutants: session
            .accepted()
            .iter()
            .filter_map(|index| session.catalog().by_index(*index))
            .map(|mutant| mutant_document(session, mutant))
            .collect(),
        rejections: session
            .rejections()
            .iter()
            .map(|rejection| RejectionDocument {
                id: rejection.id.clone(),
                display_id: rejection.display_id.clone(),
                path: rejection.path.clone(),
                rule: rejection.rule.clone(),
                code: rejection.code.clone(),
                diagnostic: rejection.diagnostic.clone(),
            })
            .collect(),
        skips: session
            .skips()
            .iter()
            .map(|skip| SkipDocument {
                reason: skip.reason.name().to_owned(),
                path: skip.path.clone(),
                count: skip.count,
                explanation: skip.reason.explanation().to_owned(),
            })
            .collect(),
    }
}

fn mutant_document(session: &Session, mutant: &Mutant) -> MutantDocument {
    let position = session.position(mutant).unwrap_or(Position {
        line: 0,
        byte_column: 0,
        char_column: 0,
    });
    MutantDocument {
        index: mutant.index,
        id: mutant.id.clone(),
        display_id: mutant.display_id.clone(),
        path: mutant.candidate.path.clone(),
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
        original: String::from_utf8_lossy(&mutant.candidate.original).into_owned(),
        replacement: String::from_utf8_lossy(&mutant.candidate.replacement).into_owned(),
    }
}

/// Everything known about one mutant.
#[must_use]
pub fn explain(session: &Session, mutant: &Mutant, source: Option<&str>) -> String {
    let candidate = &mutant.candidate;
    let where_ = source.map_or_else(
        || format!("{}:{}", candidate.path, candidate.span),
        |source| at(&candidate.path, position_in(source, candidate.span.start)),
    );
    let mut text = String::new();
    for (label, value) in [
        ("mutant", mutant.id.clone()),
        ("short", mutant.display_id.clone()),
        ("index", mutant.index.to_string()),
        (
            "rule",
            format!("{} ({})", candidate.rule, candidate.rule.family.name()),
        ),
        ("where", where_),
        (
            "edit",
            format!(
                "{:?} => {:?}",
                String::from_utf8_lossy(&candidate.original),
                String::from_utf8_lossy(&candidate.replacement)
            ),
        ),
        ("file", candidate.source_digest.clone()),
    ] {
        let written = writeln!(text, "{label:<9} {value}");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push_str(&verdict(session, mutant));
    text
}

/// What the compiler made of one mutant, and what would run it.
fn verdict(session: &Session, mutant: &Mutant) -> String {
    let mut text = String::new();
    if let Some(rejection) = session
        .rejections()
        .iter()
        .find(|rejection| rejection.id == mutant.id)
    {
        text.push_str("verdict   refused by the compiler\n");
        for line in rejection.diagnostic.lines() {
            let written = writeln!(text, "          {line}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        return text;
    }
    text.push_str("verdict   accepted; it compiles and can be executed\n");
    let targets: Vec<&str> = session
        .targets()
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    let written = writeln!(text, "targets   {}", targets.join(", "));
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    text
}

/// What one execution said.
#[must_use]
pub fn outcome(result: &MutantResult, mutant: &Mutant) -> String {
    let mut text = format!(
        "{} {}  {}  {}\n",
        mutant.display_id,
        mutant.candidate.rule,
        result.outcome.name(),
        result.target,
    );
    let written = write!(
        text,
        "exit {}  {} ms",
        result.exit_code,
        result.duration.as_millis()
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    if let Some(summary) = result.summary {
        let written = write!(
            text,
            "  {} passed, {} failed, {} ignored, {} filtered out",
            summary.passed, summary.failed, summary.ignored, summary.filtered_out
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push('\n');
    text
}

/// The exit code an outcome earns: zero when the tests noticed the mutant,
/// one when they did not, and two when nothing was established.
#[must_use]
pub const fn exit_code(outcome: rust_mutants::outcome::Outcome) -> u8 {
    use rust_mutants::outcome::Outcome;
    match outcome {
        Outcome::Killed | Outcome::TimedOut => 0,
        Outcome::Survived => 1,
        // Not run, inconclusive, errored, and whatever a later version
        // adds: nothing was established, which is not a verdict.
        _ => crate::EXIT_USAGE,
    }
}
