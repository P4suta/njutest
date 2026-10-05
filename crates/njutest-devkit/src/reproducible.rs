// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether this machine builds a change made and reverted back to the bytes it built before, and the bytes a build left.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest as _, Sha256};

mod witness;

/// What one build produced, by target name, each executable digested.
type Built = BTreeMap<String, String>;

/// Whether a tree built, changed, and built back comes out as the bytes it came out as, which is not whether a second build of an unchanged tree runs the first one's bytes.
///
/// # Panics
/// When the fixture cannot be copied, which is a setup failure rather than an answer.
#[must_use]
pub fn builds_a_reverted_change_to_the_same_bytes() -> bool {
    match witness::answer() {
        Ok(answer) => answer,
        Err(source) => {
            eprintln!("reproducibility-unbound: {source}");
            actual_reverted_change()
        }
    }
}

fn actual_reverted_change() -> bool {
    let fixture = crate::fixture::Fixture::copy("fixture-equivalent");
    let target = fixture.temp().join("twice");
    let source = fixture.root().join("src/lib.rs");
    let Ok(original) = std::fs::read(&source) else {
        return false;
    };

    let first = built(fixture.root(), &target);
    let mut changed = original.clone();
    changed.extend_from_slice(b"\npub const A_THING_NOTHING_READS: u8 = 7;\n");
    if std::fs::write(&source, &changed).is_err() {
        return false;
    }
    let between = built(fixture.root(), &target);
    if between.is_empty() {
        return false;
    }
    if std::fs::write(&source, &original).is_err() {
        return false;
    }
    let again = built(fixture.root(), &target);

    !first.is_empty() && first == again
}

/// The SHA-256 of the file at `path`, in hex: what a test compares to know two runs ran the same program.
///
/// # Panics
/// When the file cannot be read, which is a setup failure rather than an answer.
#[must_use]
#[track_caller]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot read the program it ran has nothing to compare, and saying so is its only honest answer"
)]
pub fn digest(path: &Path) -> String {
    hex::encode(Sha256::digest(
        std::fs::read(path).expect("the file a test digests is readable"),
    ))
}

/// One build of the tree at `root`, or nothing at all when it did not build.
fn built(root: &Path, target: &Path) -> Built {
    let mut command = std::process::Command::new(crate::paths::cargo_binary());
    configure_build_command(&mut command, root, target);
    let Ok(said) = crate::cost::cargo(command, "reproducible::built compiler witness") else {
        return Built::new();
    };
    if !said.status.success() {
        return Built::new();
    }
    let Ok(stdout) = String::from_utf8(said.stdout) else {
        return Built::new();
    };
    let mut found = Built::new();
    for line in stdout.lines() {
        let Ok(message) = crate::strictjson::decode_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let (Some(name), Some(executable)) = (
            message
                .get("target")
                .and_then(|target| target.get("name"))
                .and_then(serde_json::Value::as_str),
            message
                .get("executable")
                .and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(executable) else {
            return Built::new();
        };
        if found
            .insert(name.to_owned(), hex::encode(Sha256::digest(&bytes)))
            .is_some()
        {
            return Built::new();
        }
    }
    found
}

#[expect(
    unused_results,
    reason = "Command's infallible builder API returns self; this unit helper is the explicit boundary"
)]
fn configure_build_command(command: &mut std::process::Command, root: &Path, target: &Path) {
    command
        .env_clear()
        .envs(crate::paths::environment_for_a_toolchain_run(&[]))
        .env("CARGO_INCREMENTAL", "0")
        .args(["test", "--no-run", "--message-format=json", "--offline"])
        .arg("--locked")
        .arg("--target-dir")
        .arg(target)
        .current_dir(root);
}

/// Whether this platform rebuilds a reverted change to the same bytes; an MSVC link stamps each image with its time and a fresh PDB identity.
pub const REVERTED_CHANGE_REPRODUCES: bool = cfg!(not(windows));

/// `image` with what a link stamps afresh every time cleared to zero: in a PE image the COFF header's time, each debug directory entry's time, and the PDB identity of its `CodeView` record; an image of any other format is returned as it is.
#[must_use]
pub fn without_link_stamps(image: &[u8]) -> Vec<u8> {
    let mut cleared = image.to_vec();
    if let Some(stamps) = link_stamps(image) {
        for (at, width) in stamps {
            if let Some(stamp) = at
                .checked_add(width)
                .and_then(|end| cleared.get_mut(at..end))
            {
                stamp.fill(0);
            }
        }
    }
    cleared
}

