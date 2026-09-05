#!/bin/sh
# SPDX-FileCopyrightText: 2026 mjutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# A generation provider that offers what a test told it to offer. Committed
# for the same reason as fake-cargo.sh.
cat >/dev/null
printf '%s\n' "${FAKE_GENERATOR_OFFERS}"
