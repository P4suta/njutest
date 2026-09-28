// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `cargo xtask bundle` puts in a release archive, read from the manifests, and what it refuses to guess.

#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "the helpers that read the repository's own files and write the archives these tests read are not themselves tests"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tar::EntryType;
use xtask::bundle::{BundleError, Entry, Package, TemplateError, Values, plan, read_back, render};
use xtask::error::{Coded as _, XtCode};

const LINUX: &str = "x86_64-unknown-linux-gnu";
const WINDOWS: &str = "x86_64-pc-windows-msvc";
const URL: &str = "{ repo }/releases/download/v{ version }/bundled-{ version }-{ target }.tar.gz";
const DIR: &str = "bundled-{ version }-{ target }/{ bin }{ binary-ext }";

fn binstall(url: &str, format: &str, dir: &str) -> serde_json::Value {
    serde_json::json!({ "binstall": { "pkg-url": url, "pkg-fmt": format, "bin-dir": dir } })
}

fn shipped(name: &str, binaries: &[&str]) -> Package {
    Package {
        id: format!("path+file:///w/{name}#0.3.1"),
        name: name.to_owned(),
        version: "0.3.1".to_owned(),
        repository: Some("https://example.invalid/bundled".to_owned()),
        published: true,
        binaries: binaries.iter().map(|binary| (*binary).to_owned()).collect(),
        metadata: binstall(URL, "tgz", DIR),
    }
}

const fn values<'a>(target: &'a str, bin: Option<&'a str>) -> Values<'a> {
    Values {
        name: "shipped",
        version: "0.3.1",
        repo: Some("https://example.invalid/bundled"),
        target,
        bin,
    }
}

fn refused(packages: &[Package]) -> BundleError {
    match plan(packages, LINUX) {
        Ok(planned) => panic!("the manifests were planned rather than refused: {planned:?}"),
        Err(error) => error,
    }
}

#[test]
fn a_template_is_filled_the_way_binstall_fills_it() {
    assert_eq!(
        render(DIR, &values(LINUX, Some("shipped"))),
        Ok("bundled-0.3.1-x86_64-unknown-linux-gnu/shipped".to_owned())
    );
    assert_eq!(
        render(DIR, &values(WINDOWS, Some("shipped"))),
        Ok("bundled-0.3.1-x86_64-pc-windows-msvc/shipped.exe".to_owned()),
        "binstall adds `.exe` to a binary for a Windows target and nothing to any other"
    );
    assert_eq!(
        render(URL, &values(LINUX, None)),
        Ok("https://example.invalid/bundled/releases/download/v0.3.1/\
             bundled-0.3.1-x86_64-unknown-linux-gnu.tar.gz"
            .to_owned())
    );
    assert_eq!(
        render("{name}-{version}", &values(LINUX, None)),
        Ok("shipped-0.3.1".to_owned()),
        "the spaces inside the braces are not part of the name"
    );
}

#[test]
fn a_template_this_command_cannot_fill_as_binstall_would_is_refused() {
    for (template, bin, expected) in [
        (
            "{ target-arch }/{ bin }",
            Some("shipped"),
            TemplateError::Unknown {
                template: "{ target-arch }/{ bin }".to_owned(),
                variable: "target-arch".to_owned(),
            },
        ),
        (
            "{ bin }.tar.gz",
            None,
            TemplateError::Unfilled {
                template: "{ bin }.tar.gz".to_owned(),
                variable: "bin",
            },
        ),
        (
            "{ version",
            None,
            TemplateError::Unbalanced {
                template: "{ version".to_owned(),
            },
        ),
        (
            "version }",
            None,
            TemplateError::Unbalanced {
                template: "version }".to_owned(),
            },
        ),
        (
            "\\{ bin }",
            Some("shipped"),
            TemplateError::Escaped {
                template: "\\{ bin }".to_owned(),
            },
        ),
    ] {
        assert_eq!(
            render(template, &values(LINUX, bin)),
            Err(expected),
            "{template}"
        );
    }
    let no_repository = Values {
        repo: None,
        ..values(LINUX, None)
    };
    assert_eq!(
        render(URL, &no_repository),
        Err(TemplateError::Unfilled {
            template: URL.to_owned(),
            variable: "repo",
        }),
        "a manifest naming no repository leaves binstall no URL to fetch"
    );
}

