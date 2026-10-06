// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{
    Change, Home, Layout, confining, directories, drive_and_rest, holding, identity, temporary,
};

const SEPARATOR: &str = std::path::MAIN_SEPARATOR_STR;

fn set(name: &'static str, value: &str) -> Change {
    Change::Set {
        name,
        value: OsString::from(value),
    }
}

fn confined_names(home: &str) -> Vec<Change> {
    [
        ("HOME", ""),
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_CACHE_HOME", ".cache"),
        ("XDG_STATE_HOME", ".local/state"),
        ("XDG_DATA_HOME", ".local/share"),
        ("XDG_RUNTIME_DIR", ".local/run"),
        ("USERPROFILE", ""),
        ("APPDATA", "AppData/Roaming"),
        ("LOCALAPPDATA", "AppData/Local"),
    ]
    .into_iter()
    .map(|(name, under)| set(name, &format!("{home}{SEPARATOR}{under}")))
    .collect()
}

fn no_drive() -> Option<(&'static OsStr, std::path::Components<'static>)> {
    None
}

#[test]
fn an_execution_lays_its_directories_side_by_side_under_its_own() {
    let own = Path::new("/scratch/7");
    let confined = Layout::under(own, Home::Confined);
    assert_eq!(confined.tmp(), own.join("tmp"));
    assert_eq!(confined.engine(), own.join("engine"));
    assert_eq!(confined.home_directory(), Some(own.join("home").as_path()));
    assert_eq!(confined.home(), Home::Confined);
    let given = Layout::under(own, Home::Given);
    assert_eq!(
        given.home_directory(),
        None,
        "a given home is not the execution's"
    );
    assert_eq!(given.home(), Home::Given);
    let elsewhere = confined.with_engine(PathBuf::from("/kept/engine"));
    assert_eq!(elsewhere.engine(), Path::new("/kept/engine"));
    assert_eq!(
        elsewhere.tmp(),
        own.join("tmp"),
        "only the engine's files move"
    );
    assert_eq!(elsewhere.home(), Home::Confined);
}

#[test]
fn every_name_a_temporary_directory_is_found_by_names_the_executions() {
    assert_eq!(
        temporary(Path::new("/scratch/7/tmp")),
        vec![
            set("TMPDIR", "/scratch/7/tmp"),
            set("TMP", "/scratch/7/tmp"),
            set("TEMP", "/scratch/7/tmp"),
        ]
    );
}

#[test]
fn a_confined_home_pins_the_build_homes_drops_the_given_identity_and_names_itself_everywhere() {
    let home = Path::new("/scratch/7/home");
    let given = Path::new("/home/somebody");
    let mut expected = vec![
        set("CARGO_HOME", &format!("/home/somebody{SEPARATOR}.cargo")),
        set("RUSTUP_HOME", &format!("/home/somebody{SEPARATOR}.rustup")),
        Change::Remove {
            name: "GIT_CONFIG_GLOBAL",
        },
    ];
    expected.extend(confined_names("/scratch/7/home"));
    assert_eq!(
        confining(home, Some(given), |_| false, no_drive()),
        expected
    );
    let pinned_already = confining(home, Some(given), |name| name == "CARGO_HOME", no_drive());
    assert_eq!(
        pinned_already.first(),
        Some(&set(
            "RUSTUP_HOME",
            &format!("/home/somebody{SEPARATOR}.rustup")
        )),
        "a build home the environment names stays where it is, and the next is still pinned"
    );
    assert_eq!(pinned_already.len(), expected.len() - 1);
    let mut unpinned = vec![Change::Remove {
        name: "GIT_CONFIG_GLOBAL",
    }];
    unpinned.extend(confined_names("/scratch/7/home"));
    assert_eq!(
        confining(home, None, |_| false, no_drive()),
        unpinned,
        "with no given home there is nowhere to pin a build home to"
    );
}

