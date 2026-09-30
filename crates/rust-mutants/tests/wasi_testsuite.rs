// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! WebAssembly/wasi-testsuite's preview1 tests, each run on the sealed host as a sealed test instance runs, and held to the result `wasi-testsuite.toml` names for it.

#![expect(clippy::panic, reason = "a test reports a setup failure by panicking")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rust_mutants::sealed::bench::{CLOCK, CONTROL_FUEL, LIMITS, WATCHDOG, seed};
use rust_mutants_sealed::{
    Arguments, Environment, Interrupt, Invocation, Preopen, Preopens, RefusalReason, SealedRunner,
    SealedStop, Snapshot, SnapshotBuilder, Transcript, WasiFunction,
};

/// The variable `cargo xtask wasi-testsuite` names the checkout it fetched and verified in.
const SUITE: &str = "NJUTEST_WASI_TESTSUITE";

/// The expectations, from the workspace root.
const EXPECTATIONS: &str = "crates/rust-mutants/tests/wasi-testsuite.toml";

/// The page whose import table every refusal is tied to, from the workspace root.
const HOST_PAGE: &str = "docs/engine/sealed-host.md";

/// Where each language of the suite keeps its prebuilt preview1 tests, below `tests/<language>/`.
const PREVIEW1: &str = "testsuite/wasm32-wasip1";

/// The guest path the suite's own runners preopen a test's root at.
const ROOT: &str = "/";

/// The expectations file as written.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    /// The repository the suite is fetched from.
    repository: String,
    /// The commit of it every expectation is about.
    commit: String,
    /// Every test, once.
    test: Vec<WrittenTest>,
}

/// One test as the file writes it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenTest {
    /// Its language's directory and its own name, as `rust/path_link`.
    name: String,
    /// Whether it passes, or fails because the host refused what it asked.
    result: Outcome,
    /// Every refusal the host records while it runs, each as `function: reason`.
    refused: Vec<String>,
    /// How a refused test ends, as a stop is spelled here.
    ends: Option<String>,
    /// The row of the import table that says the host refuses it, as the row lists its functions.
    row: Option<String>,
}

/// What a test comes to, by the suite's own criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    /// Its exit code is the one the suite expects, and so is its output where the suite names it.
    Pass,
    /// It fails the suite's criteria because the host refused what it asked, as a row of the import table says the host does.
    Refused,
}

/// A refusal the host records: the function refused, and why.
type Refusal = (WasiFunction, RefusalReason);

/// What the host must give for one test.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expected {
    /// The suite's criteria hold.
    Pass,
    /// The suite's criteria do not hold, because the host refused what the test asked.
    Refused {
        /// How the test ends.
        ends: String,
        /// The row of the import table that says so.
        row: String,
    },
}

/// One test's expectation, read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Expectation {
    /// What the host must give.
    expected: Expected,
    /// Every refusal the host must record while it runs, and no other.
    refused: BTreeSet<Refusal>,
}

/// The expectations, read and checked.
#[derive(Debug)]
struct Expectations {
    /// The commit they are about.
    commit: String,
    /// Every test's, by name.
    tests: BTreeMap<String, Expectation>,
}

/// What one test came to on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observed {
    /// Where the suite's own criteria did not hold, what did not; nothing where the test passed.
    missed: Option<String>,
    /// How it ended, as a stop is spelled here.
    ends: String,
    /// Every refusal the host recorded.
    refused: BTreeSet<Refusal>,
}

/// A test's configuration, as the suite's legacy JSON beside its module gives it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Configured {
    /// The arguments after the program's name.
    args: Option<Vec<String>>,
    /// The environment.
    env: Option<BTreeMap<String, String>>,
    /// The directory beside the module preopened at `/`.
    root: Option<String>,
    /// The exit code the test must end with, zero where it is not named.
    exit_code: Option<u32>,
    /// What the test must write to standard output, where it is named.
    stdout: Option<String>,
    /// What the test must write to standard error, where it is named.
    stderr: Option<String>,
}

/// `path` under the workspace root, read as text.
fn workspace_text(path: &str) -> String {
    let at = njutest_devkit::paths::workspace_root().join(path);
    std::fs::read_to_string(&at).unwrap_or_else(|error| panic!("{}: {error}", at.display()))
}