#[test]
fn the_archive_holds_every_binary_a_shipped_package_declares_where_its_bin_dir_puts_it() {
    let unpublished = Package {
        published: false,
        ..shipped("internal", &["internal"])
    };
    let library = shipped("library", &[]);
    let packages = [
        shipped("second", &["second"]),
        unpublished,
        library,
        shipped("first", &["first", "cargo-first"]),
    ];
    let planned = match plan(&packages, WINDOWS) {
        Ok(planned) => planned,
        Err(error) => panic!("{}", error.coded()),
    };
    let directory = "bundled-0.3.1-x86_64-pc-windows-msvc";
    assert_eq!(planned.archive, format!("{directory}.tar.gz"));
    let programs: Vec<(&str, &str, &str)> = planned
        .programs
        .iter()
        .map(|program| {
            (
                program.package.as_str(),
                program.binary.as_str(),
                program.path.as_str(),
            )
        })
        .collect();
    assert_eq!(
        programs,
        [
            (
                "first",
                "cargo-first",
                &*format!("{directory}/cargo-first.exe")
            ),
            ("first", "first", &*format!("{directory}/first.exe")),
            ("second", "second", &*format!("{directory}/second.exe")),
        ],
        "every binary of every published package, and nothing of one cargo would not publish"
    );
    let documents: Vec<(&str, String)> = planned
        .documents
        .iter()
        .map(|document| (document.name.as_str(), document.path.clone()))
        .collect();
    assert_eq!(
        documents,
        xtask::bundle::DOCUMENTS
            .map(|name| (name, format!("{directory}/{name}")))
            .to_vec(),
        "the licences and the README go where the binaries are"
    );
    assert_eq!(planned.directories, [directory]);
}

#[test]
fn a_shipped_package_that_does_not_say_where_binstall_reads_it_is_refused() {
    let silent = Package {
        metadata: serde_json::json!({ "njutest": { "surface": "incidental" } }),
        ..shipped("silent", &["silent"])
    };
    assert!(
        matches!(refused(&[silent]), BundleError::Unannounced { ref package } if package == "silent"),
        "binstall compiles a package that says nothing, so its binaries are not in the archive"
    );

    let mut overridden = binstall(URL, "tgz", DIR);
    if let Some(table) = overridden
        .get_mut("binstall")
        .and_then(serde_json::Value::as_object_mut)
    {
        table.insert(
            "overrides".to_owned(),
            serde_json::json!({ WINDOWS: { "pkg-fmt": "zip" } }),
        );
    }
    let unread = Package {
        metadata: overridden,
        ..shipped("overridden", &["overridden"])
    };
    assert!(
        matches!(refused(&[unread]), BundleError::Unreadable { .. }),
        "a key this command does not read can change what binstall reads for some target"
    );

    let zipped = Package {
        metadata: binstall(URL, "zip", DIR),
        ..shipped("zipped", &["zipped"])
    };
    assert!(
        matches!(refused(&[zipped]), BundleError::Format { ref format, .. } if format == "zip"),
        "the archive this command writes is a gzipped tar"
    );
}

#[test]
fn a_pkg_url_that_does_not_end_in_a_gzipped_tar_is_refused() {
    for url in [
        "{ repo }/releases/download/v{ version }/bundled-{ target }.zip",
        "{ repo }/releases/download/v{ version }/.tar.gz",
        "{ repo }/download?asset=bundled-{ target }.tar.gz",
    ] {
        let named = Package {
            metadata: binstall(url, "tgz", DIR),
            ..shipped("named", &["named"])
        };
        assert!(
            matches!(refused(&[named]), BundleError::ArchiveName { .. }),
            "{url} names no archive this command writes"
        );
    }
}

#[test]
fn a_cargo_that_cannot_be_started_is_refused_by_name() {
    let missing = std::ffi::OsStr::new("/no/cargo/here");
    match xtask::bundle::packages(
        &root(),
        missing,
        &xtask::environment::Environment::of(Vec::new()),
    ) {
        Ok(packages) => panic!("a cargo that is not there described {packages:?}"),
        Err(error) => {
            assert_eq!(error.code(), XtCode::BundleUnbuilt);
            assert!(
                matches!(&error, BundleError::Start { program, .. } if program == "/no/cargo/here"),
                "{error}"
            );
        }
    }
}

#[test]
fn shipped_packages_that_name_two_archives_are_refused() {
    let elsewhere = Package {
        metadata: binstall(
            "{ repo }/releases/download/v{ version }/other-{ version }-{ target }.tar.gz",
            "tgz",
            DIR,
        ),
        ..shipped("elsewhere", &["elsewhere"])
    };
    let error = refused(&[shipped("first", &["first"]), elsewhere]);
    assert!(
        matches!(error, BundleError::Archives { .. }),
        "one release asset per target, so both packages must name it: {error}"
    );
    assert_eq!(error.code(), XtCode::BundleManifest);
}

