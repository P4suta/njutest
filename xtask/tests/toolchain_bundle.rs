// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A bundle built by a real cargo for this machine, read back by this machine's own `tar`, against what binstall's templates resolve to here.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking: a workspace that cannot be laid out leaves nothing to bundle"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use njutest_devkit::paths::{Project, cargo_binary, environment_for_a_toolchain_run};
use sha2::{Digest as _, Sha256};
use xtask::bundle::{Request, Written, bundle};
use xtask::error::{Coded as _, XtCode};

const VERSION: &str = "0.3.1";
const PKG_URL: &str =
    "{ repo }/releases/download/v{ version }/bundled-{ version }-{ target }.tar.gz";
const BIN_DIR: &str = "bundled-{ version }-{ target }/{ bin }{ binary-ext }";

/// A program that answers `--version` the way the shipped binaries do.
const ANSWERS: &str = "fn main() {\n    println!(\"{} {}\", env!(\"CARGO_BIN_NAME\"), env!(\"CARGO_PKG_VERSION\"));\n}\n";

/// A workspace with one package that ships two binaries and one that ships nothing, and a cargo to build it with.
struct Workspace {
    project: Project,
    environment: xtask::environment::Environment,
}

impl Workspace {
    fn new() -> Self {
        let workspace = Self {
            project: Project::fresh(),
            environment: xtask::environment::Environment::of(environment_for_a_toolchain_run(&[
                "RUSTUP_TOOLCHAIN",
            ])),
        };
        workspace.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nresolver = \"3\"\nmembers = [\"shipped\", \"internal\"]\n\n\
                 [workspace.package]\nversion = \"{VERSION}\"\nedition = \"2024\"\n\
                 license = \"MIT OR Apache-2.0\"\nrepository = \"https://example.invalid/bundled\"\n"
            ),
        );
        workspace.write(
            "shipped/Cargo.toml",
            &format!(
                "[package]\nname = \"shipped\"\nversion.workspace = true\nedition.workspace = true\n\
                 license.workspace = true\nrepository.workspace = true\n\n\
                 [[bin]]\nname = \"shipped\"\npath = \"src/main.rs\"\n\n\
                 [[bin]]\nname = \"cargo-shipped\"\npath = \"src/bin/cargo-shipped.rs\"\n\n\
                 [package.metadata.binstall]\npkg-url = \"{PKG_URL}\"\npkg-fmt = \"tgz\"\n\
                 bin-dir = \"{BIN_DIR}\"\n"
            ),
        );
        workspace.write("shipped/src/main.rs", ANSWERS);
        workspace.write("shipped/src/bin/cargo-shipped.rs", ANSWERS);
        workspace.write(
            "internal/Cargo.toml",
            "[package]\nname = \"internal\"\nversion.workspace = true\nedition.workspace = true\n\
             publish = false\n",
        );
        workspace.write("internal/src/main.rs", ANSWERS);
        for document in xtask::bundle::DOCUMENTS {
            workspace.write(document, &format!("{document} of the bundled workspace\n"));
        }
        let locked = Command::new(cargo_binary())
            .args(["generate-lockfile", "--offline"])
            .current_dir(workspace.root())
            .env_clear()
            .envs(workspace.environment.pairs())
            .status()
            .expect("cargo writes the lockfile of a workspace with no dependencies");
        assert!(locked.success(), "cargo generate-lockfile: {locked}");
        workspace
    }

    fn root(&self) -> &Path {
        self.project.path()
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a directory of the workspace");
        }
        std::fs::write(&path, contents).expect("a file of the workspace");
    }

    fn out(&self, name: &str) -> PathBuf {
        self.root().join("..").join(name)
    }

    fn bundle(&self, out: &Path) -> Result<Written, xtask::bundle::BundleError> {
        let host = host();
        bundle(&Request {
            root: self.root(),
            cargo: cargo_binary().as_os_str(),
            environment: &self.environment,
            target: &host,
            out,
        })
    }
}

/// The triple this machine's cargo builds for when it is not told another.
fn host() -> String {
    let said = Command::new(cargo_binary())
        .arg("-vV")
        .output()
        .expect("cargo says what it is");
    String::from_utf8(said.stdout)
        .expect("cargo speaks UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("cargo names its host")
        .to_owned()
}

/// Every entry this machine's `tar` lists in `archive`, without the slash that marks a directory.
fn listed(archive: &Path) -> BTreeSet<String> {
    let said = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .expect("this machine has a tar");
    assert!(said.status.success(), "tar -tzf: {said:?}");
    String::from_utf8(said.stdout)
        .expect("the entries are named in UTF-8")
        .lines()
        .map(|entry| entry.trim_end_matches('/').to_owned())
        .collect()
}

/// What binstall resolves the bundled workspace's `bin-dir` to on this machine, for `bin`.
fn binstall_path(bin: &str) -> String {
    BIN_DIR
        .replace("{ version }", VERSION)
        .replace("{ target }", &host())
        .replace("{ bin }", bin)
        .replace("{ binary-ext }", std::env::consts::EXE_SUFFIX)
}

/// The names in `dir`.
fn names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .expect("the output directory")
        .map(|entry| {
            entry
                .expect("an entry of the output directory")
                .file_name()
                .into_string()
                .expect("a UTF-8 name")
        })
        .collect()
}