/// The refusal `written` spells as `function: reason`.
fn refusal(written: &str) -> Refusal {
    let (function, reason) = written
        .split_once(": ")
        .unwrap_or_else(|| panic!("{written:?} is not a refusal spelled `function: reason`"));
    let function = WasiFunction::ALL
        .into_iter()
        .find(|known| known.name() == function)
        .unwrap_or_else(|| panic!("{function:?} is no function of WASI preview1"));
    let reason = RefusalReason::ALL
        .into_iter()
        .find(|known| known.name() == reason)
        .unwrap_or_else(|| panic!("{reason:?} is no reason the sealed host refuses a call for"));
    (function, reason)
}

/// One test as the file writes it, checked into what the host must give.
fn expectation(written: WrittenTest) -> (String, Expectation) {
    let name = written.name;
    let refused: BTreeSet<Refusal> = written.refused.iter().map(|one| refusal(one)).collect();
    assert_eq!(
        refused.len(),
        written.refused.len(),
        "{name}: a refusal is named twice"
    );
    let malformed = |ends: Option<String>, row: Option<String>| -> Expected {
        panic!(
            "{name}: a passing test names neither `ends` nor `row`, and a refused one names both \
             and the refusals it rests on; this one is {:?} with ends {ends:?}, row {row:?} and \
             refusals {refused:?}",
            written.result
        )
    };
    let expected = match written.result {
        Outcome::Pass => match (written.ends, written.row) {
            (None, None) => Expected::Pass,
            (ends, row) => malformed(ends, row),
        },
        Outcome::Refused => match (written.ends, written.row) {
            (Some(ends), Some(row)) if !refused.is_empty() => Expected::Refused { ends, row },
            (ends, row) => malformed(ends, row),
        },
    };
    (name, Expectation { expected, refused })
}

/// The expectations `text` writes, every test named once as `language/test`.
fn expectations_of(text: &str) -> Expectations {
    let written: Written =
        toml::from_str(text).unwrap_or_else(|error| panic!("{EXPECTATIONS}: {error}"));
    assert!(
        written.repository.starts_with("https://"),
        "the suite is fetched over https, and {:?} is not",
        written.repository
    );
    assert!(
        written.commit.len() == 40 && written.commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "the suite is pinned by a full commit id, and {:?} is not one",
        written.commit
    );
    let mut tests = BTreeMap::new();
    for test in written.test {
        let (name, expectation) = expectation(test);
        assert!(
            name.split_once('/')
                .is_some_and(|(language, test)| !language.is_empty() && !test.is_empty()),
            "{name:?} is not a test's name, `language/test`"
        );
        assert!(
            tests.insert(name.clone(), expectation).is_none(),
            "{name} is named twice"
        );
    }
    Expectations {
        commit: written.commit,
        tests,
    }
}

/// The rows of the import table on the host's page, by the functions each lists, as what each says the host does.
fn import_table(page: &str) -> BTreeMap<String, String> {
    let table = page.split_once("## The import table").map_or_else(
        || panic!("{HOST_PAGE} has no import table"),
        |(_before, after)| after,
    );
    table
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split(" | ").collect();
            match cells.as_slice() {
                [functions, meaning] => {
                    (functions.trim().replace('`', ""), meaning.trim().to_owned())
                }
                _ => panic!("{HOST_PAGE}: a row of the import table is not two cells: {line}"),
            }
        })
        .collect()
}

/// Where a refused test's expectation is not tied to a row of `table` that lists every function it names and says the host refuses them with the answer each reason gives.
fn untied(name: &str, expectation: &Expectation, table: &BTreeMap<String, String>) -> Vec<String> {
    let Expected::Refused { row, .. } = &expectation.expected else {
        return Vec::new();
    };
    let Some(meaning) = table.get(row) else {
        return vec![format!(
            "{name}: {row:?} is no row of the import table in {HOST_PAGE}"
        )];
    };
    let listed: BTreeSet<&str> = row.split(", ").collect();
    expectation
        .refused
        .iter()
        .filter_map(|(function, reason)| {
            let answer = format!("{:?}", reason.errno()).to_lowercase();
            let said = format!("refused with `{answer}`");
            (!listed.contains(function.name()) || !meaning.contains(&said)).then(|| {
                format!(
                    "{name}: the row {row:?} does not list {} as refused with `{answer}`, which \
                     is how the host answers a refusal for {}",
                    function.name(),
                    reason.name()
                )
            })
        })
        .collect()
}

