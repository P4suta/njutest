// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Planted examples a gate must find before its silence about the real tree is believed.

use std::path::Path;

/// One planted example, made of the files it needs by the path each would have in a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// One file, read by the per-file scan alone.
    Source {
        /// What a refusal calls it.
        name: String,
        /// Its repository-relative path.
        path: String,
        /// Its text.
        text: String,
    },
    /// Files laid over a synthetic repository and read by the whole gate.
    Tree {
        /// What a refusal calls it.
        name: String,
        /// Each file as its repository-relative path and its text.
        files: Vec<(String, String)>,
    },
}

impl Shape {
    /// What a refusal calls it.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Source { name, .. } | Self::Tree { name, .. } => name,
        }
    }
}

/// Planted text that does not say what its examples are.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlantedError {
    /// Text before the first header.
    #[error("the planted text begins with `{line}` rather than a `=== ` header")]
    Headless {
        /// The first line.
        line: String,
    },
    /// A header that is neither form.
    #[error(
        "`{header}` is neither `=== <name> source <path>` nor `=== <name> tree`, so it plants nothing a gate could be held to"
    )]
    Header {
        /// The header as written.
        header: String,
    },
    /// A tree example whose text comes before any file line.
    #[error("shape `{name}` is a tree and has text before its first `--- <path>` line")]
    Unplaced {
        /// The example.
        name: String,
    },
    /// A one-file example that names a second file.
    #[error("shape `{name}` is one source file and carries a `--- ` line; make it a tree")]
    SecondFile {
        /// The example.
        name: String,
    },
    /// An example with no file in it.
    #[error("shape `{name}` holds no file")]
    Empty {
        /// The example.
        name: String,
    },
    /// Text holding no example at all.
    #[error("the planted text holds no shape")]
    Nothing,
}

impl crate::error::Coded for PlantedError {
    fn code(&self) -> crate::error::ErrorCode {
        crate::error::SENTINEL_PLANTED
    }
}

const HEADER: &str = "=== ";
const FILE: &str = "--- ";

/// Every example `planted` holds, in the order it holds them.
///
/// # Errors
/// [`PlantedError`] when the text does not say where each example begins and which files it is.
pub fn shapes(planted: &str) -> Result<Vec<Shape>, PlantedError> {
    let mut shapes: Vec<Shape> = Vec::new();
    for line in planted.split_inclusive('\n') {
        if let Some(header) = line.strip_prefix(HEADER) {
            shapes.push(opened(header.trim_end())?);
            continue;
        }
        let Some(shape) = shapes.last_mut() else {
            return Err(PlantedError::Headless {
                line: line.trim_end().to_owned(),
            });
        };
        match (shape, line.strip_prefix(FILE)) {
            (Shape::Source { name, .. }, Some(_)) => {
                return Err(PlantedError::SecondFile { name: name.clone() });
            }
            (Shape::Source { text, .. }, None) => text.push_str(line),
            (Shape::Tree { files, .. }, Some(path)) => {
                files.push((path.trim_end().to_owned(), String::new()));
            }
            (Shape::Tree { name, files }, None) => {
                let Some((_, text)) = files.last_mut() else {
                    return Err(PlantedError::Unplaced { name: name.clone() });
                };
                text.push_str(line);
            }
        }
    }
    if let Some(empty) = shapes
        .iter()
        .find(|shape| matches!(shape, Shape::Tree { files, .. } if files.is_empty()))
    {
        return Err(PlantedError::Empty {
            name: empty.name().to_owned(),
        });
    }
    if shapes.is_empty() {
        return Err(PlantedError::Nothing);
    }
    Ok(shapes)
}

fn opened(header: &str) -> Result<Shape, PlantedError> {
    let refused = || PlantedError::Header {
        header: format!("{HEADER}{header}"),
    };
    let mut words = header.split(' ');
    let (Some(name), Some(scope)) = (words.next(), words.next()) else {
        return Err(refused());
    };
    match (scope, words.next(), words.next()) {
        ("source", Some(path), None) => Ok(Shape::Source {
            name: name.to_owned(),
            path: path.to_owned(),
            text: String::new(),
        }),
        ("tree", None, None) => Ok(Shape::Tree {
            name: name.to_owned(),
            files: Vec::new(),
        }),
        _ => Err(refused()),
    }
}

/// Writes the smallest repository every gate of this workspace reads without refusing it.
///
/// # Errors
/// A directory or file that could not be written.
pub fn skeleton(root: &Path) -> std::io::Result<()> {
    for directory in [
        "compiler-surfaces",
        "crates/app/src",
        "crates/njutest-macros/src",
        "xtask",
        "fuzz/src",
    ] {
        std::fs::create_dir_all(root.join(directory))?;
    }
    for (path, text) in SKELETON {
        std::fs::write(root.join(path), text)?;
    }
    Ok(())
}

const SKELETON: [(&str, &str); 14] = [
    (
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/app\", \"crates/njutest-macros\"]\nresolver = \"3\"\n",
    ),
    (
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
         \n[[package]]\nname = \"njutest-macros\"\nversion = \"0.0.0\"\n",
    ),
    (
        "crates/app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    ),
    ("crates/app/src/lib.rs", ""),
    (
        "crates/njutest-macros/Cargo.toml",
        "[package]\nname = \"njutest-macros\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\nproc-macro = true\n",
    ),
    (
        "crates/njutest-macros/src/lib.rs",
        "use proc_macro::TokenStream;\n#[proc_macro_derive(AllVariants)]\npub fn all_variants(input: TokenStream) -> TokenStream { input }\n",
    ),
    ("xtask/empty.rs", ""),
    ("xtask/wildcard_allowlist.txt", ""),
    ("xtask/waiver_ceiling.txt", "0\n"),
    (
        "xtask/proc_macro_inventory.txt",
        "root njutest-macros 0.0.0 path\n",
    ),
    ("compiler-surfaces/empty.rs", ""),
    (
        "fuzz/Cargo.toml",
        "[package]\nname = \"fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n",
    ),
    ("fuzz/src/lib.rs", ""),
    (
        "fuzz/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"fuzz\"\nversion = \"0.0.0\"\n",
    ),
];

/// Lays `files` over a fresh [`skeleton`] at `root`, creating the directories they name.
///
/// # Errors
/// A directory or file that could not be written.
pub fn plant(root: &Path, files: &[(String, String)]) -> std::io::Result<()> {
    skeleton(root)?;
    for (path, text) in files {
        let at = root.join(path);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(at, text)?;
    }
    Ok(())
}
