// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Temporary compilation caches disposed after actual producer process completion.

#![expect(
    clippy::expect_used,
    reason = "test setup failures retain their exact ownership cause"
)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::Duration;

use njutest_devkit::process::SupervisedChild;
use rust_mutants_sealed::{CompilationCache, ModuleOwner, SealedRunner};

const CHILD_CACHE: &str = "NJUTEST_SEALED_PROCESS_CACHE";
const TEST: &str = "cache_process::one_cold_preparation_is_shared_across_owned_processes";

fn producer(directory: std::path::PathBuf) {
    let cache = CompilationCache::retained(directory).expect("the parent retains this cache");
    let modules = ModuleOwner::default();
    let runner = SealedRunner::cached(&modules, Duration::from_secs(60), &cache)
        .expect("the producer's runner");
    println!("cache-producer-ready");
    std::io::stdout().flush().expect("readiness is published");
    std::io::stdin()
        .read_exact(&mut [0])
        .expect("the parent's release event");
    let bytes = crate::common::command(&[], "", "(call $emit (i32.const 0) (i32.const 0))");
    let module = runner
        .prepare(&bytes)
        .expect("actual keyed module preparation");
    let transcript = module
        .invoke(
            &crate::common::invocation(),
            &crate::common::uninterrupted(),
        )
        .expect("a fresh actual guest");
    assert_eq!(transcript.stop(), rust_mutants_sealed::SealedStop::Returned);
    println!(
        "cache-preparation {:?} {} {}",
        module.reuse(),
        module.digest(),
        module.configuration()
    );
    println!(
        "cache-work {}",
        serde_json::to_string(&runner.spent()).expect("actual work")
    );
}

#[derive(Debug)]
struct Producer {
    child: SupervisedChild,
    output: std::io::BufReader<std::process::ChildStdout>,
}

impl Producer {
    fn start(directory: &std::path::Path) -> Self {
        let mut command = Command::new(std::env::current_exe().expect("the real test binary"));
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD_CACHE, directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = SupervisedChild::launch(&mut command).expect("the owned producer starts");
        let mut output = std::io::BufReader::new(child.take_stdout().expect("the readiness pipe"));
        loop {
            let mut line = String::new();
            assert!(
                output.read_line(&mut line).expect("readiness is readable") != 0,
                "the producer ended before publishing readiness"
            );
            if line.trim() == "cache-producer-ready" {
                break;
            }
        }
        Self { child, output }
    }

    fn release(&mut self) {
        self.child
            .take_stdin()
            .expect("the release pipe")
            .write_all(b"x")
            .expect("the producer is released by an event");
    }

    fn finish(mut self) -> String {
        assert!(
            self.child
                .wait()
                .expect("actual producer process completion")
                .success()
        );
        let mut output = String::new();
        self.output
            .read_to_string(&mut output)
            .expect("all producer descriptors reached EOF");
        output
    }
}

#[test]
fn one_cold_preparation_is_shared_across_owned_processes() {
    if let Some(directory) = std::env::var_os(CHILD_CACHE) {
        producer(directory.into());
        return;
    }
    let directory = njutest_devkit::temporary::CacheDirectory::make("sealed-process-")
        .expect("the parent owns the temporary cache lifetime");
    let cache = directory.path().join("modules");
    let mut first = Producer::start(&cache);
    let mut second = Producer::start(&cache);
    first.release();
    second.release();
    let output = [first.finish(), second.finish()];
    let mut preparations: Vec<_> = output
        .iter()
        .flat_map(|output| output.lines())
        .filter_map(|line| line.strip_prefix("cache-preparation "))
        .collect();
    preparations.sort_unstable();
    assert_eq!(
        preparations.len(),
        2,
        "both actual processes published their work: {output:?}"
    );
    let cold = preparations
        .first()
        .expect("the cold preparation")
        .strip_prefix("Cold ")
        .expect("exactly one process prepared cold");
    let disk = preparations
        .get(1)
        .expect("the disk preparation")
        .strip_prefix("Disk ")
        .expect("the other process loaded safe Wasmtime cached code");
    assert_eq!(
        cold, disk,
        "both processes bind the same module and configuration"
    );
    drop(directory);
}
