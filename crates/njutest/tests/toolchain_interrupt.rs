// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a verification leaves behind when the process is asked to stop: the exit code the contract names, and no descendant process.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_devkit::fixture::copy_tree;
use njutest_devkit::process::SupervisedChild;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command, Output, Stdio};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::{Duration, Instant};

fn interrupted_by(signal: rustix::process::Signal, expected: i32) {
    let dir = tempfile::Builder::new()
        .prefix("njutest-interrupt-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let child = verify_in(&root, &[]);
    let pid = child.id().expect("the child is live");

    let child = measuring(child);
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        signal,
    )
    .expect("the signal is delivered");

    let status = child.wait_with_output().status;
    assert_eq!(
        status.code(),
        Some(expected),
        "a run that was asked to stop says so in its exit code rather than in a crash"
    );
    if let Some(stragglers) =
        njutest_devkit::process::in_group(pid).expect("the processes of the run's group")
    {
        assert!(
            stragglers.is_empty(),
            "the run left {stragglers:?} behind in its own process group"
        );
    }
}

#[test]
fn an_interrupted_verification_exits_130_and_leaves_no_process_behind() {
    interrupted_by(rustix::process::Signal::INT, 130);
}

#[test]
fn a_terminated_verification_exits_143_and_leaves_no_process_behind() {
    interrupted_by(rustix::process::Signal::TERM, 143);
}

struct MeasuringChild {
    child: Option<SupervisedChild>,
    reader: Option<njutest_devkit::thread::JoinedThread<std::io::Result<Vec<u8>>>>,
}

impl MeasuringChild {
    fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.as_mut()?.take_stdin()
    }

    fn wait_with_output(mut self) -> Output {
        let child = self.child.take().expect("the producer is still owned");
        let completion = child.completion().expect("the retained child event");
        let began = Instant::now();
        let completed = completion
            .wait(Some(Duration::from_secs(300)))
            .expect("the actual process exit is observable");
        record_wait(began, "owned-child-exit-or-semantic-deadline");
        assert!(completed, "the run did not stop when it was asked to");
        let mut output = child
            .wait_with_output()
            .expect("the complete group settles");
        let began = Instant::now();
        let stderr = self
            .reader
            .take()
            .expect("the stderr reader remains owned")
            .join()
            .expect("the actual stderr reader joins")
            .expect("the actual stderr stream reaches EOF");
        record_wait(began, "owned-stderr-eof-and-reader-join");
        assert!(output.stderr.is_empty(), "stderr has exactly one collector");
        output.stderr = stderr;
        output
    }
}

impl Drop for MeasuringChild {
    fn drop(&mut self) {
        let owned_producer = self.child.take();
        drop(owned_producer);
        let Some(reader) = self.reader.take() else {
            return;
        };
        let began = Instant::now();
        match reader.join() {
            Ok(Ok(bytes)) => eprintln!("{}", njutest_devkit::process::strict_utf8(&bytes)),
            Ok(Err(source)) => eprintln!("the retained stderr reader refused: {source}"),
            Err(source) => {
                eprintln!("the owned stderr reader could not join: {source}");
                std::process::abort();
            }
        }
        record_wait(began, "cleanup-stderr-eof-and-reader-join");
    }
}

fn record_wait(began: Instant, cause: &str) {
    let elapsed_ns = u64::try_from(began.elapsed().as_nanos()).expect("the actual wait width");
    eprintln!(
        "{}",
        serde_json::json!({
            "kind": "host-wait",
            "payload": {
                "owner": "njutest-interrupt-stderr",
                "cause": cause,
                "elapsed_ns": elapsed_ns,
                "machine": {
                    "os": std::env::consts::OS,
                    "cpus": std::thread::available_parallelism().expect("the executing host").get()
                }
            }
        })
    );
}

fn measuring(mut child: SupervisedChild) -> MeasuringChild {
    let stderr = child.take_stderr().expect("stderr is piped");
    let (ready, receiver) = sync_channel(1);
    let owned = MeasuringChild {
        child: Some(child),
        reader: Some(njutest_devkit::thread::JoinedThread::launch(move || {
            collect_measuring(stderr, MeasuringPublication(Some(ready)))
        })),
    };
    let began = Instant::now();
    let observed = receiver.recv_timeout(Duration::from_secs(300));
    record_wait(began, "baseline-line-publication-or-semantic-deadline");
    observed
        .expect("the run never began measuring before its semantic deadline")
        .expect("the actual baseline publication");
    owned
}

struct MeasuringPublication(Option<SyncSender<std::io::Result<()>>>);

