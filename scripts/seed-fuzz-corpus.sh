#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

if [[ $# -ne 1 || ! $1 =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo "usage: seed-fuzz-corpus.sh TARGET" >&2
  exit 2
fi

target=$1
seed="seeds/${target}"
corpus="corpus/${target}"
if [[ ! -d "${seed}" ]]; then
  exit 0
fi

mkdir -p "${corpus}"
cp -R "${seed}/." "${corpus}/"
