#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Record the complete suite's timings and engine work, and refuse growth in compile/build counts."""

import argparse
import collections
import json
import math
import pathlib
import platform
import sys
import xml.etree.ElementTree as ET


def unique(pairs):
    """A duplicate diagnostic field cannot replace the measured count before validation."""
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate diagnostic field: {key}")
        result[key] = value
    return result


def number(value, label):
    """Require an actual nonnegative count rather than defaulting a missing observation."""
    if type(value) is not int or value < 0 or value > 2**64 - 1:
        raise ValueError(f"{label}: expected a nonnegative measured integer, got {value!r}")
    return value


def measured(directory, junit, require_pass=True):
    """Join complete records to the tests nextest actually executed."""
    suite = ET.parse(junit).getroot()
    if require_pass and (int(suite.attrib["failures"]) or int(suite.attrib["errors"])):
        raise ValueError("the suite did not pass; its cost cannot establish a new baseline")
    tests = {}
    binaries = collections.defaultdict(lambda: {
        "tests": 0, "seconds": 0.0, "records": 0, "builds": 0, "units": 0,
        "preparations": 0, "instances": 0, "hits": 0, "misses": 0,
        "compile_ns": 0, "execution_ns": 0, "build_ms": 0, "module_requests": 0,
    })
    for binary in suite.findall("testsuite"):
        identity = binary.attrib["name"]
        for test in binary.findall("testcase"):
            key = (identity, test.attrib["name"])
            if key in tests:
                raise ValueError(f"duplicate or retried test: {key}")
            row = {"binary": identity, "test": key[1], "seconds": float(test.attrib["time"]), "records": 0,
                "builds": 0, "units": 0, "preparations": 0, "instances": 0,
                "hits": 0, "misses": 0, "compile_ns": 0, "execution_ns": 0, "build_ms": 0, "module_requests": 0}
            if not math.isfinite(row["seconds"]) or row["seconds"] < 0:
                raise ValueError(f"invalid test duration: {key}")
            tests[key] = row
            binaries[identity]["tests"] += 1
            binaries[identity]["seconds"] += row["seconds"]
    if len(tests) != int(suite.attrib["tests"]):
        raise ValueError("the JUnit test inventory does not close")
    paths = sorted(pathlib.Path(directory).glob("cost-*.json"))
    if not paths:
        raise ValueError("no cost records: run with NJUTEST_TEST_COST_DIR and nextest labels")
    for path in paths:
        record = json.loads(path.read_text(), object_pairs_hook=unique)
        if record["schema"] != "njutest-test-cost-v1":
            raise ValueError(f"{path}: unknown cost schema")
        key = (record["binary"], record["test"])
        if key not in tests:
            raise ValueError(f"{path}: record does not belong to this suite: {key}")
        row = tests[key]
        work = record["work"]
        if work["error"] is not None:
            raise ValueError(f"{path}: {work['error']}")
        row["records"] += 1
        for name in ("builds", "build_ms", "units"):
            row[name] += number(work[name], f"{path}:{name}")
        if "platform_requests" in work:
            row["module_requests"] += number(work["platform_requests"], f"{path}:platform_requests")
        elif require_pass:
            raise ValueError(f"{path}: platform request count is absent")
        modules = work["platform"]
        if record["sealed"] is not None:
            row["module_requests"] += number(record["sealed"]["compiles"], f"{path}:bench_modules")
            modules = [*modules, record["sealed"]]
        for sealed in modules:
            row["preparations"] += number(sealed["compiles"], f"{path}:compiles")
            row["instances"] += number(sealed["instances"], f"{path}:instances")
            compilation = sealed["compilation"] if "compilation" in sealed else None
            if sealed["compiles"] and compilation is None:
                raise ValueError(f"{path}: module preparation timing is absent")
            if compilation is not None:
                hits = number(compilation["hits"], f"{path}:hits")
                misses = number(compilation["misses"], f"{path}:misses")
                if hits + misses != sealed["compiles"]:
                    raise ValueError(f"{path}: cache accounting does not close")
                row["hits"] += hits
                row["misses"] += misses
                row["compile_ns"] += number(compilation["duration_ns"], f"{path}:duration_ns")
            if "execution_ns" in sealed:
                row["execution_ns"] += number(sealed["execution_ns"], f"{path}:execution_ns")
            elif sealed["instances"]:
                raise ValueError(f"{path}: executed instance timing is absent")
    for row in tests.values():
        if require_pass and row["preparations"] > row["module_requests"]:
            raise ValueError(f"{row['binary']}::{row['test']}: module preparations exceed recorded requests")
        for name in row:
            if name not in ("binary", "test", "seconds"):
                binaries[row["binary"]][name] += row[name]
    return {"schema": "njutest-suite-cost-v1", "platform": platform.system(),
        "wall_seconds": float(suite.attrib["time"]), "failures": int(suite.attrib["failures"]), "errors": int(suite.attrib["errors"]), "tests": list(tests.values()), "binaries": dict(binaries)}


