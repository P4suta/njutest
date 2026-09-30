// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where one execution's directories lie, and the environment that confines a test process to them (ADR 0044).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use rust_mutants_decision::confinement::{CONFINED_HOME, GIT_GLOBAL_CONFIG};

/// The home the run was given, as the platform names it.
pub const GIVEN_HOME: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

/// The homes a build needs where they are, each with the directory it defaults to under the home the run was given.
pub const PINNED_HOMES: [(&str, &str); 2] = [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")];

/// The variables a process finds its temporary directory by, on every platform.
pub const TEMPORARY: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];

/// Which home a test process is given (ADR 0044).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Home {
    /// A home of the execution's own, beside its temporary directory, which the scratch's emptying takes with it.
    Confined,
    /// The home the run was given, for a target whose tests pass only with it.
    Given,
}

/// The directories one execution is given, none inside another: the temporary directory its process sees, the engine's own files about it, and a home of its own where its home is confined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    tmp: PathBuf,
    engine: PathBuf,
    home: Option<PathBuf>,
}

impl Layout {
    /// The layout under `own`, a directory one execution alone holds.
    #[must_use]
    pub fn under(own: &Path, home: Home) -> Self {
        Self {
            tmp: own.join("tmp"),
            engine: own.join("engine"),
            home: match home {
                Home::Confined => Some(own.join("home")),
                Home::Given => None,
            },
        }
    }

    /// This layout, keeping the engine's own files in `engine` instead.
    #[must_use]
    pub fn with_engine(self, engine: PathBuf) -> Self {
        Self { engine, ..self }
    }

    /// The temporary directory the process sees.
    #[must_use]
    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    /// Where the engine keeps its own files about the process.
    #[must_use]
    pub fn engine(&self) -> &Path {
        &self.engine
    }

    /// The home the process is given, where it is its own.
    #[must_use]
    pub fn home_directory(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// Which home the process is given.
    #[must_use]
    pub const fn home(&self) -> Home {
        match self.home {
            Some(_) => Home::Confined,
            None => Home::Given,
        }
    }
}

/// One change to the environment a process is started with, in the order it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Gives the variable this value, in place of every spelling of its name.
    Set {
        /// The variable.
        name: &'static str,
        /// Its value.
        value: OsString,
    },
    /// Takes the variable out, however its name is spelled.
    Remove {
        /// The variable.
        name: &'static str,
    },
}

/// The changes that point a process's temporary directories at `tmp`.
#[must_use]
pub fn temporary(tmp: &Path) -> Vec<Change> {
    TEMPORARY
        .into_iter()
        .map(|name| Change::Set {
            name,
            value: tmp.as_os_str().to_owned(),
        })
        .collect()
}

/// The changes that give a process `home`: the homes a build needs pinned where the home the run was given, `given`, keeps them unless the environment already names them, which `holds` answers, nothing left that names the given home's git identity, every name a home is found by, and where the home has a drive, as `drive` holds it, the two names Windows spells a home by.
#[must_use]
pub fn confining<'a>(
    home: &Path,
    given: Option<&Path>,
    holds: impl Fn(&str) -> bool,
    drive: Option<(&OsStr, impl Iterator<Item = Component<'a>>)>,
) -> Vec<Change> {
    let mut changes = Vec::new();
    for (name, beside) in PINNED_HOMES {
        if holds(name) {
            continue;
        }
        if let Some(given) = given {
            changes.push(Change::Set {
                name,
                value: given.join(beside).into_os_string(),
            });
        }
    }
    changes.push(Change::Remove {
        name: GIT_GLOBAL_CONFIG,
    });
    for (name, under) in CONFINED_HOME {
        changes.push(Change::Set {
            name,
            value: home.join(under).into_os_string(),
        });
    }
    if let Some((prefix, rest)) = drive {
        changes.extend(drive_and_rest(prefix, rest, std::path::MAIN_SEPARATOR_STR));
    }
    changes
}

/// The two names Windows spells a home by: `HOMEDRIVE`, its drive `prefix`, and `HOMEPATH`, `separator` and the path of what `rest` holds after the drive.
#[must_use]
pub fn drive_and_rest<'a>(
    prefix: &OsStr,
    rest: impl Iterator<Item = Component<'a>>,
    separator: &str,
) -> [Change; 2] {
    let parts: PathBuf = rest.collect();
    let mut under = OsString::from(separator);
    under.push(parts.as_os_str());
    [
        Change::Set {
            name: "HOMEDRIVE",
            value: prefix.to_owned(),
        },
        Change::Set {
            name: "HOMEPATH",
            value: under,
        },
    ]
}

/// The files git reads a user's identity from, each with where a confined home keeps it: a global configuration the environment names in place of `~/.gitconfig`, or the one under the home the run was given, and `git/config` under the configuration directory it names, or under the given home's `.config`; a name set to nothing names nothing.
#[must_use]
pub fn identity(
    given: Option<&Path>,
    global: Option<&OsStr>,
    configuration: Option<&OsStr>,
) -> Vec<(PathBuf, &'static str)> {
    let named = |value: Option<&OsStr>| value.filter(|value| !value.is_empty()).map(PathBuf::from);
    let mut found = Vec::new();
    match (named(global), given) {
        (Some(global), _) => found.push((global, ".gitconfig")),
        (None, Some(given)) => found.push((given.join(".gitconfig"), ".gitconfig")),
        (None, None) => {}
    }
    let configuration = named(configuration).or_else(|| given.map(|given| given.join(".config")));
    if let Some(configuration) = configuration {
        found.push((
            configuration.join("git").join("config"),
            ".config/git/config",
        ));
    }
    found
}

/// Every directory the path `under` names below a home, outermost first, each written with a trailing `/`.
#[must_use]
pub fn directories(under: &str) -> Vec<String> {
    let mut directory = String::new();
    let mut found = Vec::new();
    for part in under.split('/').filter(|part| !part.is_empty()) {
        directory.push_str(part);
        directory.push('/');
        found.push(directory.clone());
    }
    found
}

/// Every directory that holds the file at `under` below a home, outermost first, each written with a trailing `/`.
#[must_use]
pub fn holding(under: &str) -> Vec<String> {
    match under.rsplit_once('/') {
        Some((parent, _file)) => directories(parent),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
