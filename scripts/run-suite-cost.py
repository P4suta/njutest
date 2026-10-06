#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Run the complete nextest suite once and enforce the recorded compile/build cost ledger."""

import json
import datetime
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import time

root = pathlib.Path(__file__).resolve().parent.parent
output = root / "target" / "suite-cost"
output.mkdir(parents=True, exist_ok=True)
directory = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=output))
environment = os.environ.copy()
environment["NJUTEST_TEST_COST_DIR"] = str(directory)
environment["CARGO_BUILD_JOBS"] = "3"
environment["RUSTC_WRAPPER"] = ""
environment["NJUTEST_FIXTURE_BUILD_CACHE"] = str(directory / "fixture-builds")
checks = subprocess.run([sys.executable, str(root / "scripts" / "test-suite-cost.py")], cwd=root, check=False)
if checks.returncode:
    sys.exit(checks.returncode)
command = ["cargo", "xtask", "tidy", "--", "cargo", "nextest", "run", "--locked", "--workspace", "--all-targets", "--all-features", "--no-fail-fast", "--test-threads", "3", "--status-level", "pass", "--profile", "cost"]
started = time.monotonic()
run = subprocess.Popen(command, cwd=root, env=environment)
samples = []
with (directory / "load.jsonl").open("w") as evidence:
    while True:
        active = []
        if os.name == "posix":
            processes = subprocess.run(["ps", "-axo", "pid,ppid,comm"], capture_output=True, text=True, check=True)
            rows = [row.split(None, 2) for row in processes.stdout.splitlines()[1:]]
            descendants = {run.pid}
            for _ in rows:
                found = {int(pid) for pid, parent, _ in rows if int(parent) in descendants}
                if found <= descendants:
                    break
                descendants.update(found)
            active = [{"pid": int(pid), "binary": pathlib.Path(program).name}
                for pid, _, program in rows if int(pid) in descendants and "/deps/toolchain_" in program]
        sample = {"elapsed_seconds": time.monotonic() - started,
            "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "load": list(os.getloadavg()) if hasattr(os, "getloadavg") else None,
            "toolchain_running": len(active), "active": active}
        samples.append(sample)
        evidence.write(json.dumps(sample) + "\n")
        evidence.flush()
        try:
            status = run.wait(timeout=5)
            break
        except subprocess.TimeoutExpired:
            continue
measurement = {"command": command, "build_jobs": 3, "test_threads": 3,
    "wrapper": "", "machine": platform.platform(), "cpu_count": os.cpu_count(),
    "elapsed_seconds": time.monotonic() - started, "exit_code": status,
    "maximum_observed_toolchain_concurrency": max(sample["toolchain_running"] for sample in samples),
    "load_samples": samples}
(directory / "machine.json").write_text(json.dumps(measurement, indent=2) + "\n")
print(f"Machine and concurrency evidence: {directory / 'machine.json'}", flush=True)
metadata = subprocess.run(["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--no-deps"], cwd=root, env=environment, capture_output=True, check=True)
junit = pathlib.Path(json.loads(metadata.stdout)["target_directory"]) / "nextest" / "cost" / "junit.xml"
shutil.copyfile(junit, directory / "junit.xml")
if status:
    subprocess.run([sys.executable, str(root / "scripts" / "suite-cost.py"), str(directory), str(junit), "--measure-only"], cwd=root, check=False)
    sys.exit(status)
check = subprocess.run([sys.executable, str(root / "scripts" / "suite-cost.py"), str(directory), str(junit), *sys.argv[1:]], cwd=root, check=False)
sys.exit(check.returncode)
