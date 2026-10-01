#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Read the shared Rust toolchain pins for CI and local tools.

The parser is a strict reader of the two pinned string fields rather than a TOML library, because the system Python these scripts run on has no ``tomllib``.
Any deviation from the grammar fails closed rather than guessing a pin.
"""

import pathlib
import re
import sys

text = (
    pathlib.Path(__file__).resolve().parent.parent / "rust-toolchain.toml"
).read_text()


def pinned(section, key):
    body = re.findall(
        rf"^\[{re.escape(section)}\]\n(.*?)(?=^\[|\Z)", text, re.MULTILINE | re.DOTALL
    )
    if len(body) != 1:
        sys.exit(f"toolchain: exactly one [{section}] section is required")
    found = re.findall(rf'^{re.escape(key)} = "([^"]+)"', body[0], re.MULTILINE)
    if len(found) != 1:
        sys.exit(f"toolchain: exactly one {section}.{key} pin is required")
    return found[0]


stable = pinned("toolchain", "channel")
nightly = pinned("njutest", "nightly")
if not re.fullmatch(r"\d+\.\d+\.\d+", stable) or not re.fullmatch(
    r"nightly-\d{4}-\d{2}-\d{2}", nightly
):
    sys.exit("toolchain: stable must be an exact version and nightly must be dated")
if sys.argv[1:] == ["--env"]:
    print(f"NJUTEST_NIGHTLY={nightly}")
elif sys.argv[1:] == ["--output"]:
    print(f"stable={stable}\nnightly={nightly}")
else:
    sys.exit("usage: toolchain.py --env | --output")
