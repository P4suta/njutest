#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Record the complete suite's timings and engine work, and refuse growth in compile/build counts."""

import argparse
import collections
import datetime
import json
import math
import pathlib
import platform
import sys
import xml.etree.ElementTree as ET

LEDGER_SCHEMA = "njutest-suite-cost-budget-v3"
WORK_NUMBERS = (
    "builds",
    "build_ms",
    "units",
    "build_requests",
    "build_hits",
    "build_misses",
    "direct_commands",
    "cargo_test_processes",
    "cargo_other_processes",
)
INVENTORY_NUMBERS = ("direct_commands", "cargo_test_processes", "cargo_other_processes")


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
        raise ValueError(
            f"{label}: expected a nonnegative measured integer, got {value!r}"
        )
    return value


def key_work(value, label):
    """Read one bound key's multiplicity: every request, process, hit, miss and concrete reason."""
    if not isinstance(value, dict):
        raise ValueError(f"{label}: a bound key's work is not a record")
    for name in ("requests", "hits", "misses", "processes"):
        if name not in value:
            raise ValueError(f"{label}: bound key count {name} is absent")
        number(value[name], f"{label}:{name}")
    for name in ("reasons", "refused_writes"):
        if name not in value:
            raise ValueError(f"{label}: bound key {name} are absent")
        if not isinstance(value[name], dict):
            raise ValueError(
                f"{label}: bound key {name} are not a mapping of concrete causes"
            )
        for reason, count in value[name].items():
            if not isinstance(reason, str) or not reason:
                raise ValueError(f"{label}:{name}: a cause without its concrete reason")
            number(count, f"{label}:{name}:{reason}")
    if value["hits"] + value["misses"] != value["requests"]:
        raise ValueError(f"{label}: requests do not close into hits and misses")
    if value["misses"] != value["processes"]:
        raise ValueError(f"{label}: misses and processes do not close")
    if sum(value["reasons"].values()) != value["misses"]:
        raise ValueError(f"{label}: misses without their concrete reasons do not close")
    return {
        name: value[name] for name in ("requests", "hits", "misses", "processes")
    } | {
        "reasons": dict(value["reasons"]),
        "refused_writes": dict(value["refused_writes"]),
    }


def unbound_work(value, identity, label):
    """Read one unbound identity's multiplicity, closed by whether the cache was ever asked."""
    if not isinstance(value, dict):
        raise ValueError(f"{label}: an unbound identity's work is not a record")
    for name in ("requests", "misses", "processes"):
        if name not in value:
            raise ValueError(f"{label}: unbound count {name} is absent")
        number(value[name], f"{label}:{name}")
    if value["requests"] != value["processes"]:
        raise ValueError(f"{label}: unbound requests and processes do not close")
    expected_misses = value["requests"] if identity.startswith("unbound") else 0
    if value["misses"] != expected_misses:
        raise ValueError(f"{label}: unbound misses do not close")
    return {name: value[name] for name in ("requests", "misses", "processes")}


def merge_key(into, held):
    """Sum one key's multiplicity across every record of a test, never dropping a repeat."""
    for name in ("requests", "hits", "misses", "processes"):
        into[name] += held[name]
    for reason, count in held["reasons"].items():
        into["reasons"][reason] = into["reasons"].get(reason, 0) + count
    for reason, count in held["refused_writes"].items():
        into["refused_writes"][reason] = into["refused_writes"].get(reason, 0) + count


def merge_unbound(into, held):
    for name in ("requests", "misses", "processes"):
        into[name] += held[name]


def refuse_redundancy(row, label):
    """A bound input whose cold builds repeat had a record that should have answered."""
    for key, held in row["build_keys"].items():
        cold = sum(
            count
            for reason, count in held["reasons"].items()
            if reason.startswith("cold:")
        )
        if cold >= 2 and not held["refused_writes"]:
            raise ValueError(
                f"{label}: redundant cold builds of one bound input: {key} built cold {cold} times"
            )