/// How `stop` is spelled in the expectations.
fn spelled(stop: SealedStop) -> String {
    match stop {
        SealedStop::Returned => "returned".to_owned(),
        SealedStop::Exited { code } => format!("exited {code}"),
        SealedStop::Trapped { kind } => format!("trapped {}", kind.name()),
        SealedStop::FuelExhausted => "fuel exhausted".to_owned(),
        SealedStop::MemoryExhausted => "memory exhausted".to_owned(),
        SealedStop::Halted => "halted".to_owned(),
    }
}

/// `bytes` as a person reads them, every byte that is not printable ASCII escaped.
fn shown(bytes: &[u8]) -> String {
    bytes.escape_ascii().to_string()
}

/// Where `transcript` misses the suite's own criteria for a test configured as `configured`: its exit code, and its output where the suite names it.
fn missed(configured: &Configured, transcript: &Transcript) -> Option<String> {
    let wanted = configured.exit_code.unwrap_or(0);
    let code = match transcript.stop() {
        SealedStop::Returned => Some(0),
        SealedStop::Exited { code } => Some(code),
        SealedStop::Trapped { .. }
        | SealedStop::FuelExhausted
        | SealedStop::MemoryExhausted
        | SealedStop::Halted => None,
    };
    if code != Some(wanted) {
        return Some(format!(
            "it {}, and the suite wants exit code {wanted}",
            spelled(transcript.stop())
        ));
    }
    let streams = [
        ("standard output", &configured.stdout, transcript.stdout()),
        ("standard error", &configured.stderr, transcript.stderr()),
    ];
    streams.into_iter().find_map(|(stream, wanted, captured)| {
        let wanted = wanted.as_ref()?;
        (captured.bytes() != wanted.as_bytes() || captured.truncated() > 0).then(|| {
            format!(
                "its {stream} was \"{}\", and the suite wants \"{}\"",
                shown(captured.bytes()),
                shown(wanted.as_bytes())
            )
        })
    })
}

/// The snapshot of the directory at `directory`, every file and directory below it by its relative path.
fn tree(directory: &Path) -> Snapshot {
    let mut builder: SnapshotBuilder = Snapshot::builder();
    let mut pending = vec![(directory.to_path_buf(), String::new())];
    while let Some((at, relative)) = pending.pop() {
        let mut entries = std::fs::read_dir(&at)
            .and_then(Iterator::collect::<Result<Vec<_>, _>>)
            .unwrap_or_else(|error| panic!("{}: {error}", at.display()));
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().into_string().unwrap_or_else(|name| {
                panic!(
                    "{}: {} is a name no guest path spells",
                    at.display(),
                    name.display()
                )
            });
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let kind = entry
                .file_type()
                .unwrap_or_else(|error| panic!("{}: {error}", entry.path().display()));
            builder = if kind.is_dir() {
                pending.push((entry.path(), path.clone()));
                builder.directory(&path)
            } else if kind.is_file() {
                let bytes = std::fs::read(entry.path())
                    .unwrap_or_else(|error| panic!("{}: {error}", entry.path().display()));
                builder.file(&path, bytes)
            } else {
                panic!(
                    "{}: a test's root holds files and directories alone",
                    entry.path().display()
                )
            }
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        }
    }
    builder
        .build()
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
}