#[test]
fn git_reads_an_identity_where_the_environment_says_or_under_the_given_home() {
    let given = Path::new("/home/somebody");
    assert_eq!(
        identity(Some(given), None, None),
        vec![
            (given.join(".gitconfig"), ".gitconfig"),
            (
                given.join(".config").join("git").join("config"),
                ".config/git/config"
            ),
        ]
    );
    assert_eq!(
        identity(
            Some(given),
            Some(OsStr::new("/etc/identity")),
            Some(OsStr::new("/xdg"))
        ),
        vec![
            (PathBuf::from("/etc/identity"), ".gitconfig"),
            (
                Path::new("/xdg").join("git").join("config"),
                ".config/git/config"
            ),
        ]
    );
    assert_eq!(
        identity(Some(given), Some(OsStr::new("")), Some(OsStr::new(""))),
        identity(Some(given), None, None),
        "a name set to nothing names nothing"
    );
    assert_eq!(
        identity(None, Some(OsStr::new("/etc/identity")), None),
        vec![(PathBuf::from("/etc/identity"), ".gitconfig")]
    );
    assert_eq!(identity(None, None, None), Vec::new());
}

#[test]
fn a_directory_below_a_home_is_every_directory_it_passes_through() {
    assert_eq!(directories(""), Vec::<String>::new());
    assert_eq!(directories(".config"), vec![".config/".to_owned()]);
    assert_eq!(
        directories(".local/state"),
        vec![".local/".to_owned(), ".local/state/".to_owned()]
    );
    assert_eq!(
        directories("a//b/"),
        vec!["a/".to_owned(), "a/b/".to_owned()]
    );
    assert_eq!(holding(".gitconfig"), Vec::<String>::new());
    assert_eq!(
        holding(".config/git/config"),
        vec![".config/".to_owned(), ".config/git/".to_owned()]
    );
}

#[test]
fn a_home_on_a_drive_is_spelled_by_the_drive_and_the_rest_under_it() {
    assert_eq!(
        drive_and_rest(
            OsStr::new("C:"),
            Path::new("users/somebody").components(),
            "\\"
        ),
        [
            set("HOMEDRIVE", "C:"),
            set("HOMEPATH", &format!("\\users{SEPARATOR}somebody")),
        ],
        "Windows spells a home in two variables: the separator given stands first, and the \
         rest's own names keep the path's"
    );
    assert_eq!(
        drive_and_rest(OsStr::new("D:"), Path::new("somebody").components(), "\\"),
        [set("HOMEDRIVE", "D:"), set("HOMEPATH", "\\somebody")],
        "the separator stands even where the rest is one name, so the two variables always \
         name an absolute path together"
    );
}

#[test]
fn a_home_the_confinement_lays_on_a_drive_names_the_drive_in_its_changes() {
    let home = Path::new("/scratch/7/home");
    let mut expected = vec![Change::Remove {
        name: "GIT_CONFIG_GLOBAL",
    }];
    expected.extend(confined_names("/scratch/7/home"));
    expected.extend([
        set("HOMEDRIVE", "scratch:"),
        set("HOMEPATH", &format!("{SEPARATOR}7{SEPARATOR}home")),
    ]);
    let drive = (OsStr::new("scratch:"), Path::new("7/home").components());
    assert_eq!(
        confining(home, None, |_| false, Some(drive)),
        expected,
        "the drive's own changes come last, after the names every home sets, led by the \
         platform's own separator"
    );
}

#[test]
fn a_planted_confinement_that_keeps_the_given_identity_is_caught() {
    let home = Path::new("/scratch/7/home");
    let planted: Vec<Change> = confining(home, None, |_| false, no_drive())
        .into_iter()
        .filter(|change| !matches!(change, Change::Remove { .. }))
        .collect();
    assert_ne!(
        planted,
        confining(home, None, |_| false, no_drive()),
        "the changes did not say that the given global git configuration goes"
    );
}
