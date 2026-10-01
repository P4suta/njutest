#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Run the complete nextest suite once and enforce the recorded compile/build cost ledger."""

import json
import os
import pathlib
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parent.parent
output = root / "target" / "suite-cost"
output.mkdir(parents=True, exist_ok=True)
directory = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=output))
environment = os.environ.copy()
environment["NJUTEST_TEST_COST_DIR"] = str(directory)
environment["CARGO_BUILD_JOBS"] = "6"
checks = subprocess.run([sys.executable, str(root / "scripts" / "test-suite-cost.py")], cwd=root, check=False)
if checks.returncode:
    sys.exit(checks.returncode)
command = ["cargo", "nextest", "run", "--locked", "--workspace", "--all-targets", "--all-features", "--no-fail-fast", "--test-threads", "2", "--profile", "cost"]
run = subprocess.run(command, cwd=root, env=environment, check=False)
if run.returncode:
    sys.exit(run.returncode)
metadata = subprocess.run(["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--no-deps"], cwd=root, env=environment, capture_output=True, check=True)
junit = pathlib.Path(json.loads(metadata.stdout)["target_directory"]) / "nextest" / "cost" / "junit.xml"
check = subprocess.run([sys.executable, str(root / "scripts" / "suite-cost.py"), str(directory), str(junit), *sys.argv[1:]], cwd=root, check=False)
sys.exit(check.returncode)
