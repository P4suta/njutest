// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledgers the runner's own pages keep, against the code that is the ledger.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own pages are not themselves tests: a page \
              that cannot be read leaves nothing to assert"
)]

use mjutest_cli::config::Config;
use mjutest_cli::report::FindingKind;

fn page(relative: &str) -> String {
    let path = mjutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn a_kind_a_report_carries_is_the_name_it_is_written_under() {
    for kind in FindingKind::ALL {
        assert_eq!(
            serde_json::to_value(kind).ok(),
            Some(serde_json::Value::String(kind.name().to_owned())),
            "a kind is written into the report by one rule and read out of the code by \
             another, and the two are the same word or the page below documents a name \
             no report carries"
        );
    }
}

#[test]
fn every_finding_kind_is_named_on_the_page_that_documents_the_report() {
    let text = page("docs/report-v1.md");
    let missing: Vec<&str> = FindingKind::ALL
        .into_iter()
        .map(FindingKind::name)
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/report-v1.md does not name these, and a report can carry every one of \
         them: a consumer that meets a kind the page does not have has no way to learn \
         what it is claiming. {missing:?}"
    );
}

#[test]
fn every_configuration_key_a_reader_may_write_is_on_the_configuration_page() {
    let text = page("docs/configuration.md");
    let default = toml::to_string(&Config::default()).expect("the defaults serialise");
    let missing: Vec<String> = default
        .lines()
        .filter_map(|line| {
            line.strip_prefix('[').map_or_else(
                || line.split_once(' ').map(|(name, _rest)| name.to_owned()),
                |section| Some(format!("[{section}")),
            )
        })
        .filter(|key| !key.is_empty() && !documented(&text, key))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/configuration.md does not document {missing:?}, and a key nobody wrote \
         down is a key nobody sets"
    );
}

/// Whether the page shows `key`, which for a section of named tables is a table with a name in it.
///
/// A map of tables serialises with its own header and no members — the
/// defaults have no resources in them — while what a reader writes and what
/// the page has to show is one of the named ones. `[resources]` is documented
/// by `[resources.postgres]`, and looking only for the bare header would ask
/// the page to show a section nobody would ever write.
fn documented(text: &str, key: &str) -> bool {
    text.contains(key)
        || key
            .strip_suffix(']')
            .is_some_and(|section| text.contains(&format!("{section}.")))
}

#[test]
fn every_key_the_configuration_page_shows_is_one_the_reader_accepts() {
    let text = page("docs/configuration.md");
    let skeleton: String = text
        .split("```toml")
        .nth(1)
        .expect("the skeleton the page shows")
        .split("```")
        .next()
        .expect("the end of it")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");
    let parsed: Result<Config, _> = toml::from_str(&skeleton);
    assert!(
        parsed.is_ok(),
        "a reader who copies what the page shows gets a file the run refuses, which is \
         the page teaching somebody to write a configuration that does not work: {:?}\n\
         {skeleton}",
        parsed.err()
    );
}

#[test]
fn the_exit_codes_the_page_lists_are_the_ones_a_run_can_carry() {
    let printed = mjutest_cli::cli::exit_codes();
    let text = page("docs/report-v1.md");
    let table: String = text
        .lines()
        .skip_while(|line| !line.starts_with("| Code |"))
        .take_while(|line| line.starts_with('|'))
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        !table.is_empty(),
        "docs/report-v1.md has no exit code table"
    );

    for line in printed.lines().skip(1) {
        let (code, names) = line.trim().split_once(' ').expect("a code and its names");
        let listed = table
            .lines()
            .find(|row| row.starts_with(&format!("| {code} |")))
            .unwrap_or_else(|| panic!("docs/report-v1.md has no row for exit code {code}"));
        for name in names
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let quoted = format!("`{name}`");
            assert!(
                listed.contains(&quoted) || listed.contains(name),
                "a run exits {code} carrying {name}, and the page's row for {code} does \
                 not say so. The help a person reads is printed from the verdicts \
                 themselves; a page written beside it is the copy that goes wrong: \
                 {listed}"
            );
        }
    }

    let named: Vec<&str> = table
        .lines()
        .flat_map(|row| row.split('`').skip(1).step_by(2))
        .collect();
    let invented: Vec<&&str> = named
        .iter()
        .filter(|name| !printed.contains(**name))
        .collect();
    assert!(
        invented.is_empty(),
        "and a name the page lists that no run carries is a promise nothing keeps: a \
         reader waits for an exit that never comes. {invented:?}"
    );
}
