// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind, driven in this process against one real run.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::disallowed_methods,
    reason = "the helpers that copy a fixture and run one verification are not themselves \
              tests, a setup that fails is reported by panicking, and a test reads a \
              document by the names the run it drove put there"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use njutest::app::reports::Index;
use njutest::cli::Environment;
use njutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

/// One workspace with one completed run in it, kept for the length of a test.
struct Verified {
    root: PathBuf,
    run: String,
    _dir: tempfile::TempDir,
}

/// Where the runs of one workspace live, spelled as the documented default.
fn runs_root(root: &Path) -> PathBuf {
    root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
}

/// Where one index lives, spelled as the documented default.
fn index_path(root: &Path, index: Index) -> PathBuf {
    root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join(index.file())
}

/// What one command said, driven in this process.
#[derive(Debug)]
struct Said {
    code: u8,
    out: String,
    err: String,
}

/// The environment a run of the tree at `root` is given, with what it writes beside the tree rather than in it.
fn environment(root: &Path) -> Environment {
    Environment {
        cache_directory: njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        working_directory: root.to_path_buf(),
        temp_directory: njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        program: PathBuf::from("this test never runs it"),
        vars: njutest_devkit::paths::environment_for_a_run(),
        cancel: Cancel::new(),
        terminal: njutest::presentation::Terminal::default(),
    }
}

fn ask(root: &Path, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(root),
        &mut out,
        &mut err,
    );
    Said {
        code,
        out: njutest_devkit::process::strict_utf8(&out).into_owned(),
        err: njutest_devkit::process::strict_utf8(&err).into_owned(),
    }
}

/// A copy of `fixture-baseline` that one verification has already run in.
fn verified() -> Verified {
    let dir = tempfile::Builder::new()
        .prefix("njutest-commands-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let said = ask(
        &root,
        &["verify", "--offline", "--locked", "--trace", "--ui=plain"],
    );
    assert_eq!(
        said.code, 2,
        "this fixture has a gap its own tests cannot see: {}{}",
        said.out, said.err
    );
    let run = njutest::app::reports::pointed_at(&root, Index::Any)
        .expect("the index is readable")
        .expect("the index names a run")
        .as_str()
        .to_owned();
    Verified {
        root,
        run,
        _dir: dir,
    }
}

#[test]
fn every_command_that_reads_a_run_reads_the_one_that_ran() {
    let it = verified();
    let lines = ask(&it.root, &["report"]);
    assert_eq!(
        lines.code, 0,
        "reading a report establishes nothing, so it has nothing to fail about: {}",
        lines.err
    );
    assert!(
        lines.out.starts_with("RUN\t") && lines.out.trim_end().ends_with("VERDICT\tINSUFFICIENT"),
        "a report says which run it is about first and what it concluded last, because \
         those are the two things a person came for: {}",
        lines.out
    );

    let document = ask(&it.root, &["report", "--format", "json"]);
    assert_eq!(document.code, 0, "{}", document.err);
    let parsed: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&document.out).expect("the report is a document");
    assert_eq!(
        parsed["report"]["run_id"].as_str(),
        Some(it.run.as_str()),
        "and the document names the same run the lines did: two commands over one \
         directory answering about two runs is a report nobody can act on"
    );

    let named = ask(&it.root, &["report", &it.run]);
    assert_eq!(
        (named.code, named.out == lines.out),
        (0, true),
        "a run named by its identity is the run the latest pointer names, when they are \
         the same run: {}",
        named.out
    );

    let missing = ask(&it.root, &["report", "20200101T000000Z-000000"]);
    assert_eq!(missing.code, 3, "{}", missing.out);
    assert!(
        missing.err.contains("20200101T000000Z-000000"),
        "a run that is not there is named rather than answered about, or a person reads \
         another run's verdict believing it is this one's: {}",
        missing.err
    );

    kept(&it);
    unreadable(&it);
}

