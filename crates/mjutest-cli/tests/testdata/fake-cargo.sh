#!/bin/sh
# SPDX-FileCopyrightText: 2026 mjutest contributors
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
if [ -n "${FAKE_CARGO_SAYS:-}" ]; then
  printf '%s\n' "${FAKE_CARGO_SAYS}"
fi
exit "${FAKE_CARGO_CODE:-0}"