#[test]
fn a_bundle_holds_exactly_what_binstall_reads_on_this_machine_and_what_it_digests_to() {
    let workspace = Workspace::new();
    let out = workspace.out("dist");
    let written = match workspace.bundle(&out) {
        Ok(written) => written,
        Err(error) => panic!("{}", error.coded()),
    };
    let archive = format!("bundled-{VERSION}-{}.tar.gz", host());
    assert_eq!(
        names(&out),
        BTreeSet::from([archive.clone(), format!("{archive}.sha256")]),
        "the archive and its checksum, and nothing half-written beside them"
    );
    assert_eq!(written.archive, out.join(&archive));

    let directory = format!("bundled-{VERSION}-{}", host());
    let mut expected = BTreeSet::from([directory.clone()]);
    expected.extend(["shipped", "cargo-shipped"].map(binstall_path));
    expected.extend(xtask::bundle::DOCUMENTS.map(|name| format!("{directory}/{name}")));
    assert_eq!(
        listed(&written.archive),
        expected,
        "the archive holds the binaries the shipped manifest declares where binstall reads \
         them, the licences and the README, and nothing of the package cargo would not publish"
    );

    let bytes = std::fs::read(&written.archive).expect("the archive");
    let digest = hex::encode(Sha256::digest(&bytes));
    assert_eq!(written.digest, digest);
    assert_eq!(
        std::fs::read_to_string(&written.checksum).expect("the checksum"),
        format!("{digest}  {archive}\n"),
        "the checksum reads as `shasum -a 256` writes one"
    );

    let unpacked = workspace.out("unpacked");
    std::fs::create_dir_all(&unpacked).expect("a directory to unpack into");
    let extracted = Command::new("tar")
        .arg("-xzf")
        .arg(&written.archive)
        .current_dir(&unpacked)
        .status()
        .expect("this machine has a tar");
    assert!(extracted.success(), "tar -xzf: {extracted}");
    for bin in ["shipped", "cargo-shipped"] {
        let said = Command::new(unpacked.join(binstall_path(bin)))
            .arg("--version")
            .output()
            .expect("an unpacked binary runs");
        assert_eq!(
            String::from_utf8(said.stdout).expect("the binary speaks UTF-8"),
            format!("{bin} {VERSION}\n"),
            "the binary unpacked from the archive is the one that was built"
        );
    }
}

#[test]
fn the_same_binaries_make_the_same_archive() {
    let workspace = Workspace::new();
    let first = workspace.bundle(&workspace.out("first"));
    let second = workspace.bundle(&workspace.out("second"));
    match (first, second) {
        (Ok(first), Ok(second)) => assert_eq!(
            std::fs::read(&first.archive).expect("the first archive"),
            std::fs::read(&second.archive).expect("the second archive"),
            "nothing but the binaries and the documents decides the bytes: no clock, no owner, \
             no order a directory happened to list in"
        ),
        (first, second) => panic!("{first:?}\n{second:?}"),
    }
}

#[test]
fn a_binary_added_to_a_shipped_manifest_is_bundled_without_any_list_naming_it() {
    let workspace = Workspace::new();
    let manifest = workspace.root().join("shipped/Cargo.toml");
    let declared = std::fs::read_to_string(&manifest).expect("the shipped manifest");
    workspace.write(
        "shipped/Cargo.toml",
        &format!("{declared}\n[[bin]]\nname = \"planted\"\npath = \"src/bin/planted.rs\"\n"),
    );
    workspace.write("shipped/src/bin/planted.rs", ANSWERS);
    let written = match workspace.bundle(&workspace.out("dist")) {
        Ok(written) => written,
        Err(error) => panic!("{}", error.coded()),
    };
    assert!(
        listed(&written.archive).contains(&binstall_path("planted")),
        "the archive follows the manifest: a binary declared there is one binstall looks for"
    );
    assert!(
        written
            .plan
            .programs
            .iter()
            .any(|program| program.binary == "planted"),
        "{:?}",
        written.plan
    );
}

/// How many entries `out` holds, none where it was never made.
fn left_in(out: &Path) -> usize {
    match std::fs::read_dir(out) {
        Ok(entries) => entries.count(),
        Err(absent) if absent.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("{}: {error}", out.display()),
    }
}

#[test]
fn a_binary_that_says_another_version_leaves_no_archive_behind() {
    let workspace = Workspace::new();
    workspace.write(
        "shipped/src/bin/cargo-shipped.rs",
        "fn main() {\n    println!(\"cargo-shipped 9.9.9\");\n}\n",
    );
    let out = workspace.out("dist");
    match workspace.bundle(&out) {
        Ok(written) => panic!("a binary saying 9.9.9 was bundled as {VERSION}: {written:?}"),
        Err(error) => assert_eq!(error.code(), XtCode::BundleVersion, "{}", error.coded()),
    }
    assert_eq!(
        left_in(&out),
        0,
        "a refused bundle writes nothing anybody could upload"
    );
}

#[test]
fn a_binary_that_does_not_build_and_a_manifest_cargo_cannot_read_leave_no_archive_behind() {
    let workspace = Workspace::new();
    workspace.write("shipped/src/bin/cargo-shipped.rs", "fn main() -> u8 {}\n");
    let out = workspace.out("dist");
    match workspace.bundle(&out) {
        Ok(written) => panic!("a binary that does not compile was bundled: {written:?}"),
        Err(error) => assert_eq!(error.code(), XtCode::BundleUnbuilt, "{}", error.coded()),
    }
    workspace.write("Cargo.toml", "[workspace\n");
    match workspace.bundle(&out) {
        Ok(written) => panic!("a workspace cargo cannot read was bundled: {written:?}"),
        Err(error) => assert_eq!(error.code(), XtCode::BundleUnbuilt, "{}", error.coded()),
    }
    assert_eq!(left_in(&out), 0, "neither refusal wrote anything");
}