/// What `report` says about a run directory whose document is not one.
fn unreadable(it: &Verified) {
    let hollow = "20270101T000000Z-hollow";
    std::fs::create_dir_all(runs_root(&it.root).join(hollow))
        .expect("a run directory with nothing in it");
    let empty = ask(&it.root, &["report", hollow]);
    assert!(
        empty.code == 3 && empty.err.contains(hollow),
        "a run directory with no document in it is a run nobody can be told about, and \
         printing nothing with a clean exit would read as a run that concluded nothing: \
         {}{}",
        empty.out,
        empty.err
    );

    let broken = "20270101T000000Z-broken";
    let directory = runs_root(&it.root).join(broken);
    std::fs::create_dir_all(&directory).expect("a run directory");
    std::fs::write(
        directory.join(njutest::app::reports::DOCUMENT_NAME),
        "{\"schema\":\"from a later release\"}\n",
    )
    .expect("a document this release does not understand");
    let refused = ask(&it.root, &["report", broken]);
    assert!(
        refused.code == 3 && refused.err.contains(broken),
        "and a document this release cannot read is refused by name rather than read as \
         far as it goes: half a report is a verdict about half a run: {}{}",
        refused.out,
        refused.err
    );

    let as_json = ask(&it.root, &["report", broken, "--format", "json"]);
    assert_eq!(
        (as_json.code, as_json.out.trim()),
        (0, "{\"schema\":\"from a later release\"}"),
        "while the document shape hands back what the run wrote, byte for byte: a \
         program piping it wants the document a later release stored and not this \
         release's opinion of it. Only the shape this release has to read for itself \
         refuses: {}",
        as_json.err
    );
}

/// What a run leaves in its own directory, and where the pointers point.
fn kept(it: &Verified) {
    let directory = runs_root(&it.root).join(&it.run);
    for name in [
        njutest::app::reports::DOCUMENT_NAME,
        njutest::app::reports::HTML_NAME,
        njutest::app::reports::SARIF_NAME,
        njutest::app::reports::JUNIT_NAME,
        njutest::report::lines::FILE_NAME,
    ] {
        assert!(
            directory.join(name).exists(),
            "a run writes every projection beside its own document, because the surface \
             a team already reads is the one it will read this on, and one that has to \
             be generated later is one nobody generates: {name} is not in {}",
            directory.display()
        );
    }

    for index in Index::ALL {
        let text = std::fs::read_to_string(index_path(&it.root, index)).expect("the index");
        let pointer: serde_json::Value =
            njutest_devkit::strictjson::decode_str(&text).expect("the index is JSON");
        assert_eq!(
            pointer["run_id"].as_str(),
            Some(it.run.as_str()),
            "and points both indexes at it: this run looked at the whole project, so it \
             is the latest of any kind and the latest full one, and an index left behind \
             names a run whose directory the next collection may take: {}",
            index.file()
        );
    }
}

