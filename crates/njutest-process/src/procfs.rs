// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `/proc` says about a process by its id, which is [`Asked::Gone`] once that process has been reaped, however far the read had got.

use std::io;
use std::path::PathBuf;

use crate::Asked;

/// Every process id `/proc` lists.
///
/// # Errors
/// `/proc` could not be listed, or listed a name that is not text.
pub fn processes() -> io::Result<Vec<u32>> {
    let mut listed = Vec::new();
    for entry in std::fs::read_dir(TABLE)? {
        let name = entry?.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| io::Error::other("the process table lists a name that is not text"))?;
        if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
            listed.push(name.parse::<u32>().map_err(io::Error::other)?);
        }
    }
    Ok(listed)
}

/// One process's `/proc/<pid>/stat` line.
///
/// # Errors
/// `/proc` refused the open or the read for a reason other than the process having been reaped.
pub fn stat(pid: u32) -> io::Result<Asked<String>> {
    match open_stat(pid)? {
        Asked::Answered(opened) => read_stat(opened),
        Asked::Gone => Ok(Asked::Gone),
    }
}

/// The user that owns the process `pid`, which is the owner of its `/proc` directory.
///
/// # Errors
/// `/proc` refused to say for a reason other than the process having been reaped.
pub fn owner(pid: u32) -> io::Result<Asked<u32>> {
    use std::os::unix::fs::MetadataExt as _;

    Ok(listed(std::fs::metadata(directory(pid)))?.map(|metadata| metadata.uid()))
}

/// The directory the process `pid` works in.
///
/// # Errors
/// `/proc` refused to say for a reason other than the process having been reaped, including `PermissionDenied`, which it answers both for a process this one may not inspect and for one reaped while it was asked.
pub fn working_directory(pid: u32) -> io::Result<Asked<PathBuf>> {
    listed(std::fs::read_link(directory(pid).join("cwd")))
}

/// Where `/proc` lists its processes.
const TABLE: &str = "/proc";

/// The `/proc` directory of the process `pid`.
fn directory(pid: u32) -> PathBuf {
    PathBuf::from(TABLE).join(pid.to_string())
}

/// What one call naming a path under `/proc/<pid>` comes to: gone where `/proc` no longer lists the process, which it says as `ENOENT`, and as [`super::unix::asked_io`] reads any other refusal.
fn listed<T>(answer: io::Result<T>) -> io::Result<Asked<T>> {
    match answer {
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Asked::Gone),
        answer => super::unix::asked_io(answer),
    }
}

/// Opens one process's `/proc/<pid>/stat`.
fn open_stat(pid: u32) -> io::Result<Asked<std::fs::File>> {
    listed(std::fs::File::open(directory(pid).join("stat")))
}

/// Reads an opened `/proc/<pid>/stat`, which `/proc` refuses with `ESRCH` once the process has been reaped since the open.
fn read_stat(opened: std::fs::File) -> io::Result<Asked<String>> {
    super::unix::asked_io(io::read_to_string(opened))
}

#[cfg(test)]
mod tests {
    use crate::Asked;

    fn exited() -> (crate::GroupChild, u32) {
        let ended =
            crate::GroupChild::start(&mut std::process::Command::new("true")).expect("true starts");
        let pid = ended.id().expect("the unreaped leader");
        assert!(
            ended
                .completion()
                .wait(None)
                .expect("the leader's exit event"),
            "the leader has exited and waits to be reaped"
        );
        (ended, pid)
    }

    #[test]
    fn a_process_reaped_between_the_open_and_the_read_of_its_stat_is_gone() {
        let (mut ended, pid) = exited();
        let opened = match super::open_stat(pid).expect("an exited process keeps its stat") {
            Asked::Answered(opened) => opened,
            Asked::Gone => panic!("a process not yet reaped is still listed"),
        };
        ended.wait().expect("the leader is reaped");
        assert_eq!(
            super::read_stat(opened).expect("a process reaped under an open stat has gone"),
            Asked::Gone
        );
    }

    #[test]
    fn a_reaped_process_is_gone_from_every_question_asked_of_it() {
        let (mut ended, pid) = exited();
        ended.wait().expect("the leader is reaped");
        assert_eq!(super::stat(pid).expect("a reaped stat"), Asked::Gone);
        assert_eq!(super::owner(pid).expect("a reaped owner"), Asked::Gone);
        assert_eq!(
            super::working_directory(pid).expect("a reaped directory"),
            Asked::Gone
        );
    }

    #[test]
    fn this_process_answers_every_question_asked_of_it() {
        let own = std::process::id();
        assert!(
            super::processes()
                .expect("the process table")
                .contains(&own),
            "the table lists the process reading it"
        );
        match super::stat(own).expect("this process's stat") {
            Asked::Answered(line) => assert!(line.starts_with(&format!("{own} (")), "{line}"),
            Asked::Gone => panic!("the process reading its own stat has not ended"),
        }
        assert_eq!(
            super::owner(own).expect("this process's owner"),
            Asked::Answered(rustix::process::geteuid().as_raw())
        );
        assert_eq!(
            super::working_directory(own).expect("this process's directory"),
            Asked::Answered(std::env::current_dir().expect("the working directory"))
        );
    }
}