/// The size of one entry of a PE debug directory.
const DEBUG_ENTRY: usize = 28;

/// The size of one PE section header.
const SECTION_HEADER: usize = 40;

/// Where a PE image holds what its link stamped, as offsets and widths, or nothing for an image that is no PE or that ends before its headers do.
fn link_stamps(image: &[u8]) -> Option<Vec<(usize, usize)>> {
    if image.get(..2)? != b"MZ" {
        return None;
    }
    let signature = little(image, 0x3c, 4)?;
    if image.get(signature..signature.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let coff = signature.checked_add(4)?;
    let mut stamps = vec![(coff.checked_add(4)?, 4)];
    stamps.extend(debug_stamps(image, coff)?);
    Some(stamps)
}

/// The time of each entry of the debug directory of the PE image whose COFF header is at `coff`, and the PDB identity of each `CodeView` record an entry points to.
fn debug_stamps(image: &[u8], coff: usize) -> Option<Vec<(usize, usize)>> {
    let optional = coff.checked_add(20)?;
    let directories = match little(image, optional, 2)? {
        0x10b => optional.checked_add(96)?,
        0x20b => optional.checked_add(112)?,
        _ => return None,
    };
    if little(image, directories.checked_sub(4)?, 4)? <= 6 {
        return Some(Vec::new());
    }
    let debug = directories.checked_add(48)?;
    let sections = (
        optional.checked_add(little(image, coff.checked_add(16)?, 2)?)?,
        little(image, coff.checked_add(2)?, 2)?,
    );
    let table = file_offset(image, sections, little(image, debug, 4)?)?;
    let mut stamps = Vec::new();
    for entry in 0..little(image, debug.checked_add(4)?, 4)?.checked_div(DEBUG_ENTRY)? {
        let at = table.checked_add(entry.checked_mul(DEBUG_ENTRY)?)?;
        stamps.push((at.checked_add(4)?, 4));
        let data = little(image, at.checked_add(24)?, 4)?;
        if little(image, at.checked_add(12)?, 4)? == 2
            && image.get(data..data.checked_add(4)?)? == b"RSDS"
        {
            stamps.push((data.checked_add(4)?, 20));
        }
    }
    Some(stamps)
}

/// Where in the file the section that holds `address` keeps it, reading the `count` section headers at `table`.
fn file_offset(image: &[u8], (table, count): (usize, usize), address: usize) -> Option<usize> {
    for section in 0..count {
        let header = table.checked_add(section.checked_mul(SECTION_HEADER)?)?;
        let start = little(image, header.checked_add(12)?, 4)?;
        let in_memory = little(image, header.checked_add(8)?, 4)?;
        let in_file = little(image, header.checked_add(16)?, 4)?;
        if address >= start && address < start.checked_add(in_memory.max(in_file))? {
            let raw = little(image, header.checked_add(20)?, 4)?;
            return raw.checked_add(address.checked_sub(start)?);
        }
    }
    None
}

/// The little-endian unsigned integer `width` bytes wide at `at`, or nothing past the end of `image`.
fn little(image: &[u8], at: usize, width: usize) -> Option<usize> {
    image
        .get(at..at.checked_add(width)?)?
        .iter()
        .rev()
        .try_fold(0_usize, |value, byte| {
            value.checked_mul(256)?.checked_add(usize::from(*byte))
        })
}

#[cfg(test)]
mod tests {
    /// Writes `bytes` into `image` at `at`.
    fn put(image: &mut [u8], at: usize, bytes: &[u8]) {
        let end = at
            .checked_add(bytes.len())
            .expect("a field's end is an offset");
        image
            .get_mut(at..end)
            .expect("the field lies inside the planted image")
            .copy_from_slice(bytes);
    }

    /// A PE32+ image laid out as an MSVC link lays one out, with one section holding a debug directory of a `CodeView` and a `POGO` entry, every byte nothing names left at `0x11`, and the four stamps at the offsets a link writes them.
    fn planted() -> Vec<u8> {
        let mut image = vec![0x11_u8; 0x400];
        put(&mut image, 0, b"MZ");
        put(&mut image, 0x3c, &0x80_u32.to_le_bytes());
        put(&mut image, 0x80, b"PE\0\0");
        put(&mut image, 0x86, &1_u16.to_le_bytes());
        put(&mut image, 0x88, &[0xaa; 4]);
        put(&mut image, 0x94, &240_u16.to_le_bytes());
        put(&mut image, 0x98, &0x20b_u16.to_le_bytes());
        put(&mut image, 0x104, &16_u32.to_le_bytes());
        put(&mut image, 0x138, &0x2000_u32.to_le_bytes());
        put(&mut image, 0x13c, &56_u32.to_le_bytes());
        put(&mut image, 0x190, &0x200_u32.to_le_bytes());
        put(&mut image, 0x194, &0x2000_u32.to_le_bytes());
        put(&mut image, 0x198, &0x200_u32.to_le_bytes());
        put(&mut image, 0x19c, &0x200_u32.to_le_bytes());
        put(&mut image, 0x204, &[0xbb; 4]);
        put(&mut image, 0x20c, &2_u32.to_le_bytes());
        put(&mut image, 0x218, &0x300_u32.to_le_bytes());
        put(&mut image, 0x220, &[0xcc; 4]);
        put(&mut image, 0x228, &13_u32.to_le_bytes());
        put(&mut image, 0x234, &0_u32.to_le_bytes());
        put(&mut image, 0x300, b"RSDS");
        put(&mut image, 0x304, &[0xdd; 20]);
        put(&mut image, 0x318, b"capture.pdb\0");
        image
    }

    #[test]
    fn a_link_s_time_and_pdb_identity_are_cleared_and_nothing_else() {
        let image = planted();
        let mut expected = image.clone();
        for (at, width) in [(0x88, 4), (0x204, 4), (0x220, 4), (0x304, 20)] {
            put(&mut expected, at, &vec![0; width]);
        }
        assert_eq!(super::without_link_stamps(&image), expected);
        let mut relinked = image.clone();
        put(&mut relinked, 0x88, &[0x5a; 4]);
        put(&mut relinked, 0x304, &[0x5b; 20]);
        assert_eq!(
            super::without_link_stamps(&relinked),
            super::without_link_stamps(&image),
            "two links of one input differ only in what the link stamps"
        );
        let mut changed = image.clone();
        put(&mut changed, 0x318, b"capturf");
        assert_ne!(
            super::without_link_stamps(&changed),
            super::without_link_stamps(&image),
            "a byte the link does not stamp still tells two images apart"
        );
    }

    #[test]
    fn an_image_that_is_no_pe_is_compared_as_it_is() {
        let elf = b"\x7fELF\x02\x01\x01 an image no MSVC link wrote".to_vec();
        assert_eq!(super::without_link_stamps(&elf), elf);
        let mut truncated = planted();
        truncated.truncate(0x210);
        assert_eq!(
            super::without_link_stamps(&truncated),
            truncated,
            "an image whose debug directory runs past its end clears nothing, so it compares \
             as the bytes it is"
        );
    }

    #[test]
    fn identical_reproducibility_questions_share_one_actual_independent_pair() {
        const CHILD: &str = "NJUTEST_REPRODUCIBILITY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(
                super::builds_a_reverted_change_to_the_same_bytes(),
                super::REVERTED_CHANGE_REPRODUCES
            );
            assert_eq!(
                super::builds_a_reverted_change_to_the_same_bytes(),
                super::REVERTED_CHANGE_REPRODUCES
            );
            return;
        }
        let directory = tempfile::tempdir().expect("the actual work owner");
        let cost = directory.path().join("cost");
        std::fs::create_dir_all(&cost).expect("the private actual-work inventory");
        let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args([
                "--exact",
                "reproducible::tests::identical_reproducibility_questions_share_one_actual_independent_pair",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("NJUTEST_TEST_COST_DIR", &cost)
            .env("NJUTEST_FIXTURE_BUILD_CACHE", directory.path().join("cache"))
            .status()
            .expect("the actual paired compiler child");
        assert!(
            status.success(),
            "the unchanged compiler questions: {status}"
        );
        let mut builds = 0_u64;
        for entry in std::fs::read_dir(&cost).expect("the original actual cost records") {
            let path = entry.expect("one actual record").path();
            let record: serde_json::Value = crate::strictjson::decode_slice(
                &std::fs::read(&path).expect("the complete actual record"),
            )
            .expect("strict original work");
            let work = record.get("work").expect("actual work");
            let actual = work
                .get("builds")
                .and_then(serde_json::Value::as_u64)
                .expect("the complete actual build count");
            builds = builds.checked_add(actual).expect("the measured total fits");
            let root = record
                .get("root")
                .and_then(serde_json::Value::as_str)
                .expect("the original compiler source root");
            crate::cost::record(
                std::path::Path::new(root),
                work,
                record.get("sealed").expect("the original sealed work"),
            )
            .expect("charge only the actual current compiler work once");
        }
        assert_eq!(
            builds, 3,
            "two identical questions retain the original, changed and independent restored processes"
        );
    }
}
