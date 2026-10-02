// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// A shell producer owns its FIFO before checking the sticky release, so publications before and during its blocking read are both retained until its supervised lifetime ends.
const UNTIL_GO: &str = "turn=\"$TURNS/go.$$\"; mkfifo -m 600 \"$turn\" || exit 1; exec 3<> \"$turn\" || exit 1; if [ ! -e \"$TURNS/go\" ]; then IFS= read -r released <&3 || exit 1; fi; exec 3>&-; rm -f \"$turn\"";

/// Publishes the sticky release before waking every already registered producer-owned FIFO.
///
/// # Errors
/// The complete endpoint inventory, a FIFO identity or its nonblocking publication is unreadable.
fn release_turns(turns: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::FileTypeExt as _;

    std::fs::write(turns.join("go"), "released\n")?;
    for entry in std::fs::read_dir(turns)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| std::io::Error::other("a turn endpoint has no UTF-8 identity"))?;
        if !name.starts_with("go.") {
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(source),
        };
        if !metadata.file_type().is_fifo() {
            return Err(std::io::Error::other(format!(
                "{} is not a producer-owned FIFO",
                path.display()
            )));
        }
        let descriptor = match rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(source) => return Err(source.into()),
        };
        let identity = rustix::fs::fstat(&descriptor)?;
        if rustix::fs::FileType::from_raw_mode(identity.st_mode) != rustix::fs::FileType::Fifo {
            return Err(std::io::Error::other(
                "the registered FIFO identity changed",
            ));
        }
        let mut endpoint = std::fs::File::from(descriptor);
        std::io::Write::write_all(&mut endpoint, b"released\n")?;
    }
    Ok(())
}