#[test]
fn a_binary_placed_outside_the_archive_or_on_another_is_refused() {
    for dir in [
        "../{ bin }",
        "/usr/bin/{ bin }",
        "a/./{ bin }",
        "a//{ bin }",
    ] {
        let escaping = Package {
            metadata: binstall(URL, "tgz", dir),
            ..shipped("escaping", &["escaping"])
        };
        assert!(
            matches!(refused(&[escaping]), BundleError::Path { .. }),
            "{dir} is not a path inside the archive"
        );
    }
    let same = Package {
        metadata: binstall(URL, "tgz", "bundled-{ version }/program{ binary-ext }"),
        ..shipped("same", &["one", "two"])
    };
    assert!(
        matches!(refused(&[same]), BundleError::Twice { .. }),
        "two binaries at one path is one binary lost"
    );
    let document = Package {
        metadata: binstall(URL, "tgz", "bundled/README.md"),
        ..shipped("document", &["document"])
    };
    assert!(
        matches!(refused(&[document]), BundleError::Twice { .. }),
        "a binary where the README goes is one of the two lost"
    );
    let split = Package {
        metadata: binstall(URL, "tgz", "elsewhere-{ version }/{ bin }{ binary-ext }"),
        ..shipped("split", &["split"])
    };
    assert!(
        matches!(
            refused(&[shipped("first", &["first"]), split]),
            BundleError::Directories { .. }
        ),
        "the licences go beside the binaries, which needs one place for them"
    );
}

#[test]
fn a_workspace_that_ships_no_binary_is_refused() {
    let library = shipped("library", &[]);
    let internal = Package {
        published: false,
        ..shipped("internal", &["internal"])
    };
    assert!(
        matches!(refused(&[library, internal]), BundleError::NothingShipped),
        "an archive of nothing is not a release"
    );
}

/// One entry of an archive a test writes: its path, tar type, mode, and bytes.
type Written<'a> = (&'a str, EntryType, u32, &'a [u8]);

/// A gzipped tar holding `entries`, in order.
fn archive(entries: &[Written<'_>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (path, kind, mode, bytes) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(*kind);
        header.set_mode(*mode);
        header.set_size(u64::try_from(bytes.len()).expect("a small entry"));
        builder
            .append_data(&mut header, path, *bytes)
            .expect("an entry of a test archive");
    }
    builder
        .into_inner()
        .and_then(flate2::write::GzEncoder::finish)
        .expect("a test archive")
}

#[test]
fn an_archive_that_reads_back_as_anything_but_its_plan_is_refused() {
    let expected = BTreeMap::from([
        (
            "bundled".to_owned(),
            Entry::of(EntryType::Directory, 0o755, &[]),
        ),
        (
            "bundled/shipped".to_owned(),
            Entry::of(EntryType::Regular, 0o755, b"program"),
        ),
        (
            "bundled/README.md".to_owned(),
            Entry::of(EntryType::Regular, 0o644, b"readme"),
        ),
    ]);
    let directory: Written<'_> = ("bundled/", EntryType::Directory, 0o755, b"");
    let program: Written<'_> = ("bundled/shipped", EntryType::Regular, 0o755, b"program");
    let readme: Written<'_> = ("bundled/README.md", EntryType::Regular, 0o644, b"readme");
    match read_back(&expected, &archive(&[directory, program, readme])) {
        Ok(()) => {}
        Err(error) => panic!("the planned archive was refused: {error}"),
    }
    for (tampered, said) in [
        (vec![directory, program], "bundled/README.md is missing"),
        (
            vec![
                directory,
                program,
                readme,
                ("bundled/extra", EntryType::Regular, 0o755, b"extra"),
            ],
            "bundled/extra was never planned",
        ),
        (
            vec![
                directory,
                ("bundled/shipped", EntryType::Regular, 0o644, b"program"),
                readme,
            ],
            "bundled/shipped reads back as",
        ),
        (
            vec![
                directory,
                ("bundled/shipped", EntryType::Regular, 0o755, b"another"),
                readme,
            ],
            "bundled/shipped reads back as",
        ),
        (
            vec![directory, program, program, readme],
            "bundled/shipped is in it twice",
        ),
        (
            vec![("bundled", EntryType::Regular, 0o755, b""), program, readme],
            "bundled reads back as",
        ),
    ] {
        match read_back(&expected, &archive(&tampered)) {
            Err(BundleError::ReadBack { differences }) => assert!(
                differences
                    .iter()
                    .any(|difference| difference.contains(said)),
                "{said:?} in {differences:?}"
            ),
            other => panic!("{said}: {other:?}"),
        }
    }
    assert!(
        matches!(
            read_back(&expected, b"not an archive"),
            Err(BundleError::ReadBack { ref differences })
                if differences.iter().any(|one| one.contains("does not read as a gzipped tar"))
        ),
        "bytes that are not an archive are refused rather than read as an empty one"
    );
}