def measured(directory, junit, require_pass=True):
    """Join complete records to the tests nextest actually executed."""
    suite = ET.parse(junit).getroot()
    if require_pass and (int(suite.attrib["failures"]) or int(suite.attrib["errors"])):
        raise ValueError(
            "the suite did not pass; its cost cannot establish a new baseline"
        )
    tests = {}
    machine_file = pathlib.Path(directory) / "machine.json"
    machine = json.loads(machine_file.read_text()) if machine_file.exists() else None

    def empty_row(binary, test):
        return {
            "binary": binary,
            "test": test,
            "seconds": 0.0,
            "records": 0,
            "builds": 0,
            "units": 0,
            "preparations": 0,
            "instances": 0,
            "hits": 0,
            "misses": 0,
            "compile_ns": 0,
            "execution_ns": 0,
            "build_ms": 0,
            "module_requests": 0,
            "build_requests": 0,
            "build_hits": 0,
            "build_misses": 0,
            "build_keys": {},
            "unbound": {},
            **{name: 0 for name in INVENTORY_NUMBERS},
        }

    binaries = collections.defaultdict(lambda: empty_row("", ""))
    for binary in suite.findall("testsuite"):
        identity = binary.attrib["name"]
        for test in binary.findall("testcase"):
            key = (identity, test.attrib["name"])
            if key in tests:
                raise ValueError(f"duplicate or retried test: {key}")
            row = empty_row(identity, key[1])
            row["seconds"] = float(test.attrib["time"])
            if not math.isfinite(row["seconds"]) or row["seconds"] < 0:
                raise ValueError(f"invalid test duration: {key}")
            if "timestamp" in test.attrib:
                row["timestamp"] = test.attrib["timestamp"]
                if machine is not None:
                    instant = datetime.datetime.fromisoformat(test.attrib["timestamp"])
                    nearest = min(
                        machine["load_samples"],
                        key=lambda sample: abs(
                            (
                                datetime.datetime.fromisoformat(sample["utc"]) - instant
                            ).total_seconds()
                        ),
                    )
                    row["load"] = nearest["load"]
            tests[key] = row
            binaries[identity]["tests"] = binaries[identity].get("tests", 0) + 1
            binaries[identity]["seconds"] = (
                binaries[identity].get("seconds", 0.0) + row["seconds"]
            )
    if len(tests) != int(suite.attrib["tests"]):
        raise ValueError("the JUnit test inventory does not close")
    paths = sorted(pathlib.Path(directory).glob("cost-*.json"))
    if not paths:
        raise ValueError(
            "no cost records: run with NJUTEST_TEST_COST_DIR and nextest labels"
        )
    for path in paths:
        record = json.loads(path.read_text(), object_pairs_hook=unique)
        if record["schema"] == "njutest-test-cost-v1":
            raise ValueError(
                f"{path}: a v1 record never measured per-key multiplicity; re-measure under the v2 accounting"
            )
        if record["schema"] != "njutest-test-cost-v2":
            raise ValueError(f"{path}: unknown cost schema")
        key = (record["binary"], record["test"])
        if key not in tests:
            raise ValueError(f"{path}: record does not belong to this suite: {key}")
        row = tests[key]
        work = record["work"]
        if work["error"] is not None:
            raise ValueError(f"{path}: {work['error']}")
        row["records"] += 1
        for name in WORK_NUMBERS:
            if name not in work:
                raise ValueError(f"{path}: work count {name} is absent")
            row[name] += number(work[name], f"{path}:{name}")
        if "platform_requests" not in work:
            raise ValueError(f"{path}: platform request count is absent")
        row["module_requests"] += number(
            work["platform_requests"], f"{path}:platform_requests"
        )
        unobserved = work.get("unobserved_cargo")
        if (
            not isinstance(unobserved, list)
            or not unobserved
            or any(not isinstance(name, str) or not name for name in unobserved)
        ):
            raise ValueError(
                f"{path}: the cargo inventory does not name its unobserved command classes"
            )
        if not isinstance(work["build_keys"], dict):
            raise ValueError(f"{path}: content-addressed build keys are absent")
        for key_identity, held in work["build_keys"].items():
            if (
                not isinstance(key_identity, str)
                or len(key_identity) != 64
                or not all(char in "0123456789abcdef" for char in key_identity)
            ):
                raise ValueError(f"{path}: invalid content-addressed build key")
            parsed = key_work(held, f"{path}:{key_identity[:8]}")
            merge_key(
                row["build_keys"].setdefault(
                    key_identity,
                    {
                        "requests": 0,
                        "hits": 0,
                        "misses": 0,
                        "processes": 0,
                        "reasons": {},
                        "refused_writes": {},
                    },
                ),
                parsed,
            )
        if not isinstance(work["unbound"], dict):
            raise ValueError(f"{path}: unbound build identities are absent")
        for identity, held in work["unbound"].items():
            if not isinstance(identity, str) or not (
                identity.startswith("unbound: ") or identity.startswith("direct: ")
            ):
                raise ValueError(
                    f"{path}: an unbound identity without its explicit reason: {identity!r}"
                )
            merge_unbound(
                row["unbound"].setdefault(
                    identity, {"requests": 0, "misses": 0, "processes": 0}
                ),
                unbound_work(held, identity, f"{path}:{identity}"),
            )
        attributed = (
            sum(held["processes"] for held in row["build_keys"].values())
            + sum(held["processes"] for held in row["unbound"].values())
            + row["direct_commands"]
            + row["cargo_test_processes"]
        )
        if attributed != row["builds"]:
            raise ValueError(
                f"{path}: the process inventory does not close: {row['builds']} builds against {attributed} attributed"
            )
        requested = sum(held["requests"] for held in row["build_keys"].values()) + sum(
            held["requests"] for held in row["unbound"].values()
        )
        if requested != row["build_requests"]:
            raise ValueError(f"{path}: the request inventory does not close")
        if (
            sum(held["hits"] for held in row["build_keys"].values())
            != row["build_hits"]
        ):
            raise ValueError(f"{path}: the hit inventory does not close")
        missed = sum(held["misses"] for held in row["build_keys"].values()) + sum(
            held["misses"] for held in row["unbound"].values()
        )
        if missed != row["build_misses"]:
            raise ValueError(f"{path}: the miss inventory does not close")
        modules = work["platform"]
        if record["sealed"] is not None:
            row["module_requests"] += number(
                record["sealed"]["compiles"], f"{path}:bench_modules"
            )
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
                row["compile_ns"] += number(
                    compilation["duration_ns"], f"{path}:duration_ns"
                )
            if "execution_ns" in sealed:
                row["execution_ns"] += number(
                    sealed["execution_ns"], f"{path}:execution_ns"
                )
            elif sealed["instances"]:
                raise ValueError(f"{path}: executed instance timing is absent")
    for row in tests.values():
        if require_pass and row["preparations"] > row["module_requests"]:
            raise ValueError(
                f"{row['binary']}::{row['test']}: module preparations exceed recorded requests"
            )
        if require_pass:
            refuse_redundancy(row, f"{row['binary']}::{row['test']}")
        binaries[row["binary"]]["records"] += row["records"]
        for name in WORK_NUMBERS + ("module_requests",):
            binaries[row["binary"]][name] += row[name]
        for key_identity, held in row["build_keys"].items():
            merge_key(
                binaries[row["binary"]]["build_keys"].setdefault(
                    key_identity,
                    {
                        "requests": 0,
                        "hits": 0,
                        "misses": 0,
                        "processes": 0,
                        "reasons": {},
                        "refused_writes": {},
                    },
                ),
                held,
            )
        for identity, held in row["unbound"].items():
            merge_unbound(
                binaries[row["binary"]]["unbound"].setdefault(
                    identity, {"requests": 0, "misses": 0, "processes": 0}
                ),
                held,
            )
    return {
        "schema": "njutest-suite-cost-v2",
        "platform": platform.system(),
        "wall_seconds": float(suite.attrib["time"]),
        "failures": int(suite.attrib["failures"]),
        "errors": int(suite.attrib["errors"]),
        "tests": list(tests.values()),
        "binaries": dict(binaries),
        "toolchain_concurrency": concurrency(tests.values()),
    }


