// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `mjutest cache`: what earlier runs left behind, and collecting what is no longer an answer.

use std::io::Write;

use jiff::Timestamp;

use crate::cache::store::Store;
use crate::cli::{Cache, EXIT_ASSURED, EXIT_ERROR, Environment};
use crate::config::Config;

/// Says what the store holds, and collects it when asked.
pub fn run(
    arguments: &Cache,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = arguments
        .directory
        .clone()
        .unwrap_or_else(|| environment.working_directory.clone());
    let config = match Config::load(&root) {
        Ok(config) => config,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let store = Store::new(
        &environment.cache_directory,
        config.cache.max_bytes,
        config.cache.ttl,
    );
    let collected = if arguments.gc {
        match store.collect(Timestamp::now()) {
            Ok(collected) => collected,
            Err(error) => {
                super::diagnose(stderr, &error.to_string());
                return EXIT_ERROR;
            }
        }
    } else {
        crate::cache::store::Collected::default()
    };
    let status = match store.status() {
        Ok(status) => status,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    super::say(stdout, &format!("root      {}", store.root().display()));
    super::say(
        stdout,
        &format!(
            "holds     {} answers, {} bytes of at most {}",
            status.entries, status.bytes, config.cache.max_bytes
        ),
    );
    super::say(
        stdout,
        &format!(
            "collected {} expired, {} evicted, {} bytes",
            collected.expired.len(),
            collected.evicted.len(),
            collected.bytes
        ),
    );
    EXIT_ASSURED
}