#[test]
fn a_mutant_is_explained_by_the_run_that_judged_it_and_never_by_a_guess() {
    let it = verified();
    let document = ask(&it.root, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&document.out).expect("the report is a document");
    let mutant = parsed["report"]["builds"][0]["parts"][0]["mutants"][0]["display_id"]
        .as_str()
        .expect("a mutant the run judged")
        .to_owned();
    let full = parsed["report"]["builds"][0]["parts"][0]["mutants"][0]["id"]
        .as_str()
        .expect("the full identity")
        .to_owned();

    let explained = ask(&it.root, &["explain", &mutant]);
    assert_eq!(explained.code, 0, "{}{}", explained.out, explained.err);
    let said: std::collections::BTreeMap<&str, &str> = explained
        .out
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    assert_eq!(
        said.get("RUN"),
        Some(&it.run.as_str()),
        "an explanation says which run recorded this, because a mutation is judged by a \
         run and not by a tree: {}",
        explained.out
    );
    assert!(
        said.get("MUTANT").is_some_and(|it| it.contains(&mutant)),
        "and names the mutation both ways, so the short name a person typed and the \
         identity a report carries are visibly the same thing: {}",
        explained.out
    );
    placed_and_ruled(&said, &explained.out);
    let explained_by_full_identity = ask(&it.root, &["explain", &full]);
    assert_eq!(
        (
            explained_by_full_identity.code,
            explained_by_full_identity.out
        ),
        (0, explained.out.clone()),
        "the full identity and its display prefix name exactly the same mutation"
    );

    let killed = parsed["report"]["builds"][0]["parts"][0]["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|one| one["decision"]["outcome"] == "killed")
        .and_then(|one| one["display_id"].as_str())
        .expect("a mutation the tests noticed")
        .to_owned();
    let caught = ask(&it.root, &["explain", &killed]);
    assert!(
        caught
            .out
            .lines()
            .any(|line| line.starts_with("DECIDED-BY\t")),
        "a mutation something noticed names what noticed it, or a kill is a number with \
         nothing behind it: {}",
        caught.out
    );
    assert!(
        !caught.out.contains("ACCEPTANCE\t"),
        "a mutation the tests killed is not accepted merely because it has no finding: {}",
        caught.out
    );

    let nobody = ask(&it.root, &["explain", "ffffffffffffffff"]);
    assert_eq!(
        nobody.code, 3,
        "while a mutation this run never judged is refused rather than explained as one \
         it did: {}{}",
        nobody.out, nobody.err
    );
    assert!(
        nobody.err.contains("ffffffffffffffff"),
        "naming what was asked for, because the usual answer is a typo: {}",
        nobody.err
    );

    let ambiguous = ask(&it.root, &["explain", ""]);
    assert_ne!(
        ambiguous.code, 0,
        "and a name that could be any of them is not one of them: {}{}",
        ambiguous.out, ambiguous.err
    );
    assert!(
        ambiguous.err.contains("names") && ambiguous.err.contains(", "),
        "which says how many it names and which they are, so a person can pick one \
         without going back to the report: {}",
        ambiguous.err
    );

    open_and_then_accepted(&it, &parsed);
}

/// Where an explanation says the mutation is, and what it says was done there.
fn placed_and_ruled(said: &std::collections::BTreeMap<&str, &str>, whole: &str) {
    let placed = said.get("WHERE").copied().unwrap_or_default();
    assert!(
        placed.contains(".rs:") && placed.matches(':').count() == 2,
        "where it is, as a file and a line and a column, which is what an editor takes: \
         {placed:?}"
    );
    for named in ["RULE", "DECISION"] {
        assert!(
            said.get(named).is_some_and(|it| !it.is_empty()),
            "and what was done to the code and what came of it: {named} is not in\n{whole}"
        );
    }
}

/// What an explanation says about a survivor, before a reviewer looks at it and after.
fn open_and_then_accepted(it: &Verified, parsed: &serde_json::Value) {
    let survivor = parsed["report"]["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["kind"] == "surviving-mutant")
        .and_then(|finding| finding["subject"].as_str())
        .expect("a survivor")
        .to_owned();
    let open = ask(&it.root, &["explain", &survivor]);
    assert!(
        open.out.lines().any(|line| line.starts_with("FINDING\t")),
        "a mutation nothing noticed carries the finding it raised, so an explanation is \
         where a person can go from the identity to what to do about it: {}",
        open.out
    );
    assert!(
        open.out
            .contains("ACCEPTANCE\tnobody has recorded a reason"),
        "a survivor whose own finding still stands is not described as accepted, even when \
         the report also has findings about other mutations — and a reader looking at one \
         is told how to record a reason rather than left to find the form: {}",
        open.out
    );

    accepted_next_time(it, &survivor);
}

/// What an explanation says about a mutation a reviewer has since accepted.
fn accepted_next_time(it: &Verified, survivor: &str) {
    let recorded = ask(
        &it.root,
        &["accept", survivor, "--reason", "reviewed: equivalent"],
    );
    assert_eq!(recorded.code, 0, "{}{}", recorded.out, recorded.err);
    let again = ask(
        &it.root,
        &[
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--ui=plain",
        ],
    );
    assert!(
        again.code <= 2,
        "the next run reads the acceptance: {}{}",
        again.out,
        again.err
    );

    let explained = ask(&it.root, &["explain", survivor]);
    assert!(
        explained.out.contains("ACCEPTANCE\t"),
        "and an explanation of it says a reviewer accepted it, because otherwise a \
         mutation nothing noticed and raising no finding reads as one nothing was \
         recorded about at all: {}",
        explained.out
    );
    assert!(
        !explained
            .out
            .lines()
            .any(|line| line.starts_with("FINDING\t")),
        "with no finding beside it: an acceptance that left the finding standing would \
         be a review that changed nothing: {}",
        explained.out
    );
}

#[test]
fn an_acceptance_is_written_where_the_next_run_reads_it() {
    let it = verified();
    let document = ask(&it.root, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&document.out).expect("the report is a document");
    let survivor = parsed["report"]["builds"][0]["parts"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["kind"] == "surviving-mutant")
        .and_then(|finding| finding["subject"].as_str())
        .expect("a survivor to accept")
        .to_owned();

    let accepted = ask(
        &it.root,
        &[
            "accept",
            &survivor,
            "--reason",
            "reviewed: the two forms compile to one",
            "--owner",
            "quality-team",
            "--ticket",
            "QA-123",
        ],
    );
    assert_eq!(accepted.code, 0, "{}{}", accepted.out, accepted.err);
    assert!(
        accepted.out.contains("accepted") && accepted.out.contains(njutest::config::FILE_NAME),
        "it says what it did and where, because an acceptance written somewhere a person \
         is not looking is one they record twice: {}",
        accepted.out
    );
    let written = std::fs::read_to_string(it.root.join(njutest::config::FILE_NAME))
        .expect("the configuration the acceptance was written into");
    assert!(
        written.contains("[[acceptance]]")
            && written.contains("reviewed: the two forms compile to one")
            && written.starts_with("version = 1\n"),
        "an acceptance is a line in the file the next run reads, with the reason beside \
         it: one recorded anywhere else is a decision the run cannot find, and one with \
         no reason is a mutant nobody looked at: {written}"
    );
    let config = njutest::config::Config::load(&it.root).expect("a configuration it can read");
    let recorded = config
        .acceptance
        .iter()
        .find(|one| {
            one.id
                .starts_with(survivor.split_whitespace().next().unwrap_or(&survivor))
                || survivor.starts_with(&one.id)
        })
        .expect("the acceptance, as the reader reads it back");
    assert_eq!(
        (recorded.owner.as_deref(), recorded.ticket.as_deref()),
        (Some("quality-team"), Some("QA-123")),
        "and who decided and where the decision is written down are carried with it: an \
         acceptance a reader cannot trace back to a person is one nobody can ask about"
    );

    let again = ask(
        &it.root,
        &["accept", &survivor, "--reason", "reviewed twice"],
    );
    assert_eq!(again.code, 0, "{}{}", again.out, again.err);
    assert!(
        again.out.contains("already accepted"),
        "accepting one twice says so rather than writing a second line: two acceptances \
         of one mutation are two reasons, and the run would have to pick one: {}",
        again.out
    );
    let after = njutest::config::Config::load(&it.root).expect("a configuration it can read");
    assert_eq!(
        after.acceptance.len(),
        config.acceptance.len(),
        "and leaves the file with what it had"
    );

    refusals(&it, &survivor);
}

/// What `accept` refuses, which is every name that is not one survivor of this run.
fn refusals(it: &Verified, survivor: &str) {
    let document = ask(&it.root, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&document.out).expect("the report is a document");
    let killed = parsed["report"]["builds"][0]["parts"][0]["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|one| one["decision"]["outcome"] == "killed")
        .and_then(|one| one["display_id"].as_str())
        .expect("a mutation the tests noticed")
        .to_owned();

    let refused = ask(&it.root, &["accept", &killed, "--reason", "why not"]);
    assert_eq!(
        refused.code, 3,
        "only a mutation nothing noticed is a decision to accept: accepting one the \
         tests caught records a review of a gap that is not there, and hides the next \
         one that is: {}{}",
        refused.out, refused.err
    );
    assert!(refused.err.contains("tests"), "{}", refused.err);

    let nobody = ask(&it.root, &["accept", "ffffffffffff", "--reason", "why not"]);
    assert_eq!(
        nobody.code, 3,
        "a name no mutation of this run starts with is refused rather than written down: \
         {}{}",
        nobody.out, nobody.err
    );
    assert!(
        nobody.err.contains("ffffffffffff") && nobody.err.contains("no mutant"),
        "the refused prefix and why it names nothing are both diagnostic: {}",
        nobody.err
    );

    let configured = it.root.join(njutest::config::FILE_NAME);
    let held = std::fs::read_to_string(&configured).expect("the configuration");
    std::fs::write(
        &configured,
        "version = 1\nacceptance = \"not a list of tables\"\n",
    )
    .expect("a configuration whose acceptance is not one");
    let wrong = ask(&it.root, &["accept", survivor, "--reason", "why not"]);
    assert_ne!(
        wrong.code, 0,
        "an acceptance list that is not a list is one nothing may be appended to: writing \
         a table into it would replace what somebody wrote with something the reader \
         refuses, and the reviews of every survivor would go with it: {}{}",
        wrong.out, wrong.err
    );
    assert!(
        wrong.err.contains("acceptance") && wrong.err.contains("expected a sequence"),
        "the field with the wrong shape is named: {}",
        wrong.err
    );

    std::fs::write(&configured, "version = 1\nthis is not toml\n")
        .expect("a configuration that does not parse");
    let unparsable = ask(&it.root, &["accept", survivor, "--reason", "why not"]);
    assert!(
        unparsable.code != 0 && unparsable.err.contains(njutest::config::FILE_NAME),
        "and one that does not parse at all is named rather than written over: {}{}",
        unparsable.out,
        unparsable.err
    );
    std::fs::write(&configured, held).expect("the configuration, as it was");

    let several = ask(&it.root, &["accept", "", "--reason", "why not"]);
    assert!(
        several.code == 3 && several.err.contains("names") && several.err.contains("mutants"),
        "and a prefix that names more than one is refused, because which of them a \
         person meant is not something to guess at: {}{}",
        several.out,
        several.err
    );

    std::fs::remove_file(&configured).expect("remove the configuration");
    std::fs::create_dir_all(&configured).expect("a directory where the configuration belongs");
    let unwritable = ask(&it.root, &["accept", survivor, "--reason", "reviewed"]);
    assert_eq!(unwritable.code, 3, "{}{}", unwritable.out, unwritable.err);
    assert!(
        unwritable.err.contains("unsafe store entry")
            && unwritable.err.contains(njutest::config::FILE_NAME),
        "an acceptance that cannot be persisted names the file and is an error: {}",
        unwritable.err
    );
}

#[test]
fn accept_propagates_both_a_missing_run_and_an_unreadable_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let no_run = ask(dir.path(), &["accept", "abcdef", "--reason", "reviewed"]);
    assert_eq!(no_run.code, 3, "{}{}", no_run.out, no_run.err);
    assert!(
        no_run.err.contains("no run has completed"),
        "{}",
        no_run.err
    );

    let run = "20260101T000000Z-broken";
    std::fs::create_dir_all(runs_root(dir.path()).join(run))
        .expect("a run directory without a report");
    let unreadable = ask(
        dir.path(),
        &["accept", "abcdef", "--reason", "reviewed", "--run", run],
    );
    assert_eq!(unreadable.code, 3, "{}{}", unreadable.out, unreadable.err);
    assert!(
        unreadable.err.contains(run)
            && unreadable
                .err
                .contains(njutest::app::reports::DOCUMENT_NAME),
        "{}",
        unreadable.err
    );
}

#[test]
fn what_a_run_left_behind_is_listed_bundled_and_collected() {
    let it = verified();
    bundled(&it);
    stored(&it);
    planned(&it.root);
}

/// What a bundle of one run holds, and what it refuses to bundle.
fn bundled(it: &Verified) {
    let bundled = ask(&it.root, &["diagnostics", &it.run]);
    assert_eq!(bundled.code, 0, "{}{}", bundled.out, bundled.err);
    let directory = bundled
        .out
        .split_whitespace()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
        .expect("the directory it says it wrote");
    assert!(
        directory.starts_with(it.root.join(".njutest/diagnostics")),
        "a bundle goes where a run's own workings go, so a person collecting one to send \
         somebody knows where to look and a `.gitignore` already covers it: {}",
        directory.display()
    );
    let manifest =
        std::fs::read_to_string(directory.join(njutest::app::diagnostics::MANIFEST_NAME))
            .expect("a bundle says what is in it, or it is a directory somebody has to guess at");
    assert!(
        manifest.ends_with('\n'),
        "and the manifest is a line, so a reader concatenating bundles gets lines: \
         {manifest:?}"
    );
    let described: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&manifest).expect("the manifest is a document");
    assert_eq!(
        described["run_id"].as_str(),
        Some(it.run.as_str()),
        "the manifest names the run it is about, because a bundle a person is sent is \
         one they have to place: {manifest}"
    );
    assert!(
        directory
            .join(njutest::app::reports::DOCUMENT_NAME)
            .exists(),
        "and the report the run wrote is in it: a bundle without it is a directory of \
         workings nobody can read the conclusion of"
    );
    assert!(
        directory.join(njutest::trace::FILE_NAME).exists(),
        "as is the recording, which is what an audit re-derives the proofs from"
    );

    let nobody = ask(&it.root, &["diagnostics", "20200101T000000Z-000000"]);
    assert_eq!(
        nobody.code, 3,
        "while a run that is not there is named rather than bundled as an empty \
         directory: {}{}",
        nobody.out, nobody.err
    );
}

/// What the store says it holds, and what it carries between machines.
fn stored(it: &Verified) {
    std::fs::write(
        it.root.join(njutest::config::FILE_NAME),
        "version = 1\n\n[cache]\nmax_bytes = 1\n",
    )
    .expect("a store bounded at nothing");
    let before = ask(&it.root, &["cache"]);
    assert!(
        before
            .out
            .contains("collected nothing; --gc is what collects"),
        "a store nobody asked to collect takes nothing, however far over its bound it \
         is: a command that collected as a side effect of being asked what it holds \
         would take answers from a run that is still using them. And it says that is \
         what happened, because three zeroes read as a store with nothing to collect: {}",
        before.out
    );
    let swept = ask(&it.root, &["cache", "--gc"]);
    assert!(
        swept.out.contains("evicted") && !swept.out.contains("0 evicted"),
        "while one that was asked takes what is over the bound and says how much: a \
         collection nobody can see the size of is one nobody runs twice: {}",
        swept.out
    );

    let held = ask(&it.root, &["cache"]);
    assert_eq!(held.code, 0, "{}{}", held.out, held.err);
    for said in [
        "root      ",
        "holds     ",
        "collected ",
        "kept      ",
        "temp      ",
    ] {
        assert!(
            held.out.lines().any(|line| line.starts_with(said)),
            "a store says everything it holds, because the answer to a slow run is often \
             that it holds nothing and the answer to a full disk is which of these it \
             is. {said:?} is not in\n{}",
            held.out
        );
    }
    let swept = ask(&it.root, &["cache", "--gc"]);
    assert_eq!(swept.code, 0, "{}{}", swept.out, swept.err);
    assert!(
        swept.out.lines().any(|line| line.starts_with("collected ")),
        "and a collection says what it took, or a person who ran it cannot tell it from \
         one that took nothing: {}",
        swept.out
    );

    let carried = it.root.join("carried.jsonl");
    let out = ask(
        &it.root,
        &["cache", "--export", &carried.display().to_string()],
    );
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    assert!(
        out.out.contains("exported") && out.out.contains(&carried.display().to_string()),
        "an export says how many answers went and where they went, because the file is \
         the thing somebody has to carry: {}",
        out.out
    );
    let back = ask(
        &it.root,
        &["cache", "--import", &carried.display().to_string()],
    );
    assert_eq!(
        back.code, 0,
        "what one machine established is written out and read back on another, and a \
         store that cannot take back what it wrote is one nobody can carry: {}{}",
        back.out, back.err
    );
    assert!(
        back.out.contains("imported"),
        "and an import says the same, the other way round: {}",
        back.out
    );

    preserved_and_named(it);
}

/// What the store says about a directory a run preserved.
fn preserved_and_named(it: &Verified) {
    let preserved = it.root.join("kept-snapshot");
    std::fs::create_dir_all(&preserved).expect("a directory a run preserved");
    let written = njutest::kept::record(
        &it.root,
        &it.run,
        jiff::Timestamp::now(),
        std::slice::from_ref(&preserved),
    )
    .expect("the kept-directory ledger is written");
    assert_eq!(
        written,
        njutest::kept::path(&it.root),
        "the ledger is written at the path the reader uses"
    );
    let listed = ask(&it.root, &["cache"]);
    assert!(
        listed.out.contains("kept      1 directories")
            && listed.out.contains(&format!("({})", it.run)),
        "a directory a run preserved outlives the run, so the store names it and says \
         which run kept it: an unnamed directory on a full disk is one nobody dares \
         remove: {}",
        listed.out
    );

    let elsewhere = ask(&it.root, &["cache", "--import", "nowhere.jsonl"]);
    assert_eq!(
        elsewhere.code, 3,
        "while a file that is not there is named rather than read as an empty store: an \
         import that quietly carried nothing is a machine that does all the work again \
         and reports that it did not: {}{}",
        elsewhere.out, elsewhere.err
    );
    assert!(
        elsewhere.err.contains("nowhere.jsonl"),
        "naming the file, not the store: the store is where it was going and the file is \
         what a person has to go and find: {}",
        elsewhere.err
    );

    let nowhere = it.root.join("no/such/directory/out.jsonl");
    let refused = ask(
        &it.root,
        &["cache", "--export", &nowhere.display().to_string()],
    );
    assert!(
        refused.code == 3 && refused.err.contains("out.jsonl"),
        "and an export with nowhere to write says so rather than reporting that it \
         carried what it could not: {}{}",
        refused.out,
        refused.err
    );
}

/// What saying what a run would measure says.
fn planned(root: &Path) {
    let plain = ask(root, &["plan", "--offline", "--locked"]);
    assert_eq!(
        plain.code, 0,
        "saying what a run would measure measures nothing, so it establishes nothing to \
         fail about: {}{}",
        plain.out, plain.err
    );
    let targets: Vec<&str> = plain
        .out
        .lines()
        .filter(|line| line.starts_with("TARGET\t"))
        .collect();
    assert!(
        !targets.is_empty(),
        "a plan names the binaries a run would measure, one to a line, or it is a number \
         with nothing behind it: {}",
        plain.out
    );
    assert!(
        plain
            .out
            .lines()
            .any(|line| line == format!("TARGETS\t{}", targets.len())),
        "and counts them, so a person reading the tail of a long plan does not have to \
         count the lines above it: {}",
        plain.out
    );
    assert!(
        !plain.out.contains("SCOPE\t"),
        "a plan nobody asked to explain itself says what would run and not why: {}",
        plain.out
    );
    assert_eq!(
        targets
            .iter()
            .filter(|line| line.matches('\t').count() > 2)
            .count(),
        0,
        "and each target line is the identity and the name, with nothing after: {targets:?}"
    );

    let asked_why = ask(root, &["plan", "--offline", "--locked", "--why"]);
    assert_eq!(asked_why.code, 0, "{}{}", asked_why.out, asked_why.err);
    assert!(
        asked_why.out.contains("SCOPE\tevery workspace member"),
        "a plan asked to explain itself says what put the targets in scope first, and a \
         run nobody narrowed looked at every member: {}",
        asked_why.out
    );
    assert!(
        asked_why
            .out
            .lines()
            .filter(|line| line.starts_with("TARGET\t"))
            .all(|line| line.matches('\t').count() == 3),
        "and every target carries the reason it is there, or the flag answered for some \
         of them and not others: {}",
        asked_why.out
    );

    refused_to_plan(root);

    let narrowed = ask(
        root,
        &[
            "plan",
            "--offline",
            "--locked",
            "--why",
            "-p",
            "fixture-baseline",
        ],
    );
    assert!(
        narrowed
            .out
            .contains("SCOPE\tthe packages asked for: fixture-baseline"),
        "while a plan for named packages says which were asked for, because a scope a \
         reader cannot see is a plan they cannot check: {}",
        narrowed.out
    );
}

/// What saying what a run would measure says about a workspace that will not build.
fn refused_to_plan(root: &Path) {
    let broken = root.join("src/lib.rs");
    let source = std::fs::read_to_string(&broken).expect("the library");
    std::fs::write(&broken, format!("{source}\nfn broken( {{\n")).expect("a library cargo refuses");
    let refused = ask(root, &["plan", "--offline", "--locked"]);
    assert_ne!(
        refused.code, 0,
        "a workspace that does not compile has nothing to plan, and saying what it would \
         measure would be naming binaries that cannot be built: {}{}",
        refused.out, refused.err
    );
    assert!(
        refused.err.contains("does not compile"),
        "and says which of the things that can go wrong this was, because a plan that \
         fails in silence reads as a workspace with no targets in it: {}",
        refused.err
    );
    std::fs::write(&broken, &source).expect("the library, as it was");
}

#[test]
fn a_configuration_is_written_once_and_never_over_one_somebody_wrote() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-init-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().to_path_buf();

    let written = ask(&root, &["init"]);
    assert_eq!(written.code, 0, "{}{}", written.out, written.err);
    assert!(
        written.out.contains(njutest::config::FILE_NAME),
        "it says which file it wrote, because a command that writes in silence is one a \
         person runs again: {}",
        written.out
    );
    let path = root.join(njutest::config::FILE_NAME);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        njutest::config::skeleton(),
        "what init writes is the skeleton, so a person who reads the file and a person \
         who reads the documentation of it are reading one thing"
    );

    std::fs::write(&path, "version = 1\n# mine\n").expect("a configuration somebody wrote");
    let refused = ask(&root, &["init"]);
    assert!(
        refused.err.contains("NJ1005"),
        "and a refusal carries the code a person greps for and says how to mean it: {}",
        refused.err
    );
    assert_eq!(
        refused.code, 3,
        "a second init does not write over a configuration somebody has edited: the \
         acceptances in it are the reviews of every survivor this project has looked at, \
         and they are not recoverable from anywhere else: {}{}",
        refused.out, refused.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        "version = 1\n# mine\n",
        "and leaves it exactly as it was"
    );

    let forced = ask(&root, &["init", "--force"]);
    assert_eq!(
        forced.code, 0,
        "while a person who says to replace it is one who meant to: {}{}",
        forced.out, forced.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        njutest::config::skeleton()
    );
}

