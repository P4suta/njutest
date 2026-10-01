// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one invocation established, remembered under its digest and answered from again.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants_sealed::{SealedDigest, Transcript, Transcripts};

use crate::common::{command, invocation, runner};

/// A store whose records live under `dir`, as a run places one under its cache directory.
fn store(dir: &std::path::Path) -> Transcripts {
    Transcripts::under(Some(&dir.join(rust_mutants_sealed::TRANSCRIPTS_LAYOUT)))
}

/// One transcript of one real invocation, and the digest of everything that invocation was a function of.
fn established() -> (Transcript, SealedDigest) {
    let module = command(&[], "", "(call $emit (i32.const 3) (i32.const 0))");
    let invocation = invocation();
    let host = runner();
    let prepared = host.prepare(&module).expect("the command is valid");
    let digest = invocation.digest(prepared.digest(), prepared.configuration());
    let transcript = prepared
        .invoke(&invocation, &crate::common::uninterrupted())
        .expect("the invocation is an answer about the guest");
    (transcript, digest)
}

#[test]
fn what_one_invocation_established_is_answered_from_again_under_its_digest() {
    let dir = tempfile::tempdir().expect("a cache directory");
    let store = store(dir.path());
    let (remembered, digest) = established();
    store.remember(&digest, &remembered);
    assert_eq!(
        store.recall(&digest),
        Some(remembered),
        "a record that reads back as itself answers the invocation it names"
    );
}

#[test]
fn a_record_of_another_invocation_or_a_corrupt_one_answers_nothing() {
    let dir = tempfile::tempdir().expect("a cache directory");
    let store = store(dir.path());
    let (remembered, digest) = established();
    store.remember(&digest, &remembered);
    assert_eq!(
        store.recall(&SealedDigest::of(b"another invocation")),
        None,
        "a record answers only the invocation its digest names"
    );
    let record = dir
        .path()
        .join(rust_mutants_sealed::TRANSCRIPTS_LAYOUT)
        .join(format!("{digest}.json"));
    let read = std::fs::read_to_string(&record).expect("the record");
    let broken = read.replace(
        &format!("\"fuel_spent\":{}", remembered.fuel_spent()),
        "\"fuel_spent\":0",
    );
    assert_ne!(read, broken, "the corruption changes the record");
    std::fs::write(&record, broken).expect("the corrupted record");
    assert_eq!(
        store.recall(&digest),
        None,
        "a record whose bytes are not what its digests say they are answers nothing"
    );
}

#[test]
fn a_store_of_nowhere_neither_remembers_nor_answers() {
    let store = Transcripts::under(None);
    let (remembered, digest) = established();
    store.remember(&digest, &remembered);
    assert_eq!(
        store.recall(&digest),
        None,
        "a run asked to establish everything afresh writes nothing and answers nothing"
    );
}

#[test]
fn a_record_with_unknown_or_repeated_fields_answers_nothing() {
    let dir = tempfile::tempdir().expect("a cache directory");
    let store = store(dir.path());
    let (remembered, digest) = established();
    store.remember(&digest, &remembered);
    let path = dir
        .path()
        .join(rust_mutants_sealed::TRANSCRIPTS_LAYOUT)
        .join(format!("{digest}.json"));
    let raw = std::fs::read_to_string(&path).expect("the record");
    let repeated = format!("\"fuel_spent\":{},", remembered.fuel_spent());
    for extra in ["\"extension\":0,", repeated.as_str()] {
        let edited = raw.replacen("\"transcript\":{", &format!("\"transcript\":{{{extra}"), 1);
        assert_ne!(raw, edited, "the record's shape changed");
        std::fs::write(&path, edited).expect("the foreign record");
        assert_eq!(
            store.recall(&digest),
            None,
            "a record with unknown or repeated fields is a different document, even with unchanged digests"
        );
    }
}

#[test]
fn the_counters_say_what_the_host_spent_only_where_a_bench_assembled() {
    use rust_mutants_sealed::Counted;
    let uncounted = Counted::default();
    assert_eq!(
        uncounted.spent(),
        None,
        "a run that assembled no bench says nothing, rather than zeros it never spent"
    );
    let counted = Counted::default();
    counted.assembled();
    counted.compiled().expect("the count fits");
    counted.compiled().expect("the count fits");
    counted.instantiated().expect("the count fits");
    counted.answered().expect("the count fits");
    counted.answered().expect("the count fits");
    counted.answered().expect("the count fits");
    assert_eq!(
        counted.spent(),
        Some(rust_mutants_sealed::Spent {
            compiles: 2,
            instances: 1,
            answered: 3,
            compilation: None,
            execution_ns: None,
        }),
        "the counts are what the host spent, shared by every bench of one run"
    );
}

/// Compiled code is reusable only for the same module bytes and compiler configuration.
#[test]
fn the_compiled_module_cache_distinguishes_bytes_and_compiler_configuration() {
    use rust_mutants_sealed::{CompilerTier, SealedRunner};
    let directory = tempfile::tempdir().expect("a shared compiled cache");
    let bytes = command(&[], "", "(call $emit (i32.const 3) (i32.const 0))");
    let changed = command(&[], "", "(call $emit (i32.const 4) (i32.const 0))");
    for (tier, module, hit) in [
        (CompilerTier::Optimized, &bytes, false),
        (CompilerTier::Optimized, &bytes, true),
        (CompilerTier::Optimized, &changed, false),
        (CompilerTier::Unoptimized, &bytes, false),
        (CompilerTier::Unoptimized, &bytes, true),
    ] {
        let runner = SealedRunner::with_compiler(
            std::time::Duration::from_secs(120),
            tier,
            Some(directory.path()),
        )
        .expect("a configured cache");
        runner.prepare(module).expect("a valid module");
        let spent = runner.spent().expect("the prepared module is counted");
        let compilation = spent.compilation.expect("cache work is measured");
        assert_eq!(spent.compiles, 1);
        assert_eq!(compilation.hits, u64::from(hit));
        assert_eq!(compilation.misses, u64::from(!hit));
    }
}
