#!/bin/sh
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# A resource provider that answers what a test told it to answer. Committed
# for the same reason as fake-cargo.sh: nothing writes to it, so exec'ing it
# cannot race a fork in another thread.
while IFS= read -r line; do
  case "${line}" in
    *'"action":"start"'*)
      if [ -n "${FAKE_PROVIDER_SILENT:-}" ]; then
        continue
      fi
      printf '%s\n' "${FAKE_PROVIDER_READY}"
      ;;
    *'"action":"stop"'*)
      printf '%s\n' "${FAKE_PROVIDER_STOPPED}"
      exit 0
      ;;
    *) ;;
  esac
done
