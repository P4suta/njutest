// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A fixture with two real dependencies, against what its README says every question about them came to.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "the helpers that start a run and read a fixture are not themselves tests, and a \
              document this test wrote itself is one it may read back"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest_cli::cli::Environment;
use njutest_devkit::fixture::{Fixture, Seam};
use rust_mutants::runner::Cancel;

/// The fixture this suite is about.
const FIXTURE: &str = "fixture-wired";

/// The variable that rewrites the block rather than refusing it, as it does for the mutation fates.
const UPDATE: &str = "UPDATE_FATES";

/// The token the committed configuration holds where the provider's path goes.
///
/// A provider is a program, and where this workspace builds one is not
/// something a committed file can know. Everything else about the two seams —
/// which variable is interposed, which protocol is read, how long an answer is
/// held up — is in the fixture where a reader finds it, because a run whose
/// questions were composed by the test driving it would be a run nobody could
/// reproduce by reading the tree.
const PLACEHOLDER: &str = "UPSTREAM";

#[test]
fn every_question_the_two_seams_licensed_came_to_what_the_readme_says() {
    let fixture = Fixture::copy(FIXTURE);
    pointed_at_the_provider(&fixture);

    let output = verify(&fixture);
    let report = document(&fixture);
    let found = rows(&report);
    assert!(
        !found.is_empty(),
        "a run that recorded no seam at all established nothing this fixture is for: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stated = njutest_devkit::fixture::stated_seams(FIXTURE);
    if found == stated {
        return;
    }
    let drawn = found
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<String>>()
        .join("\n");
    if std::env::var_os(UPDATE).is_some() {
        rewrite(&drawn);
        return;
    }
    panic!(
        "what a run of {FIXTURE} established is not what its README states. A seam fate that \
         moved is either a defect or a decision, and the README is where the decision is \
         recorded, so {UPDATE}=1 rewrites the block and the diff is the review.\n\n\
         stated:\n{}\n\nfound:\n{drawn}\n",
        stated
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join("\n")
    );
}

/// The copy's configuration, with the one thing a committed file cannot hold filled in.
fn pointed_at_the_provider(fixture: &Fixture) {
    let path = fixture.root().join(".njutest.toml");
    let written = std::fs::read_to_string(&path).expect("the fixture's configuration");
    assert!(
        written.contains(PLACEHOLDER),
        "the fixture no longer says where its provider goes, so this driver would run it \
         against whatever {PLACEHOLDER} was replaced with"
    );
    let provider = njutest_devkit::fake_cargo::example("fake_upstream");
    std::fs::write(
        &path,
        written.replace(PLACEHOLDER, &provider.to_string_lossy()),
    )
    .expect("the configuration");
}

/// One run of the fixture, with everything it needs and nothing it does not.
fn verify(fixture: &Fixture) -> std::process::Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        ["njutest", "verify", "--offline", "--locked"]
            .into_iter()
            .map(OsString::from),
        &environment(fixture),
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
    }
}

fn document(fixture: &Fixture) -> serde_json::Value {
    let path = njutest_cli::app::reports::Store::read(fixture.root())
        .run_of(njutest_cli::app::reports::Index::Any)
        .expect("the index names a run")
        .join(njutest_cli::app::reports::DOCUMENT_NAME);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the report"))
        .expect("the report is a document")
}

/// Every seam the report names, in the order it names them.
fn rows(report: &serde_json::Value) -> Vec<Seam> {
    report["seams"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|row| {
            let text = |key: &str| row[key].as_str().map(str::to_owned);
            Seam {
                capability: text("capability").unwrap_or_default(),
                seq: row["seq"].as_u64().unwrap_or_default(),
                rule: text("rule").unwrap_or_default(),
                decision: text("decision").unwrap_or_default(),
                by: text("noticed_by").or_else(|| text("proof")),
            }
        })
        .collect()
}

/// Puts `drawn` in the README's seams block, leaving the rest of the page alone.
fn rewrite(drawn: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(FIXTURE)
        .join("README.md");
    let readme = std::fs::read_to_string(&path).expect("the README");
    let fence = njutest_devkit::fixture::SEAMS_FENCE;
    let (before, rest) = readme
        .split_once(fence)
        .unwrap_or_else(|| panic!("{} has no {fence} block", path.display()));
    let after = rest
        .split_once("\n```")
        .map_or("", |(_, tail)| tail)
        .to_owned();
    std::fs::write(&path, format!("{before}{fence}\n{drawn}\n```{after}")).expect("the README");
}
