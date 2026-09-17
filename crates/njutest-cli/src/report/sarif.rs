// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The findings as SARIF, for the code-scanning surface a team already reads.

use super::{Finding, FindingKind, Report};

/// The SARIF version this document declares.
pub const VERSION: &str = "2.1.0";

/// The report's findings as one SARIF log.
#[must_use]
pub fn document(report: &Report) -> serde_json::Value {
    serde_json::json!({
        "version": VERSION,
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": { "driver": {
                "name": "njutest",
                "version": report.tool.njutest,
                "informationUri": "https://github.com/P4suta/njutest",
                "rules": rules(report),
            }},
            "automationDetails": { "id": report.run_id },
            "results": report.findings.iter().map(result).collect::<Vec<_>>(),
            "properties": {
                "verdict": format!("{:?}", report.verdict),
                "accounting": serde_json::to_value(report.accounting).unwrap_or_default(),
                "limitations": report.limitations.iter().map(|limitation| {
                    serde_json::json!({ "name": limitation.name, "detail": limitation.detail })
                }).collect::<Vec<_>>(),
            },
        }],
    })
}

fn rules(report: &Report) -> Vec<serde_json::Value> {
    let mut kinds: Vec<String> = report.findings.iter().map(Finding::kind_name).collect();
    kinds.sort();
    kinds.dedup();
    kinds
        .into_iter()
        .map(|kind| {
            serde_json::json!({
                "id": kind,
                "shortDescription": { "text": kind.replace('-', " ") },
            })
        })
        .collect()
}

/// What makes two runs' findings the same finding, which has to survive the commit between them.
///
/// Code scanning carries alert state on this. A mutant's identity is a
/// function of the whole file, so keying on it closes every alert in a file
/// and opens them again as new on any commit touching it, taking a reviewer's
/// dismissals with them. Where the finding names a place, the place is the
/// key; where it names nothing else, its own kind and subject are all there is.
fn fingerprint(finding: &Finding) -> String {
    finding.path.as_ref().map_or_else(
        || format!("{}:{}", finding.kind_name(), finding.subject),
        |path| format!("{}:{}", finding.kind_name(), path),
    )
}

fn result(finding: &Finding) -> serde_json::Value {
    let mut value = serde_json::Map::new();
    value.insert("ruleId".to_owned(), finding.kind_name().into());
    value.insert("level".to_owned(), level(finding.kind).into());
    value.insert(
        "message".to_owned(),
        serde_json::json!({ "text": finding.detail }),
    );
    value.insert(
        "partialFingerprints".to_owned(),
        serde_json::json!({ "njutestFinding/v2": fingerprint(finding) }),
    );
    if let Some((path, position)) = finding
        .path
        .as_ref()
        .zip(finding.position)
        .filter(|(_path, position)| position.line >= 1)
    {
        value.insert(
            "locations".to_owned(),
            serde_json::json!([{
                "physicalLocation": {
                    "artifactLocation": { "uri": path },
                    "region": {
                        "startLine": position.line,
                        "startColumn": position.character_column,
                    },
                },
            }]),
        );
    }
    serde_json::Value::Object(value)
}

const fn level(kind: FindingKind) -> &'static str {
    if kind.is_defect() { "error" } else { "warning" }
}