def concurrency(rows):
    """Recover actual toolchain overlap from JUnit's per-test start instants and durations."""
    events = []
    for row in rows:
        if "::toolchain_" not in row["binary"] or not row["seconds"]:
            continue
        if "timestamp" not in row:
            return None
        start = datetime.datetime.fromisoformat(row["timestamp"]).timestamp()
        events.extend(((start, 1), (start + row["seconds"], -1)))
    if not events:
        return None
    running = peak = 0
    levels = collections.defaultdict(float)
    previous = min(at for at, _ in events)
    for at, change in sorted(events):
        levels[running] += at - previous
        running += change
        if running < 0:
            raise ValueError("toolchain concurrency accounting went negative")
        peak = max(peak, running)
        previous = at
    if running:
        raise ValueError("toolchain concurrency accounting did not close")
    active = sum(seconds for count, seconds in levels.items() if count)
    return {
        "peak": peak,
        "seconds_at_concurrency": dict(levels),
        "active_seconds": active,
        "mean_when_active": sum(count * seconds for count, seconds in levels.items())
        / active,
    }


def budget(report):
    """Count actual build processes and module preparations so a warm cache cannot hide new work."""
    result = {
        binary: {
            name: row[name]
            for name in (
                "tests",
                "records",
                "builds",
                "build_requests",
                "module_requests",
                "misses",
            )
        }
        for binary, row in sorted(report["binaries"].items())
        if "::toolchain_" in binary
    }
    for binary in result:
        row = report["binaries"][binary]
        result[binary]["builds"] = row["builds"]
        result[binary]["misses"] = row["module_requests"]
    direct = {
        "njutest::toolchain_build",
        "njutest::toolchain_edits",
        "rust-mutants::toolchain_cargo",
        "xtask::toolchain_bundle",
    }
    for binary in direct & result.keys():
        if not result[binary]["builds"] or not result[binary]["records"]:
            raise ValueError(f"{binary}: direct Cargo builds were not measured")
    guests = "rust-mutants-sealed::toolchain_guests"
    if guests in result:
        row = report["binaries"][guests]
        result[guests]["records"] -= row["builds"]
    return result


