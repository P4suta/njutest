// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether an environment confines a test process to its execution's home: every name a home is found by names its place there, and no global git configuration of the home the run was given is named (ADR 0044).

/// The variables a confined home replaces, each as a path under the execution's home, in the order they are set.
pub const CONFINED_HOME: [(&str, &str); 9] = [
    ("HOME", ""),
    ("XDG_CONFIG_HOME", ".config"),
    ("XDG_CACHE_HOME", ".cache"),
    ("XDG_STATE_HOME", ".local/state"),
    ("XDG_DATA_HOME", ".local/share"),
    ("XDG_RUNTIME_DIR", RUNTIME_UNDER_HOME),
    ("USERPROFILE", ""),
    ("APPDATA", "AppData/Roaming"),
    ("LOCALAPPDATA", "AppData/Local"),
];

/// Where a confined home keeps the runtime directory, which only its owner may enter.
pub const RUNTIME_UNDER_HOME: &str = ".local/run";

/// The variable naming the file git reads a user's global configuration from in place of `~/.gitconfig`.
pub const GIT_GLOBAL_CONFIG: &str = "GIT_CONFIG_GLOBAL";

/// How an environment lets a process out of its execution's home, with what it names instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Escape<V> {
    /// A variable the engine confines names somewhere other than its place under the home, or nothing.
    Escaped {
        /// The variable.
        name: &'static str,
        /// Its place under the home.
        under: &'static str,
        /// What it names instead.
        found: Option<V>,
    },
    /// The global git configuration is still named, so `git config --global` would write the given home's.
    GitGlobal {
        /// What it names.
        found: V,
    },
}

/// The first way the environment `named` reads lets a process out of the home whose place for each confined variable `placed` gives, in the order the variables are set, or nothing where it confines the process.
#[must_use]
pub fn escape<V: PartialEq>(
    named: impl Fn(&'static str) -> Option<V>,
    placed: impl Fn(&'static str) -> V,
) -> Option<Escape<V>> {
    for (name, under) in CONFINED_HOME {
        let found = named(name);
        if found.as_ref() != Some(&placed(under)) {
            return Some(Escape::Escaped { name, under, found });
        }
    }
    named(GIT_GLOBAL_CONFIG).map(|found| Escape::GitGlobal { found })
}

#[cfg(test)]
mod tests;
