#!/bin/sh
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# A cargo that says what a test told it to say. It is committed rather than
# written by the test that uses it: a file this process has just opened for
# writing is a file another thread's fork may still hold open, and exec'ing it
# then fails with ETXTBSY. Nothing writes to this one.
if [ -n "${FAKE_CARGO_ARTIFACT:-}" ]; then
  mkdir -p "$(dirname "${FAKE_CARGO_ARTIFACT}")"
  printf '%s' "${FAKE_CARGO_ARTIFACT_CONTENT:-bad input}" >"${FAKE_CARGO_ARTIFACT}"
fi
if [ -n "${FAKE_CARGO_ARTIFACT_TWO:-}" ]; then
  mkdir -p "$(dirname "${FAKE_CARGO_ARTIFACT_TWO}")"
  printf '%s' "${FAKE_CARGO_ARTIFACT_TWO_CONTENT:-second input}" >"${FAKE_CARGO_ARTIFACT_TWO}"
fi
if [ -n "${FAKE_CARGO_ENV_OUT:-}" ]; then
  mkdir -p "$(dirname "${FAKE_CARGO_ENV_OUT}")"
  {
    printf 'RUSTFLAGS=%s\n' "${RUSTFLAGS-<unset>}"
    printf 'CARGO_ENCODED_RUSTFLAGS=%s\n' "${CARGO_ENCODED_RUSTFLAGS-<unset>}"
    printf 'MIRIFLAGS=%s\n' "${MIRIFLAGS-<unset>}"
  } >"${FAKE_CARGO_ENV_OUT}"
fi
if [ -n "${FAKE_CARGO_ARGV_OUT:-}" ]; then
  mkdir -p "$(dirname "${FAKE_CARGO_ARGV_OUT}")"
  printf '%s\n' "$@" >"${FAKE_CARGO_ARGV_OUT}"
fi
if [ -n "${FAKE_CARGO_SLEEP:-}" ]; then
  sleep "${FAKE_CARGO_SLEEP}"
fi
if [ -n "${FAKE_CARGO_SAYS:-}" ]; then
  printf '%s\n' "${FAKE_CARGO_SAYS}"
fi
exit "${FAKE_CARGO_CODE:-0}"