def growth(actual, expected):
    """Refuse newly unmeasured work, omitted binaries and every increase in compile/build counts."""
    errors = []
    if actual.keys() != expected.keys():
        errors.append(
            f"toolchain binaries changed: added={sorted(actual.keys() - expected.keys())}, missing={sorted(expected.keys() - actual.keys())}"
        )
    for binary in actual.keys() & expected.keys():
        for name in ("tests", "builds", "build_requests", "module_requests", "misses"):
            limit = number(expected[binary][name], f"baseline:{binary}:{name}")
            if actual[binary][name] > limit:
                errors.append(
                    f"{binary}: {name} grew from {limit} to {actual[binary][name]}"
                )
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
    parser.add_argument(
        "--baseline",
        type=pathlib.Path,
        default=pathlib.Path(".config/suite-costs.json"),
    )
    parser.add_argument(
        "--record",
        action="store_true",
        help="establish or replace this platform's reviewed count ledger",
    )
    parser.add_argument("--measure-only", action="store_true")
    args = parser.parse_args()
    report = measured(args.directory, args.junit, require_pass=not args.measure_only)
    output = args.directory / "suite-cost.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"{len(report['tests'])} tests; suite wall {report['wall_seconds']:.3f}s; per-test counts: {output}"
    )
    for binary, row in sorted(
        report["binaries"].items(), key=lambda pair: pair[1]["seconds"], reverse=True
    )[:15]:
        print(
            f"{binary}: {row['seconds']:.3f}s summed, {row['preparations']} modules ({row['hits']} hits/{row['misses']} misses), {row['builds']} Cargo processes/{row['build_requests']} requests ({row['build_hits']} hits), {row['units']} fresh units, compile {row['compile_ns'] / 1e9:.3f}s / execute {row['execution_ns'] / 1e9:.3f}s"
        )
    if args.measure_only:
        return
    if args.record:
        ledger = (
            json.loads(args.baseline.read_text())
            if args.baseline.exists()
            else {"schema": LEDGER_SCHEMA, "platforms": {}}
        )
        ledger["schema"] = LEDGER_SCHEMA
        ledger["platforms"][report["platform"]] = budget(report)
        args.baseline.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n")
        return
    ledger = json.loads(args.baseline.read_text())
    if ledger["schema"] != LEDGER_SCHEMA:
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
