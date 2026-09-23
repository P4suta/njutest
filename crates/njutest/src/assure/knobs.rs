// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Putting a knob: what a control of a target is started with to set one thing the contract lets differ between machines, and whether this machine can.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::execute::{Launcher, Schedule, TestTarget, Variable};
use rust_mutants::outcome::Outcome;
use rust_mutants::runner::{Bound, Cancel, Spec};
use rust_mutants::session::{Conditions, Controlled, Observing, Perturbation, Request, Session};
use rust_mutants::touch::Steadiness;

use crate::error::RunnerError;
use crate::report::drift::{Moved, Unmeasured};
use crate::report::knobs::{Knob, KnobRecord, NotPut, Reach, Standing, Unsettled};
use crate::watch::Watch;

/// The zone a control is put in: a half-hour offset whose daylight saving moves by half an hour.
const ZONE: &str = "Australia/Lord_Howe";

/// The offsets `date +%z` prints in that zone, one for each half of the year.
const ZONE_OFFSETS: [&str; 2] = ["+1030", "+1100"];

/// The locale a control is put in, whose dotless i breaks case folding.
const LOCALE: &str = "tr_TR.UTF-8";

/// The mask new files are created under when the umask knob is put.
const MASK: u32 = 0o077;

/// What this machine can put, learned once before any control is started, and the directories the knobs start controls in.
#[derive(Debug, Clone)]
pub struct Place {
    zone: bool,
    locale: bool,
    shell: bool,
    temp_directory: PathBuf,
    home: PathBuf,
    cargo_home: Option<OsString>,
    rustup_home: Option<OsString>,
}

impl Place {
    /// Probes this machine through `vars` and makes the empty directories a control is given as its temporary and home directories, under `scratch`.
    ///
    /// # Errors
    /// A directory that could not be made.
    pub fn probed(
        scratch: &Path,
        vars: &[(OsString, OsString)],
        cancel: &Cancel,
    ) -> std::io::Result<Self> {
        let temp_directory = scratch.join("knobs").join("temp directory \u{e9}");
        let home = scratch.join("knobs").join("home");
        std::fs::create_dir_all(&temp_directory)?;
        std::fs::create_dir_all(&home)?;
        let named = |name: &str| {
            vars.iter()
                .find(|(held, _)| rust_mutants::vars::same_name(held, std::ffi::OsStr::new(name)))
                .map(|(_, value)| value.clone())
        };
        let under_home = |directory: &str| {
            named("HOME").map(|home| PathBuf::from(home).join(directory).into_os_string())
        };
        Ok(Self {
            zone: cfg!(unix) && zone_is_known(vars, cancel),
            locale: cfg!(unix) && locale_is_installed(vars, cancel),
            shell: cfg!(unix) && answers(&["sh", "-c", "exit 0"], vars, cancel).is_some(),
            temp_directory,
            home,
            cargo_home: named("CARGO_HOME").or_else(|| under_home(".cargo")),
            rustup_home: named("RUSTUP_HOME").or_else(|| under_home(".rustup")),
        })
    }
}

/// What `argv` printed on success, run with `vars` and a probe's bound, or nothing where it could not be started or did not succeed.
fn answers(argv: &[&str], vars: &[(OsString, OsString)], cancel: &Cancel) -> Option<Vec<u8>> {
    let argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
    let mut spec = Spec::new(argv, Bound::After(rust_mutants::runner::PROBE));
    spec.env = Some(vars.to_vec());
    let ran = rust_mutants::runner::run(&spec, cancel);
    ran.succeeded().then_some(ran.output)
}

/// Whether the time zone database knows the zone, which it shows by printing one of the zone's own offsets rather than UTC's.
fn zone_is_known(vars: &[(OsString, OsString)], cancel: &Cancel) -> bool {
    let mut zoned: Vec<(OsString, OsString)> = vars
        .iter()
        .filter(|(name, _)| !rust_mutants::vars::same_name(name, std::ffi::OsStr::new("TZ")))
        .cloned()
        .collect();
    zoned.push((OsString::from("TZ"), OsString::from(ZONE)));
    answers(&["date", "+%z"], &zoned, cancel).is_some_and(|printed| {
        std::str::from_utf8(&printed).is_ok_and(|offset| ZONE_OFFSETS.contains(&offset.trim()))
    })
}

/// Whether the locale is installed, by the list `locale -a` prints, which spells a codeset more than one way.
fn locale_is_installed(vars: &[(OsString, OsString)], cancel: &Cancel) -> bool {
    let spelled = |name: &str| name.to_ascii_lowercase().replace('-', "");
    let wanted = spelled(LOCALE);
    answers(&["locale", "-a"], vars, cancel).is_some_and(|printed| {
        std::str::from_utf8(&printed)
            .is_ok_and(|listed| listed.lines().any(|one| spelled(one.trim()) == wanted))
    })
}