#[test]
fn the_parts_of_one_catalog_are_put_back_together_and_the_parts_of_two_refused() {
    let it = verified();
    let one = sharded(&it, "1/2");
    let two = sharded(&it, "2/2");

    let whole = ask(
        &it.root,
        &[
            "merge",
            &one.display().to_string(),
            &two.display().to_string(),
        ],
    );
    let combined = njutest::report::json::parse(&whole.out).unwrap_or_else(|error| {
        panic!(
            "the whole is a report a reader takes: {error}\n{}{}",
            whole.out, whole.err
        )
    });
    assert_eq!(
        whole.code,
        combined.verdict().exit_code(),
        "with nowhere named to write it, the whole goes to the stream a pipe reads, and \
         the code is the verdict's: {}",
        whole.err
    );

    let elsewhere = it.root.join("whole.json");
    let written = ask(
        &it.root,
        &[
            "merge",
            &one.display().to_string(),
            &two.display().to_string(),
            "--output",
            &elsewhere.display().to_string(),
        ],
    );
    assert!(
        written.out.trim().is_empty() && elsewhere.exists(),
        "while a person who named a file gets the file and not the stream as well: a \
         pipeline that redirects one and reads the other would have it twice: {}",
        written.out
    );

    let nonsense = it.root.join("nonsense.json");
    std::fs::write(&nonsense, "this is not a report\n").expect("a file that is not one");
    let refused = ask(
        &it.root,
        &[
            "merge",
            &one.display().to_string(),
            &nonsense.display().to_string(),
        ],
    );
    assert_eq!(
        refused.code, 3,
        "a part that is not a report is refused rather than skipped: the whole would be \
         the rest of the catalog wearing the name of all of it: {}{}",
        refused.out, refused.err
    );
    assert!(
        refused.err.contains("nonsense.json"),
        "and names the file, because which of several parts it was is the question: {}",
        refused.err
    );

    two_trees_and_nowhere_to_write(&it, &one, &two);
}

