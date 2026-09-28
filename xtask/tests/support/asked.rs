// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// The questions a reading puts to git and cargo, each answered by git and cargo themselves and recorded.
mod asked {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use xtask::gates::tree::{Ask, Depth, Processes};
    use xtask::repository::ListingError;

    /// Every question a reading put, in the order it put them.
    #[derive(Debug)]
    pub(super) struct Asked {
        questions: Mutex<Vec<Question>>,
    }

    /// One question a reading put, by the canonical path it named where that path could be resolved.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum Question {
        /// What git lists of the repository at this root.
        Listed(PathBuf),
        /// What cargo reads of the graph at this manifest, to this depth.
        Read(PathBuf, Depth),
    }

    impl Asked {
        /// Nothing asked yet.
        pub(super) const fn new() -> Self {
            Self {
                questions: Mutex::new(Vec::new()),
            }
        }

        /// Every question put so far, in the order it was put.
        #[expect(
            clippy::expect_used,
            reason = "a question recorded while another panicked leaves no count to report"
        )]
        pub(super) fn questions(&self) -> Vec<Question> {
            self.questions
                .lock()
                .expect("no question panicked while it was recorded")
                .clone()
        }

        #[expect(
            clippy::expect_used,
            reason = "a question recorded while another panicked leaves no count to report"
        )]
        fn record(&self, question: Question) {
            self.questions
                .lock()
                .expect("no question panicked while it was recorded")
                .push(question);
        }
    }

    /// The canonical form of `path`, or `path` as it was named where it cannot be resolved.
    pub(super) fn canonical(path: &Path) -> PathBuf {
        match std::fs::canonicalize(path) {
            Ok(resolved) => resolved,
            Err(_unresolved) => path.to_path_buf(),
        }
    }

    impl Ask for Asked {
        fn listed(&self, root: &Path) -> Result<Vec<String>, ListingError> {
            self.record(Question::Listed(canonical(root)));
            Processes.listed(root)
        }

        fn read(
            &self,
            manifest: &Path,
            depth: Depth,
        ) -> Result<cargo_metadata::Metadata, cargo_metadata::Error> {
            self.record(Question::Read(canonical(manifest), depth));
            Processes.read(manifest, depth)
        }
    }
}

use asked::{Asked, Question};
