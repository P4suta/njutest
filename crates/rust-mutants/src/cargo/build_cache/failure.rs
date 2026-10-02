// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A failed preparation belongs to its waiting cohort and never certifies compiler products.

use std::collections::BTreeMap;
use std::io::{self, Write as _};

use serde::{Deserialize, Serialize};

use super::{Preparation, Request, SCHEMA, complete_environment};
use crate::cargo::{CargoError, CargoErrorKind};
use crate::runner::{RunResult, Spec};
use crate::trace::ExecRecord;
use crate::vars::Variables;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::cargo) enum FailedStage {
    Preparation,
    Process,
    Publication,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    exec: ExecRecord,
    leader: Option<u32>,
    stdout: Vec<u8>,
    stdout_digest: String,
    stderr: Vec<u8>,
    stderr_digest: String,
}

impl Attempt {
    fn verifies(&self) -> bool {
        self.stdout_digest == crate::id::digest(&self.stdout)
            && self.stderr_digest == crate::id::digest(&self.stderr)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    schema: String,
    key: String,
    generation: String,
    stage: FailedStage,
    kind: CargoErrorKind,
    cause: String,
    environment: BTreeMap<String, Option<String>>,
    attempt: Option<Attempt>,
}

impl Request {
    pub(in crate::cargo) fn failure(
        &self,
        (env, preparation): (&Variables, &Preparation),
    ) -> io::Result<CargoError> {
        let record: Failure = crate::strictjson::decode_slice(&std::fs::read(
            self.record.with_extension("failure.json"),
        )?)
        .map_err(io::Error::other)?;
        if record.schema != SCHEMA
            || record.key != self.key
            || !preparation.shares(&record.generation)
            || complete_environment(env)? != record.environment
            || record
                .attempt
                .as_ref()
                .is_some_and(|attempt| !attempt.verifies())
        {
            return Err(io::Error::other(
                "the failed producer belongs to another request",
            ));
        }
        Ok(CargoError::new(record.kind, record.cause))
    }

    pub(in crate::cargo) fn fail(
        &self,
        (error, stage): (&CargoError, FailedStage),
        (spec, result): (&Spec, Option<&RunResult>),
        preparation: &Preparation,
    ) -> io::Result<()> {
        let env = spec
            .env
            .as_ref()
            .ok_or_else(|| io::Error::other("unbound failed environment"))?;
        let parent = self
            .record
            .parent()
            .ok_or_else(|| io::Error::other("failure owner"))?;
        std::fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        let generation = staged
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("failure publication identity"))?
            .to_owned();
        let attempt = result
            .map(|result| {
                Ok::<_, io::Error>(Attempt {
                    exec: ExecRecord::of(spec, result).map_err(io::Error::other)?,
                    leader: result.leader,
                    stdout: result.stdout.clone(),
                    stdout_digest: crate::id::digest(&result.stdout),
                    stderr: result.output.clone(),
                    stderr_digest: crate::id::digest(&result.output),
                })
            })
            .transpose()?;
        let record = Failure {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            generation: generation.clone(),
            stage,
            kind: error.kind(),
            cause: error.message().to_owned(),
            environment: complete_environment(env)?,
            attempt,
        };
        staged.write_all(&serde_json::to_vec(&record).map_err(io::Error::other)?)?;
        staged
            .persist(self.record.with_extension("failure.json"))
            .map_err(io::Error::other)?;
        preparation.publish(&generation)
    }
}
