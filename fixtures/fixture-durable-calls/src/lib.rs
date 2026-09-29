// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A record kept on disk by each kind of call that writes, so a crash is put after every one of them.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Keeps `text` at `path` by writing it beside, copying it into place, and removing what was beside.
pub fn save_by_copy(path: &Path, text: &str) {
    let staged = path.with_extension("staged");
    std::fs::write(&staged, text).unwrap();
    std::fs::copy(&staged, path).unwrap();
    std::fs::remove_file(&staged).unwrap();
}

/// Keeps `text` at `path` in a file made afresh, synced once its bytes are written.
pub fn save_synced(path: &Path, text: &str) {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    file.sync_data().unwrap();
    file.sync_all().unwrap();
}

/// Keeps `text` at `path` by cutting the file back to nothing and writing it again.
pub fn save_cut(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    file.set_len(0).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

/// Keeps `text` at `path` through a buffer, which holds the bytes until it is flushed.
pub fn save_buffered(path: &Path, text: &str) {
    let mut writer = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    writer.write_all(text.as_bytes()).unwrap();
    writer.flush().unwrap();
}

/// Keeps `text` at `path` while a guard is alive, which says beside it that it was dropped when it is.
pub fn save_guarded(path: &Path, text: &str) {
    let _guard = Guard(path.with_extension("dropped"));
    std::fs::write(path, text).unwrap();
}

/// Says, when it is dropped, that it was, in the file it names.
struct Guard(PathBuf);

impl Drop for Guard {
    fn drop(&mut self) {
        std::fs::write(&self.0, b"").unwrap();
    }
}

/// Keeps `text` at `path` in one write, for the process a test starts.
pub fn save_from_a_child(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}
