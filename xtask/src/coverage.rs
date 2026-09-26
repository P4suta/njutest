// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path};
use std::process::Command;

use crate::gates::GateError;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Floor {
    scope: String,
    regions: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Report {
    floor: Floor,
    ignore: String,
}

fn floors(source: &str) -> Result<Vec<Floor>, GateError> {
    let mut found = Vec::new();
    let mut named = BTreeSet::new();
    for (index, line) in source.lines().enumerate() {
        let mut words = line.split_whitespace();
        let (Some(scope), Some(regions), None) = (words.next(), words.next(), words.next()) else {
            return Err(GateError(format!(
                "coverage_floors.txt:{}: expected one scope and one region floor",
                index.saturating_add(1)
            )));
        };
        if !named.insert(scope) {
            return Err(GateError(format!(
                "coverage_floors.txt:{}: duplicate scope {scope}",
                index.saturating_add(1)
            )));
        }
        let regions = regions.parse::<u8>().map_err(|error| {
            GateError(format!(
                "coverage_floors.txt:{}: invalid region floor: {error}",
                index.saturating_add(1)
            ))
        })?;
        if regions == 0 || regions > 100 {
            return Err(GateError(format!(
                "coverage_floors.txt:{}: region floor must be between 1 and 100",
                index.saturating_add(1)
            )));
        }
        found.push(Floor {
            scope: scope.to_owned(),
            regions,
        });
    }
    if !named.contains("workspace") {
        return Err(GateError(
            "coverage_floors.txt: missing workspace floor".to_owned(),
        ));
    }
    Ok(found)
}

fn package_dirs(
    root: &Path,
    metadata: &cargo_metadata::Metadata,
) -> Result<BTreeMap<String, String>, GateError> {
    let root = std::fs::canonicalize(root)
        .map_err(|error| GateError(format!("{}: {error}", root.display())))?;
    let mut dirs = BTreeMap::new();
    for package in metadata.workspace_packages() {
        let manifest = package.manifest_path.as_std_path();
        let directory = manifest
            .parent()
            .ok_or_else(|| GateError(format!("{}: no package directory", manifest.display())))?;
        let directory = std::fs::canonicalize(directory)
            .map_err(|error| GateError(format!("{}: {error}", directory.display())))?;
        let relative = directory.strip_prefix(&root).map_err(|error| {
            GateError(format!(
                "{}: package directory is outside {}: {error}",
                directory.display(),
                root.display()
            ))
        })?;
        let mut components = Vec::new();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                return Err(GateError(format!(
                    "{}: package directory has an unhandled component",
                    directory.display()
                )));
            };
            let part = part.to_str().ok_or_else(|| {
                GateError(format!(
                    "{}: package directory is not UTF-8",
                    directory.display()
                ))
            })?;
            if !part.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '-' || character == '_'
            }) {
                return Err(GateError(format!(
                    "{}: package directory cannot be used as a coverage regex",
                    directory.display()
                )));
            }
            components.push(part);
        }
        if components.is_empty() {
            return Err(GateError(format!(
                "{}: package directory is the workspace root",
                directory.display()
            )));
        }
        let relative = components.join("/");
        if dirs.insert(package.name.to_string(), relative).is_some() {
            return Err(GateError(format!(
                "cargo metadata repeats workspace package {}",
                package.name
            )));
        }
    }
    if !dirs.contains_key("compiler-surfaces") {
        return Err(GateError(
            "cargo metadata has no compiler-surfaces workspace package".to_owned(),
        ));
    }
    Ok(dirs)
}

fn plan(floors: &[Floor], dirs: &BTreeMap<String, String>) -> Result<Vec<Report>, GateError> {
    let mut reports = Vec::new();
    for floor in floors {
        if floor.scope != "workspace" && !dirs.contains_key(&floor.scope) {
            return Err(GateError(format!(
                "coverage_floors.txt: {} is not a workspace package",
                floor.scope
            )));
        }
        let mut excluded = BTreeSet::new();
        if floor.scope != "workspace" {
            excluded.insert("fuzz".to_owned());
        }
        for (package, path) in dirs {
            if (floor.scope == "workspace" && package == "compiler-surfaces")
                || (floor.scope != "workspace" && package != &floor.scope)
            {
                excluded.insert(path.clone());
            }
        }
        let alternatives = excluded
            .into_iter()
            .map(|path| path.replace('/', r"[/\\]"))
            .collect::<Vec<_>>()
            .join("|");
        reports.push(Report {
            floor: floor.clone(),
            ignore: format!(r"(^|[/\\])({alternatives})[/\\]"),
        });
    }
    Ok(reports)
}