def budget(report):
    """Count module preparations as well as actual misses so a warm cache cannot hide new work."""
    result = {binary: {name: row[name] for name in ("tests", "records", "builds", "module_requests")}
        for binary, row in sorted(report["binaries"].items()) if "::toolchain_" in binary}
    direct = {"njutest::toolchain_build", "njutest::toolchain_edits", "xtask::toolchain_bundle"}
    for binary in direct & result.keys():
        if not result[binary]["builds"] or not result[binary]["records"]:
            raise ValueError(f"{binary}: direct Cargo builds were not measured")
    guests = "rust-mutants-sealed::toolchain_guests"
    if guests in result:
        row = report["binaries"][guests]
        result[guests]["records"] -= row["builds"]
        result[guests]["builds"] = max(2, row["builds"])
    return result


def growth(actual, expected):
    """Refuse newly unmeasured work, omitted binaries and every increase in compile/build counts."""
    errors = []
    if actual.keys() != expected.keys():
        errors.append(f"toolchain binaries changed: added={sorted(actual.keys() - expected.keys())}, missing={sorted(expected.keys() - actual.keys())}")
    for binary in actual.keys() & expected.keys():
        for name in ("tests", "builds", "module_requests"):
            limit = number(expected[binary][name], f"baseline:{binary}:{name}")
            if actual[binary][name] > limit:
                errors.append(f"{binary}: {name} grew from {limit} to {actual[binary][name]}")
        if actual[binary]["records"] < expected[binary]["records"]:
            errors.append(f"{binary}: cost records disappeared")
        if actual[binary]["tests"] != expected[binary]["tests"]:
            errors.append(f"{binary}: the complete test inventory changed")
    return errors


def main():
    """Write reviewable per-test measurements and check the platform's committed count ledger."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=pathlib.Path)
    parser.add_argument("junit", type=pathlib.Path)
    parser.add_argument("--baseline", type=pathlib.Path, default=pathlib.Path(".config/suite-costs.json"))
    parser.add_argument("--record", action="store_true", help="establish or replace this platform's reviewed count ledger")
    parser.add_argument("--measure-only", action="store_true")
    args = parser.parse_args()
    report = measured(args.directory, args.junit, require_pass=not args.measure_only)
    output = args.directory / "suite-cost.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{len(report['tests'])} tests; suite wall {report['wall_seconds']:.3f}s; per-test counts: {output}")
    for binary, row in sorted(report["binaries"].items(), key=lambda pair: pair[1]["seconds"], reverse=True)[:15]:
        print(f"{binary}: {row['seconds']:.3f}s summed, {row['preparations']} modules ({row['hits']} hits/{row['misses']} misses), {row['builds']} fixture cargo calls/{row['units']} fresh units, compile {row['compile_ns']/1e9:.3f}s / execute {row['execution_ns']/1e9:.3f}s")
    if args.measure_only:
        return
    if args.record:
        ledger = json.loads(args.baseline.read_text()) if args.baseline.exists() else {"schema": "njutest-suite-cost-budget-v1", "platforms": {}}
        ledger["platforms"][report["platform"]] = budget(report)
        args.baseline.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n")
        return
    ledger = json.loads(args.baseline.read_text())
    if ledger["schema"] != "njutest-suite-cost-budget-v1":
        raise ValueError("unknown suite cost ledger schema")
    errors = growth(budget(report), ledger["platforms"][report["platform"]])
    if errors:
        raise ValueError("suite work grew:\n" + "\n".join(errors))
    print("suite compile/build counts stayed within the committed ledger")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, ET.ParseError) as error:
        print(f"suite-cost: {error}", file=sys.stderr)
        sys.exit(1)