impl MeasuringPublication {
    fn send(&mut self, result: std::io::Result<()>) -> std::io::Result<()> {
        if let Some(sender) = self.0.take() {
            sender.send(result).map_err(std::io::Error::other)?;
        }
        Ok(())
    }
}

impl Drop for MeasuringPublication {
    fn drop(&mut self) {
        if let Err(source) = self.send(Err(std::io::Error::other(
            "the owned stderr reader ended before publishing readiness",
        ))) {
            eprintln!("the retained readiness publication refused: {source}");
        }
    }
}

fn collect_measuring(
    stderr: ChildStderr,
    mut publication: MeasuringPublication,
) -> std::io::Result<Vec<u8>> {
    let mut reader = BufReader::new(stderr);
    let mut captured = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                publication.send(Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the run ended before it measured anything",
                )))?;
                return Ok(captured);
            }
            Ok(_read) => {
                captured.extend_from_slice(line.as_bytes());
                if line.contains("== baseline") {
                    publication.send(Ok(()))?;
                }
            }
            Err(source) => {
                publication.send(Err(std::io::Error::new(source.kind(), source.to_string())))?;
                return Err(source);
            }
        }
    }
}

#[test]
fn readiness_keeps_stderr_open_until_the_owned_producer_finishes() {
    use std::io::Write as _;

    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "printf '== baseline\\n' >&2; read released; printf 'still-owned\\n' >&2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = SupervisedChild::launch(&mut command).expect("the actual stderr producer");
    let mut child = measuring(child);
    let mut release = child.take_stdin().expect("the owned producer release");
    release
        .write_all(b"released\n")
        .expect("release the producer");
    drop(release);
    let output = child.wait_with_output();
    assert!(
        output.status.success(),
        "readiness must keep the actual stderr writer alive until completion: {output:?}"
    );
    assert_eq!(output.stderr, b"== baseline\nstill-owned\n");
}

fn verify_in(root: &Path, extra: &[&str]) -> SupervisedChild {
    let mut args = vec!["verify", "--offline", "--locked", "--ui=plain"];
    args.extend_from_slice(extra);
    let mut command = Command::new(env!("CARGO_BIN_EXE_njutest"));
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env(
            "XDG_CACHE_HOME",
            njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
        )
        .envs(njutest_devkit::paths::environment_for_a_toolchain_run(&[]))
        .envs(njutest_devkit::paths::temporary_directory(
            &njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    SupervisedChild::launch(&mut command).expect("njutest starts")
}

#[test]
fn an_interrupted_run_leaves_what_an_earlier_one_established_rather_than_clearing_it() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-resume-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-assured"),
        &root,
    );

    let checkpoints = njutest_devkit::paths::cache_beside(&root)
        .expect("a cache directory")
        .join("njutest/outcomes-v1/checkpoints");
    let seeded = checkpoints.join("an-earlier-run");
    std::fs::create_dir_all(&seeded).expect("mkdir");
    std::fs::write(
        seeded.join("checkpoint-v1.json"),
        serde_json::to_string(&serde_json::json!({
            "schema": "njutest-assurance-checkpoint-v1",
            "identity": "an-earlier-run",
            "attempts": 1,
            "targets": [],
            "mutants": [{
                "id": "0f7b4d7472329894e9b3",
                "disposition": {
                    "kind": "killed",
                    "by": "fixture-assured/lib/fixture_assured",
                    "before": []
                },
                "duration_ms": 1,
            }],
        }))
        .expect("the state renders"),
    )
    .expect("write");

    let interrupted = verify_in(&root, &[]);
    let pid = interrupted.id().expect("the child is live");
    let interrupted = measuring(interrupted);
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(pid.try_into().expect("a pid fits")).expect("a live pid"),
        rustix::process::Signal::INT,
    )
    .expect("the signal is delivered");
    assert_eq!(interrupted.wait_with_output().status.code(), Some(130));

    let states = written_states(&checkpoints);
    assert!(
        !states.is_empty(),
        "a run that stopped is not a run that finished, and clearing what an earlier \
         one established would make an interrupt cost the whole run — which is the \
         opposite of what a checkpoint is for: {states:?}"
    );
    let state: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&states[0]).expect("the state"),
    )
    .expect("the state is a document");
    assert_eq!(state["schema"], "njutest-assurance-checkpoint-v1");
    assert!(
        !state["mutants"].as_array().expect("mutants").is_empty(),
        "the mutants an earlier run judged are still there: {state}"
    );
}

/// Every checkpoint file under `root`.
fn written_states(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.map(|entry| entry.expect("every checkpoint entry is readable")) {
        let path = entry.path().join("checkpoint-v1.json");
        if path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    found
}