#[test]
fn bundle_is_a_command_of_its_own() {
    let asked = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bundle", "--help"])
        .output()
        .unwrap_or_else(|error| panic!("the xtask binary: {error}"));
    let said = String::from_utf8(asked.stdout).expect("clap writes UTF-8");
    let complained = String::from_utf8(asked.stderr).expect("clap writes UTF-8");
    assert!(asked.status.success(), "{complained}{said}");
    for option in ["--target <TRIPLE>", "--out <DIR>"] {
        assert!(said.contains(option), "{option} in {said}");
    }
}

fn root() -> PathBuf {
    njutest_devkit::paths::workspace_root()
}

/// The targets the release workflow builds an archive for.
fn released_targets() -> Vec<String> {
    let path = root().join(".github/workflows/release.yml");
    let workflow = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let found: Vec<String> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .map(ToOwned::to_owned)
        .collect();
    assert!(
        found.len() >= 3,
        "the release builds for three platforms: {found:?}"
    );
    found
}

/// A manifest's `[package.metadata.binstall]` value named `key`.
fn binstall_value(manifest: &toml::Table, key: &str) -> String {
    manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("binstall"))
        .and_then(|binstall| binstall.get(key))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("no [package.metadata.binstall] {key}"))
        .to_owned()
}

/// What binstall makes of `template`, by the four substitutions these manifests use.
fn filled(template: &str, version: &str, target: &str, bin: &str) -> String {
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    template
        .replace("{ version }", version)
        .replace("{ target }", target)
        .replace("{ bin }", bin)
        .replace("{ binary-ext }", extension)
}

#[test]
fn for_every_target_the_release_builds_this_workspace_bundles_what_its_manifests_declare() {
    let root = root();
    let environment = xtask::environment::Environment::of(std::env::vars_os());
    let packages = match xtask::bundle::packages(
        &root,
        njutest_devkit::paths::cargo_binary().as_os_str(),
        &environment,
    ) {
        Ok(packages) => packages,
        Err(error) => panic!("{}", error.coded()),
    };
    let shipped: Vec<njutest_devkit::census::Member> = njutest_devkit::census::members(&root)
        .into_iter()
        .filter(|member| member.published && !member.binaries.is_empty())
        .collect();
    assert!(shipped.len() >= 2, "two products ship: {shipped:?}");
    for target in released_targets() {
        let planned = match plan(&packages, &target) {
            Ok(planned) => planned,
            Err(error) => panic!("{target}: {}", error.coded()),
        };
        let mut expected: BTreeSet<(String, String)> = BTreeSet::new();
        let mut archives: BTreeSet<String> = BTreeSet::new();
        for member in &shipped {
            let path: &Path = &member.directory.join("Cargo.toml");
            let manifest: toml::Table = std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
                .parse::<toml::Table>()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let version = xtask::release::workspace_version(
                &std::fs::read_to_string(root.join("Cargo.toml"))
                    .unwrap_or_else(|error| panic!("Cargo.toml: {error}")),
            )
            .unwrap_or_else(|| panic!("the workspace names its version"));
            let url = filled(&binstall_value(&manifest, "pkg-url"), &version, &target, "");
            archives.insert(url.rsplit('/').next().unwrap_or_default().to_owned());
            for binary in &member.binaries {
                expected.insert((
                    binary.clone(),
                    filled(
                        &binstall_value(&manifest, "bin-dir"),
                        &version,
                        &target,
                        binary,
                    ),
                ));
            }
        }
        let bundled: BTreeSet<(String, String)> = planned
            .programs
            .iter()
            .map(|program| (program.binary.clone(), program.path.clone()))
            .collect();
        assert_eq!(
            bundled, expected,
            "{target}: the archive holds a binary no manifest declares, or leaves one out, or \
             puts one where binstall does not look"
        );
        assert_eq!(
            archives,
            BTreeSet::from([planned.archive.clone()]),
            "{target}: every shipped package names the one archive the bundle writes"
        );
    }
}
