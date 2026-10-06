// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::borrow::ToOwned as _;
use std::collections::BTreeMap;
use std::format;
use std::string::String;

use super::{Escape, escape};

const HOME: &str = "/scratch/7/home";

const EVERY_NAME_A_HOME_IS_FOUND_BY: [(&str, &str); 9] = [
    ("HOME", ""),
    ("XDG_CONFIG_HOME", ".config"),
    ("XDG_CACHE_HOME", ".cache"),
    ("XDG_STATE_HOME", ".local/state"),
    ("XDG_DATA_HOME", ".local/share"),
    ("XDG_RUNTIME_DIR", ".local/run"),
    ("USERPROFILE", ""),
    ("APPDATA", "AppData/Roaming"),
    ("LOCALAPPDATA", "AppData/Local"),
];

fn placed(under: &str) -> String {
    if under.is_empty() {
        HOME.to_owned()
    } else {
        format!("{HOME}/{under}")
    }
}

fn confined() -> BTreeMap<&'static str, String> {
    EVERY_NAME_A_HOME_IS_FOUND_BY
        .into_iter()
        .map(|(name, under)| (name, placed(under)))
        .collect()
}

fn escaped(environment: &BTreeMap<&'static str, String>) -> Option<Escape<String>> {
    escape(|name| environment.get(name).cloned(), placed)
}

#[test]
fn an_environment_naming_the_home_under_every_name_confines_the_process() {
    assert_eq!(escaped(&confined()), None);
}

#[test]
fn a_home_that_escaped_its_execution_under_any_one_name_is_named() {
    for (name, under) in EVERY_NAME_A_HOME_IS_FOUND_BY {
        let mut dropped = confined();
        dropped.remove(name);
        assert_eq!(
            escaped(&dropped),
            Some(Escape::Escaped {
                name,
                under,
                found: None
            }),
            "{name} dropped leaves the process the home the run was given"
        );
        let mut elsewhere = confined();
        elsewhere.insert(name, "/home/somebody".to_owned());
        assert_eq!(
            escaped(&elsewhere),
            Some(Escape::Escaped {
                name,
                under,
                found: Some("/home/somebody".to_owned())
            }),
            "{name} put back names the home the run was given"
        );
    }
}

#[test]
fn the_first_name_that_escaped_is_the_one_named() {
    let mut environment = confined();
    environment.remove("XDG_STATE_HOME");
    environment.remove("LOCALAPPDATA");
    assert_eq!(
        escaped(&environment),
        Some(Escape::Escaped {
            name: "XDG_STATE_HOME",
            under: ".local/state",
            found: None
        })
    );
}

#[test]
fn the_given_global_git_configuration_named_again_is_an_escape() {
    let mut environment = confined();
    environment.insert("GIT_CONFIG_GLOBAL", "/home/somebody/.gitconfig".to_owned());
    assert_eq!(
        escaped(&environment),
        Some(Escape::GitGlobal {
            found: "/home/somebody/.gitconfig".to_owned()
        }),
        "git config --global would write the given home's configuration"
    );
}