pub(super) fn ratchet(root: &Path, cargo: &OsStr) -> Result<String, GateError> {
    let floors = floors(include_str!("../coverage_floors.txt"))?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .cargo_path(std::path::PathBuf::from(cargo))
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .map_err(|error| GateError(format!("cargo metadata for coverage: {error}")))?;
    let dirs = package_dirs(root, &metadata)?;
    let reports = plan(&floors, &dirs)?;
    for report in reports {
        let regions = report.floor.regions.to_string();
        let status = Command::new(cargo)
            .current_dir(root)
            .args([
                "llvm-cov",
                "report",
                "--summary-only",
                "--fail-under-regions",
                &regions,
                "--ignore-filename-regex",
                &report.ignore,
            ])
            .status()
            .map_err(|error| {
                GateError(format!(
                    "coverage-ratchet: {} floor could not run: {error}",
                    report.floor.scope
                ))
            })?;
        if !status.success() {
            return Err(GateError(format!(
                "coverage-ratchet: {} floor {} failed with {status}",
                report.floor.scope, report.floor.regions
            )));
        }
    }
    Ok(format!("coverage-ratchet: {} floors held", floors.len()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{Floor, floors, package_dirs, plan};
    use crate::gates::GateError;

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "test assertions diagnose broken floors"
    )]
    fn the_floors_have_five_distinct_scopes() -> Result<(), GateError> {
        let read = floors(include_str!("../coverage_floors.txt"))?;
        assert_eq!(read.len(), 5);
        assert_eq!(
            read.first().map(|floor| floor.scope.as_str()),
            Some("workspace")
        );
        Ok(())
    }

    #[test]
    fn malformed_and_duplicate_floors_are_refused() {
        for source in [
            "workspace 80\nworkspace 81\n",
            "workspace 80 extra\n",
            "rust-mutants 87\n",
            "workspace 0\n",
            "workspace 101\n",
        ] {
            assert!(floors(source).is_err(), "{source}");
        }
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "test assertions diagnose wrong scopes"
    )]
    fn metadata_paths_determine_each_scope() -> Result<(), GateError> {
        let dirs = BTreeMap::from([
            (
                "compiler-surfaces".to_owned(),
                "compiler-surfaces".to_owned(),
            ),
            ("njutest".to_owned(), "crates/njutest".to_owned()),
            ("rust-mutants".to_owned(), "crates/rust-mutants".to_owned()),
            (
                "rust-mutants-cli".to_owned(),
                "crates/rust-mutants-cli".to_owned(),
            ),
            ("xtask".to_owned(), "xtask".to_owned()),
        ]);
        let floors = [
            Floor {
                scope: "workspace".to_owned(),
                regions: 80,
            },
            Floor {
                scope: "rust-mutants".to_owned(),
                regions: 87,
            },
        ];
        let reports = plan(&floors, &dirs)?;
        let Some(workspace) = reports.first() else {
            return Err(GateError("workspace report is missing".to_owned()));
        };
        let workspace_regex = regex::Regex::new(&workspace.ignore)
            .map_err(|error| GateError(format!("{}: {error}", workspace.ignore)))?;
        assert!(workspace_regex.is_match("/repo/compiler-surfaces/src/lib.rs"));
        assert!(!workspace_regex.is_match("/repo/crates/rust-mutants/src/lib.rs"));
        let Some(rust_mutants) = reports.get(1) else {
            return Err(GateError("rust-mutants report is missing".to_owned()));
        };
        let rust_mutants_regex = regex::Regex::new(&rust_mutants.ignore)
            .map_err(|error| GateError(format!("{}: {error}", rust_mutants.ignore)))?;
        assert!(rust_mutants_regex.is_match("/repo/crates/njutest/src/lib.rs"));
        assert!(!rust_mutants_regex.is_match("/repo/crates/rust-mutants/src/lib.rs"));
        let Err(_) = plan(
            &[Floor {
                scope: "missing".to_owned(),
                regions: 80,
            }],
            &dirs,
        ) else {
            return Err(GateError("an unknown package acquired a floor".to_owned()));
        };
        Ok(())
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "test assertions diagnose a scope leak"
    )]
    fn every_coverage_floor_measures_the_one_crate_it_names() -> Result<(), GateError> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| GateError("xtask has no workspace parent".to_owned()))?;
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(root.join("Cargo.toml"))
            .no_deps()
            .other_options(vec!["--locked".to_owned()])
            .exec()
            .map_err(|error| GateError(format!("cargo metadata: {error}")))?;
        let dirs = package_dirs(root, &metadata)?;
        let reports = plan(&floors(include_str!("../coverage_floors.txt"))?, &dirs)?;
        let mut sources = njutest_devkit::census::members(root)
            .into_iter()
            .map(|member| {
                let relative = member.directory.strip_prefix(root).map_err(|error| {
                    GateError(format!("{}: {error}", member.directory.display()))
                })?;
                Ok((
                    member.name.clone(),
                    format!("/repo/{}/src/lib.rs", relative.display()).replace('\\', "/"),
                ))
            })
            .collect::<Result<Vec<_>, GateError>>()?;
        sources.push(("fuzz".to_owned(), "/repo/fuzz/src/lib.rs".to_owned()));
        let mut whole = sources
            .iter()
            .filter(|(name, _)| name != "compiler-surfaces")
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        whole.sort();
        for report in reports {
            let regex = regex::Regex::new(&report.ignore)
                .map_err(|error| GateError(format!("{}: {error}", report.ignore)))?;
            let mut included = sources
                .iter()
                .filter(|(_, path)| !regex.is_match(path))
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            included.sort();
            let expected = if report.floor.scope == "workspace" {
                whole.clone()
            } else {
                vec![report.floor.scope.clone()]
            };
            assert_eq!(
                included, expected,
                "{}: {}",
                report.floor.scope, report.ignore
            );
            for (_, source) in &sources {
                let backslashes = source.replace('/', "\\");
                assert_eq!(
                    regex.is_match(source),
                    regex.is_match(&backslashes),
                    "{}: {}",
                    report.floor.scope,
                    report.ignore
                );
            }
        }
        Ok(())
    }
}