/// What putting `knob` for `target` starts its control with, or why it cannot be put.
///
/// # Errors
/// The [`NotPut`] that says why this machine, this platform, or this target cannot have it.
pub fn perturbation(
    knob: Knob,
    target: &TestTarget,
    place: &Place,
) -> Result<Perturbation, NotPut> {
    let through_cargo = !target.through.is_empty();
    let mut put = Perturbation::none();
    match knob {
        Knob::Timezone => {
            if !cfg!(unix) {
                return Err(NotPut::Platform);
            }
            if !place.zone {
                return Err(NotPut::ZoneMissing);
            }
            put.environment.push((Variable::Tz, OsString::from(ZONE)));
        }
        Knob::Locale => {
            if !cfg!(unix) {
                return Err(NotPut::Platform);
            }
            if !place.locale {
                return Err(NotPut::LocaleMissing);
            }
            put.environment
                .push((Variable::LcAll, OsString::from(LOCALE)));
        }
        Knob::TempDirectory => {
            if through_cargo {
                return Err(NotPut::ThroughCargo);
            }
            let directory = place.temp_directory.clone().into_os_string();
            for variable in [Variable::Tmpdir, Variable::Tmp, Variable::Temp] {
                put.environment.push((variable, directory.clone()));
            }
        }
        Knob::Home => {
            if !cfg!(unix) {
                return Err(NotPut::Platform);
            }
            let (Some(cargo_home), Some(rustup_home)) = (&place.cargo_home, &place.rustup_home)
            else {
                if through_cargo {
                    return Err(NotPut::ThroughCargo);
                }
                put.environment
                    .push((Variable::Home, place.home.clone().into_os_string()));
                return Ok(put);
            };
            put.environment
                .push((Variable::Home, place.home.clone().into_os_string()));
            put.environment
                .push((Variable::CargoHome, cargo_home.clone()));
            put.environment
                .push((Variable::RustupHome, rustup_home.clone()));
        }
        Knob::Umask => {
            if !cfg!(unix) {
                return Err(NotPut::Platform);
            }
            if !place.shell {
                return Err(NotPut::ShellMissing);
            }
            put.launcher = Some(Launcher::Umask { mask: MASK });
        }
        Knob::Columns => {
            put.environment
                .push((Variable::Columns, OsString::from("37")));
            put.environment
                .push((Variable::Lines, OsString::from("11")));
        }
        Knob::Threads => {
            if !target.harness {
                return Err(NotPut::NotLibtest);
            }
            put.schedule = Schedule::OneThread;
        }
    }
    Ok(put)
}

/// The test binaries every test of which passed on `baseline`, by the identity the engine starts them under: a knob asks whether a pass holds, so a binary that did not pass has nothing for it to hold.
#[must_use]
pub fn passing(baseline: &super::baseline::Baseline) -> BTreeSet<String> {
    let mut passed: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for measured in &baseline.targets {
        let target = &measured.target;
        let binary = format!(
            "{}/{}/{}",
            target.package,
            target.unit.name(),
            target.unit_name
        );
        let this = measured.status == crate::report::TargetStatus::Passed;
        passed
            .entry(binary)
            .and_modify(|every| *every = *every && this)
            .or_insert(this);
    }
    passed
        .into_iter()
        .filter_map(|(binary, every)| every.then_some(binary))
        .collect()
}

/// What each knob in `asked` establishes about each target of `session` whose baseline passed, in knob and then target order.
///
/// # Errors
/// What stopped a control from being started at all; a control that ran and established nothing is a record that says so.
pub fn measured(
    session: &Session,
    asked: &[Knob],
    (passed, place): (&BTreeSet<String>, &Place),
    watch: Watch<'_>,
) -> Result<Vec<KnobRecord>, RunnerError> {
    let asked: BTreeSet<Knob> = asked.iter().copied().collect();
    let mut records = Vec::new();
    for knob in asked {
        for target in session
            .targets()
            .iter()
            .filter(|target| passed.contains(&target.id))
        {
            if watch.cancel.is_cancelled() {
                return Err(RunnerError::Interrupted);
            }
            let standing = match perturbation(knob, target, place) {
                Err(why) => Standing::NotPut { why },
                Ok(put) => {
                    let controlled = session.control_perturbed(
                        &Request::new(String::new()).with_target(target.id.as_str()),
                        Conditions {
                            observing: Observing::Reach,
                            perturbation: &put,
                        },
                        watch.cancel,
                    )?;
                    standing(&controlled, &target.id)
                }
            };
            records.push(KnobRecord {
                target: target.id.clone(),
                knob,
                standing,
            });
        }
    }
    Ok(records)
}

/// What a control of `target` established against its baseline: the verdict first, and the reach only over a passing control.
fn standing(controlled: &Controlled, target: &str) -> Standing {
    match controlled.result.outcome() {
        Outcome::Survived => match controlled
            .observed
            .iter()
            .find(|one| one.target == target)
            .map(|one| &one.steadiness)
        {
            Some(Steadiness::Held) => Standing::Stable,
            Some(Steadiness::Moved(moved)) => Standing::Moved {
                reach: Box::new(Reach {
                    reached: Moved::of(&moved.reached),
                    bodies: Moved::of(&moved.bodies),
                    infected: Moved::of(&moved.infected),
                }),
            },
            Some(Steadiness::NotMeasured(why)) => Standing::Uncompared {
                why: Unmeasured::of(*why),
            },
            None => Standing::Passed,
        },
        Outcome::Killed => Standing::Broke {
            failed: controlled.result.failed_tests.clone(),
        },
        Outcome::Waited => Standing::Unsettled {
            why: Unsettled::Waited,
        },
        Outcome::NotRun | Outcome::StepLimitReached | Outcome::Inconclusive | Outcome::Errored => {
            Standing::Unsettled {
                why: Unsettled::Errored,
            }
        }
    }
}
