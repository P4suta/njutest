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

/// One process's `/proc/<pid>/stat` line, which only [`Stat::read`] reads, so no reader can take the line of a process being released for a running one.
///
/// # Errors
/// `/proc` refused the open or the read for a reason other than the process having been reaped.
fn stat(pid: u32) -> io::Result<Asked<String>> {
    match open_stat(pid)? {
        Asked::Answered(opened) => read_stat(opened),
        Asked::Gone => Ok(Asked::Gone),
    }
}

/// What one process's `/proc/<pid>/stat` line says of it, which `/proc` answers for a process of any user and whether or not it may be inspected.
///
/// # Errors
/// `/proc` refused the open or the read for a reason other than the process having been reaped, or wrote a line [`Stat::read`] refuses.
pub fn parsed(pid: u32) -> io::Result<Asked<Stat>> {
    match stat(pid)? {
        Asked::Answered(line) => Stat::read(&line),
        Asked::Gone => Ok(Asked::Gone),
    }
}

/// What a `/proc/<pid>/stat` line says of its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    /// The name the kernel keeps for its command, cut to fifteen bytes.
    pub name: String,
    /// Whether it has ended and waits for its parent to reap it.
    pub ended: bool,
    /// Its parent's id, or zero where it has none this process can see.
    pub parent: u32,
    /// Its process group and its session.
    pub scope: rust_mutants_decision::descent::Scope,
    /// When it started, in clock ticks since boot.
    pub born: u64,
}

impl Stat {
    /// Reads a `/proc/<pid>/stat` line, whose command name may itself hold spaces and parentheses: gone where it is the line of a process being released, dead or a zombie its reaper is collecting, which names no group or session any more.
    ///
    /// # Errors
    /// The line has no command name or lacks a field it always holds.
    pub fn read(line: &str) -> io::Result<Asked<Self>> {
        let unnamed = || {
            io::Error::other(format!(
                "a process status line without its command name: {line:?}"
            ))
        };
        let (name, fields) = line
            .split_once(" (")
            .ok_or_else(unnamed)?
            .1
            .rsplit_once(')')
            .ok_or_else(unnamed)?;
        let fields: Vec<&str> = fields.split_whitespace().collect();
        let field = |index: usize, what: &str| {
            fields.get(index).copied().ok_or_else(|| {
                io::Error::other(format!(
                    "a process status line without its {what}: {line:?}"
                ))
            })
        };
        let number = |index: usize, what: &str| {
            field(index, what)?
                .parse::<u32>()
                .map_err(|source| io::Error::other(format!("the {what} of {line:?}: {source}")))
        };
        let state = field(0, "state")?;
        let released =
            |index: usize, what: &str| Ok::<bool, io::Error>(field(index, what)?.starts_with('-'));
        if state == "X" || released(2, "process group")? || released(3, "session")? {
            return Ok(Asked::Gone);
        }
        Ok(Asked::Answered(Self {
            name: name.to_owned(),
            ended: state == "Z",
            parent: number(1, "parent")?,
            scope: rust_mutants_decision::descent::Scope {
                group: number(2, "process group")?,
                session: number(3, "session")?,
            },
            born: field(19, "start time")?.parse::<u64>().map_err(|source| {
                io::Error::other(format!("the start time of {line:?}: {source}"))
            })?,
        }))
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
    fn a_command_name_holding_spaces_and_parentheses_is_read_whole() {
        let line = "4242 (a (b) c) S 1 4200 4100 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10 18446744073709551615";
        assert_eq!(
            super::Stat::read(line).expect("a complete status line"),
            Asked::Answered(super::Stat {
                name: "a (b) c".to_owned(),
                ended: false,
                parent: 1,
                scope: rust_mutants_decision::descent::Scope {
                    group: 4200,
                    session: 4100,
                },
                born: 987_654,
            })
        );
    }

    #[test]
    fn the_line_of_a_zombie_its_reaper_is_releasing_is_gone() {
        let releasing = "161317 (rustc) Z 0 -1 -1 0 -1 4227084 6019 0 0 0 6 0 0 0 20 0 0 0 47103589 0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 17 4 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(
            super::Stat::read(releasing).expect("a line /proc wrote"),
            Asked::Gone,
            "a zombie its parent is reaping names no group or session either, as a group member \
             settlement met one in the Linux coverage run: 'invalid digit found in string'"
        );
    }

    #[test]
    fn the_line_of_a_process_the_kernel_is_releasing_is_gone() {
        let releasing = "3902405 (rust_mutants_de) X 0 -1 -1 0 -1 4227084 516 0 0 0 0 0 0 0 20 0 0 0 46335335 0 0 0 0 0 0 0 0 0 0 0 0 1 0 0 17 11 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(
            super::Stat::read(releasing).expect("a line /proc wrote"),
            Asked::Gone,
            "a process reaped while its line was read names no parent, group or session, as the \
             census met one on Linux"
        );
    }

    #[test]
    fn a_status_line_that_stops_short_is_refused() {
        for line in [
            "4242 a S 1 4200 4100",
            "4242 (a) S 1 4200 4100 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0",
            "4242 (a) Z -1 4200 4100 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654",
        ] {
            assert!(
                super::Stat::read(line).is_err(),
                "a line without every field this reader takes from it is refused: {line:?}"
            );
        }
    }

    #[test]
    fn an_ended_child_says_so_and_names_this_process_and_its_own_group() {
        let (mut ended, pid) = exited();
        let me = std::process::id();
        let session = rustix::process::getsid(None).expect("this process's session");
        match super::parsed(pid).expect("an ended child keeps its stat") {
            Asked::Answered(stat) => {
                assert!(stat.ended, "the child has ended: {stat:?}");
                assert_eq!(stat.parent, me, "{stat:?}");
                assert_eq!(
                    stat.scope.group, pid,
                    "a group leader leads its own: {stat:?}"
                );
                assert_eq!(
                    i32::try_from(stat.scope.session).expect("a session id"),
                    session.as_raw_nonzero().get(),
                    "{stat:?}"
                );
            }
            Asked::Gone => panic!("a process not yet reaped is still listed"),
        }
        ended.wait().expect("the leader is reaped");
        assert_eq!(super::parsed(pid).expect("a reaped stat"), Asked::Gone);
    }

    #[test]
    fn this_process_started_no_earlier_than_its_parent() {
        let born = |pid: u32| match super::parsed(pid).expect("a running process's stat") {
            Asked::Answered(stat) => stat.born,
            Asked::Gone => panic!("process {pid} runs"),
        };
        let own = born(std::process::id());
        let parent = born(std::os::unix::process::parent_id());
        assert!(
            parent <= own,
            "the parent started at {parent}, after this process at {own}"
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