/// Where one shard of this workspace's catalog wrote its document.
fn sharded(it: &Verified, shard: &str) -> PathBuf {
    let before = run_names(&it.root);
    let said = ask(
        &it.root,
        &[
            "verify",
            "--offline",
            "--locked",
            "--no-cache",
            "--ui=plain",
            "--shard",
            shard,
        ],
    );
    assert_eq!(said.code, 2, "{}{}", said.out, said.err);
    let fresh = run_names(&it.root)
        .into_iter()
        .find(|name| !before.contains(name))
        .expect("a sharded run writes its own directory");
    runs_root(&it.root)
        .join(fresh)
        .join(njutest::app::reports::DOCUMENT_NAME)
}

/// The run directories this workspace currently holds.
fn run_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(runs_root(root))
        .expect("the runs directory")
        .map(|entry| {
            entry
                .expect("a run entry is readable")
                .file_name()
                .into_string()
                .unwrap_or_else(|spelling| {
                    panic!(
                        "test protocol paths are UTF-8: {}",
                        njutest_devkit::paths::utf8(Path::new(&spelling))
                    )
                })
        })
        .collect();
    names.sort();
    names
}

/// The two things `merge` refuses that a pipeline actually meets.
fn two_trees_and_nowhere_to_write(it: &Verified, one: &Path, two: &Path) {
    let elsewhere = it.root.join("elsewhere.json");
    let mut other: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&std::fs::read_to_string(two).expect("the part"))
            .expect("a part");
    other["report"]["repository"]["workspace_digest"] = serde_json::json!("c".repeat(64));
    std::fs::write(
        &elsewhere,
        serde_json::to_string_pretty(&other).expect("a report a reader takes"),
    )
    .expect("a part of another tree");
    let two_trees = ask(
        &it.root,
        &[
            "merge",
            &one.display().to_string(),
            &elsewhere.display().to_string(),
        ],
    );
    assert!(
        two_trees.code == 3 && two_trees.err.contains("the repository evidence"),
        "two parts of two trees are not two parts of one: added up they would be a \
         verdict about a tree neither of them measured, which is the one thing sharding \
         may never buy: {}{}",
        two_trees.out,
        two_trees.err
    );

    let nowhere = it.root.join("no/such/place/whole.json");
    let unwritable = ask(
        &it.root,
        &[
            "merge",
            &one.display().to_string(),
            &two.display().to_string(),
            "--output",
            &nowhere.display().to_string(),
        ],
    );
    assert!(
        unwritable.code == 3 && unwritable.err.contains("whole.json"),
        "and a whole with nowhere to be written says so rather than exiting on the \
         verdict of a report nobody will find: {}{}",
        unwritable.out,
        unwritable.err
    );
}

#[test]
fn the_command_a_run_tells_a_reader_to_type_is_one_that_works() {
    let it = verified();
    let said = ask(&it.root, &["report"]);
    let printed: Vec<String> = said
        .out
        .lines()
        .chain(said.err.lines())
        .filter_map(|line| line.split_once("njutest explain ").map(|(_, rest)| rest))
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .filter(|named| !named.is_empty())
        .collect();
    assert!(
        !printed.is_empty(),
        "a run with survivors in it tells a reader what to type next: {}{}",
        said.out,
        said.err
    );
    for named in printed {
        let answered = ask(&it.root, &["explain", &named]);
        assert_eq!(
            answered.code, 0,
            "a tool that prints a command a reader cannot run has told them nothing, and \
             something following it has been sent in a circle: `njutest explain {named}` \
             said {}{}",
            answered.out, answered.err
        );
    }
}