/// The configuration beside the module at `module`, which is none where the suite gives none.
fn configured(module: &Path) -> Configured {
    let at = module.with_extension("json");
    match std::fs::read_to_string(&at) {
        Ok(text) => njutest_devkit::strictjson::decode_str(&text).unwrap_or_else(|error| {
            panic!(
                "{}: {error}; a key this harness does not read is a way of configuring a test it \
                 cannot run as the suite means",
                at.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Configured {
            args: None,
            env: None,
            root: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        },
        Err(error) => panic!("{}: {error}", at.display()),
    }
}

/// The invocation of the test `name`, whose module is at `module`, exactly as a sealed test instance is run: the engine's fuel, limits, clock and seed, the root preopened at `/` as the suite's own runners do, the arguments and environment it names, and standard input at its end.
fn invocation(name: &str, module: &Path, configured: &Configured) -> Invocation {
    let program = module
        .file_name()
        .and_then(|program| program.to_str())
        .unwrap_or_else(|| panic!("{}: a module named in UTF-8", module.display()))
        .to_owned();
    let mut arguments = vec![program];
    if let Some(given) = &configured.args {
        arguments.extend(given.iter().cloned());
    }
    let variables = match &configured.env {
        Some(given) => given
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        None => Vec::new(),
    };
    let preopens = configured
        .root
        .iter()
        .map(|root| {
            let directory = module
                .parent()
                .unwrap_or_else(|| panic!("{}: a module in a directory", module.display()))
                .join(root);
            Preopen::Tree {
                path: ROOT.to_owned(),
                snapshot: tree(&directory),
            }
        })
        .collect();
    Invocation {
        arguments: Arguments::new(arguments).unwrap_or_else(|error| panic!("{name}: {error}")),
        environment: Environment::new(variables).unwrap_or_else(|error| panic!("{name}: {error}")),
        preopens: Preopens::new(preopens).unwrap_or_else(|error| panic!("{name}: {error}")),
        seed: seed(name),
        fuel: CONTROL_FUEL,
        limits: LIMITS,
        clock: CLOCK,
        halt: None,
    }
}

/// What the test `name`, whose module is at `module`, comes to on the host `runner`.
fn observe(runner: &SealedRunner, name: &str, module: &Path) -> Observed {
    let configured = configured(module);
    let bytes =
        std::fs::read(module).unwrap_or_else(|error| panic!("{}: {error}", module.display()));
    let transcript = runner.prepare(&bytes).and_then(|prepared| {
        prepared.invoke(
            &invocation(name, module, &configured),
            &Interrupt::of(Vec::new()),
        )
    });
    match transcript {
        Ok(transcript) => Observed {
            missed: missed(&configured, &transcript),
            ends: spelled(transcript.stop()),
            refused: transcript
                .refusals()
                .iter()
                .map(|refusal| (refusal.function, refusal.reason))
                .collect(),
        },
        Err(error) => Observed {
            missed: Some(format!("the host gave no answer about it: {error}")),
            ends: format!("unanswered {}", error.code().code()),
            refused: BTreeSet::new(),
        },
    }
}

/// `refused` as the expectations spell it.
fn refusals(refused: &BTreeSet<Refusal>) -> String {
    let spelled: Vec<String> = refused
        .iter()
        .map(|(function, reason)| format!("\"{}: {}\"", function.name(), reason.name()))
        .collect();
    format!("[{}]", spelled.join(", "))
}

/// What `observed` is, as a person reads it beside an expectation it departs from.
fn account(observed: &Observed) -> String {
    let criteria = observed.missed.as_ref().map_or_else(
        || "it passed".to_owned(),
        |missed| format!("it failed, as {missed}"),
    );
    format!(
        "the host gave: {criteria}, it ended {}, and the host refused {}",
        observed.ends,
        refusals(&observed.refused)
    )
}

/// What `expectation` says, as a person reads it beside an observation that departs from it.
fn promised(expectation: &Expectation) -> String {
    let result = match &expectation.expected {
        Expected::Pass => "it passes".to_owned(),
        Expected::Refused { ends, row } => {
            format!("it fails on a refusal the row `{row}` documents, and ends {ends}")
        }
    };
    format!(
        "the file says: {result}, and the host refuses {}",
        refusals(&expectation.refused)
    )
}

/// Where `observed` departs from `expectation`, nothing where it is exactly what the file names.
fn departure(expectation: &Expectation, observed: &Observed) -> Option<String> {
    let result_agrees = match (&expectation.expected, &observed.missed) {
        (Expected::Pass, None) => true,
        (Expected::Pass, Some(_)) | (Expected::Refused { .. }, None) => false,
        (Expected::Refused { ends, .. }, Some(_)) => *ends == observed.ends,
    };
    (!result_agrees || expectation.refused != observed.refused)
        .then(|| format!("{}; {}", promised(expectation), account(observed)))
}

/// Every name one of `named` and `held` has and the other does not, as what is wrong with it.
fn unmatched(named: &BTreeSet<&str>, held: &BTreeSet<&str>, commit: &str) -> Vec<String> {
    let unheld = named.difference(held).map(|name| {
        format!("{name}: the file names it, and the suite at {commit} holds no such test")
    });
    let unnamed = held.difference(named).map(|name| {
        format!(
            "{name}: the suite at {commit} holds it, and the file does not name what the host \
             must give for it"
        )
    });
    unheld.chain(unnamed).collect()
}

/// Every preview1 test the checkout at `suite` holds, by its name, with where its module is.
fn tests_of(suite: &Path) -> BTreeMap<String, PathBuf> {
    let languages = suite.join("tests");
    let mut tests = BTreeMap::new();
    let listed = std::fs::read_dir(&languages)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .unwrap_or_else(|error| panic!("{}: {error}", languages.display()));
    for language in listed {
        let preview1 = language.path().join(PREVIEW1);
        let modules = match std::fs::read_dir(&preview1) {
            Ok(entries) => entries
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|error| panic!("{}: {error}", preview1.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => continue,
            Err(error) => panic!("{}: {error}", preview1.display()),
        };
        let language = language.file_name().into_string().unwrap_or_else(|name| {
            panic!(
                "{}: {} is no language's name",
                languages.display(),
                name.display()
            )
        });
        for module in modules.into_iter().map(|entry| entry.path()) {
            if module
                .extension()
                .is_some_and(|extension| extension == "wasm")
            {
                let stem = module
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_else(|| panic!("{}: a module named in UTF-8", module.display()));
                tests.insert(format!("{language}/{stem}"), module.clone());
            }
        }
    }
    tests
}

/// The expectations the repository holds.
fn expectations() -> Expectations {
    expectations_of(&workspace_text(EXPECTATIONS))
}

#[test]
#[ignore = "needs the pinned WebAssembly/wasi-testsuite, which `cargo xtask wasi-testsuite` fetches, verifies and names"]
fn every_preview1_test_ends_as_the_expectations_say() {
    let Some(suite) = std::env::var_os(SUITE) else {
        panic!(
            "{SUITE} names no checkout of the suite: run `cargo xtask wasi-testsuite`, which \
             fetches the pinned commit once, verifies it by its id and names it here"
        );
    };
    let expectations = expectations();
    let held = tests_of(Path::new(&suite));
    let named: BTreeSet<&str> = expectations.tests.keys().map(String::as_str).collect();
    let mut departures = unmatched(
        &named,
        &held.keys().map(String::as_str).collect(),
        &expectations.commit,
    );
    let runner = SealedRunner::new(WATCHDOG).expect("the sealed runner starts");
    for (name, module) in &held {
        let observed = observe(&runner, name, module);
        match expectations.tests.get(name) {
            Some(expectation) => departures.extend(
                departure(expectation, &observed).map(|departure| format!("{name}: {departure}")),
            ),
            None => departures.push(format!("{name}: {}", account(&observed))),
        }
    }
    assert!(
        departures.is_empty(),
        "the {} preview1 tests of WebAssembly/wasi-testsuite at {} and {EXPECTATIONS} disagree \
         about what the sealed host gives:\n  {}",
        held.len(),
        expectations.commit,
        departures.join("\n  ")
    );
}

#[test]
fn the_expectations_name_each_test_once_and_tie_every_refusal_to_a_row_of_the_import_table() {
    let expectations = expectations();
    assert!(
        !expectations.tests.is_empty(),
        "{EXPECTATIONS} names no test"
    );
    let table = import_table(&workspace_text(HOST_PAGE));
    let untied: Vec<String> = expectations
        .tests
        .iter()
        .flat_map(|(name, expectation)| untied(name, expectation, &table))
        .collect();
    assert!(
        untied.is_empty(),
        "a test the host fails by refusing it is one the import table says the host refuses:\n  {}",
        untied.join("\n  ")
    );
}

#[test]
fn a_refusal_the_import_table_does_not_list_is_untied() {
    let table = import_table(&workspace_text(HOST_PAGE));
    let refused = |row: &str, written: &str| Expectation {
        expected: Expected::Refused {
            ends: "trapped unreachable".to_owned(),
            row: row.to_owned(),
        },
        refused: BTreeSet::from([refusal(written)]),
    };
    assert!(
        untied(
            "t",
            &refused("path_link, path_symlink", "path_link: link"),
            &table
        )
        .is_empty()
    );
    assert_eq!(
        untied(
            "t",
            &refused("path_link, path_symlink", "sock_send: network"),
            &table
        )
        .len(),
        1,
        "a row that does not list the function refused ties nothing"
    );
    assert_eq!(
        untied("t", &refused("path_link", "path_link: link"), &table).len(),
        1,
        "a row the table does not hold ties nothing"
    );
    assert_eq!(
        untied("t", &refused("proc_raise", "proc_raise: link"), &table).len(),
        1,
        "a row that says the host answers otherwise than the reason does ties nothing"
    );
}

/// An observation made up for a law about departures.
fn observed(missed: Option<&str>, ends: &str, refused: &[&str]) -> Observed {
    Observed {
        missed: missed.map(ToOwned::to_owned),
        ends: ends.to_owned(),
        refused: refused.iter().map(|written| refusal(written)).collect(),
    }
}

#[test]
fn a_result_other_than_the_one_the_file_names_is_a_departure() {
    let pass = Expectation {
        expected: Expected::Pass,
        refused: BTreeSet::new(),
    };
    let link = Expectation {
        expected: Expected::Refused {
            ends: "trapped unreachable".to_owned(),
            row: "path_link, path_symlink".to_owned(),
        },
        refused: BTreeSet::from([refusal("path_link: link")]),
    };
    let failed = Some("it trapped unreachable, and the suite wants exit code 0");
    assert_eq!(departure(&pass, &observed(None, "returned", &[])), None);
    assert_eq!(
        departure(
            &link,
            &observed(failed, "trapped unreachable", &["path_link: link"])
        ),
        None
    );
    for (expectation, departed, case) in [
        (
            &pass,
            observed(failed, "trapped unreachable", &[]),
            "a pass that failed",
        ),
        (
            &pass,
            observed(None, "returned", &["path_link: link"]),
            "a pass with a refusal it does not name",
        ),
        (
            &link,
            observed(None, "returned", &["path_link: link"]),
            "a refusal that passed",
        ),
        (
            &link,
            observed(failed, "exited 1", &["path_link: link"]),
            "a refusal that ended otherwise",
        ),
        (
            &link,
            observed(failed, "trapped unreachable", &["path_symlink: link"]),
            "a refusal of another function",
        ),
    ] {
        assert!(
            departure(expectation, &departed).is_some(),
            "{case} is a departure"
        );
    }
}

#[test]
fn a_test_the_file_does_not_name_and_a_name_the_suite_does_not_hold_are_each_a_departure() {
    let named = BTreeSet::from(["c/kept", "rust/gone"]);
    let held = BTreeSet::from(["c/kept", "rust/added"]);
    let departures = unmatched(&named, &held, "0123456789012345678901234567890123456789");
    assert_eq!(departures.len(), 2, "{departures:?}");
    assert!(
        departures
            .iter()
            .any(|said| said.starts_with("rust/gone: the file names it")),
        "{departures:?}"
    );
    assert!(
        departures
            .iter()
            .any(|said| said.starts_with("rust/added: the suite at")),
        "{departures:?}"
    );
}

#[test]
fn a_malformed_expectation_is_refused_before_anything_runs() {
    let commit = "609c446139956ff30239f87cb18af1dc6128bed2";
    let head = format!(
        "repository = \"https://github.com/WebAssembly/wasi-testsuite\"\ncommit = \"{commit}\"\n"
    );
    for (case, text) in [
        ("an unknown key", format!("{head}[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = []\nwhy = \"\"\n")),
        ("a passing test naming a row", format!("{head}[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = []\nrow = \"proc_raise\"\n")),
        ("a refused test naming no refusal", format!("{head}[[test]]\nname = \"c/a\"\nresult = \"refused\"\nrefused = []\nends = \"returned\"\nrow = \"proc_raise\"\n")),
        ("a refusal of no function", format!("{head}[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = [\"fd_unknown: link\"]\n")),
        ("a name given twice", format!("{head}[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = []\n[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = []\n")),
        ("a commit that is not an id", "repository = \"https://github.com/WebAssembly/wasi-testsuite\"\ncommit = \"main\"\n[[test]]\nname = \"c/a\"\nresult = \"pass\"\nrefused = []\n".to_owned()),
    ] {
        assert!(
            std::panic::catch_unwind(|| expectations_of(&text)).is_err(),
            "{case} is refused"
        );
    }
}
