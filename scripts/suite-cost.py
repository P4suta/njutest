#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Record the complete suite's timings and engine work, and refuse growth in compile/build counts."""

import argparse
import os
import tempfile
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
    "launch_failures",
    "observed_cargo_starts",
    "cargo_probes",
    "cargo_probe_ms",
    "cargo_metadata",
    "cargo_metadata_ms",
    "rustc_probes",
    "rustc_probe_ms",
)
INVENTORY_NUMBERS = (
    "launch_failures",
    "observed_cargo_starts",
    "cargo_probes",
    "cargo_probe_ms",
    "cargo_metadata",
    "cargo_metadata_ms",
    "rustc_probes",
    "rustc_probe_ms",
)
TOTAL_NUMBERS = WORK_NUMBERS + (
    "records",
    "preparations",
    "instances",
    "hits",
    "misses",
    "compile_ns",
    "execution_ns",
    "module_requests",
)
MISS_CLASSES = ("cold", "repair")

GAPS = ["module keys", "host waits", "resource meters"]


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


def within(label, value):
    """One count boundary for every aggregate: no summed total leaves the u64 width a record could measure."""
    if type(value) is not int or value < 0 or value > 2**64 - 1:
        raise ValueError(
            f"{label}: the aggregate count {value!r} is outside the u64 width a record could measure"
        )
    return value


def summed(label, values):
    """Every derived count sum — outputs and predicates alike — stays inside the width a record could measure."""
    return within(label, sum(values))


def audit(label, row):
    """Check every aggregate count of one holder: its fields, its keys and its reasons."""
    for name in TOTAL_NUMBERS:
        if name in row:
            within(f"{label}:{name}", row[name])
    for key, held in row.get("build_keys", {}).items():
        for name in ("requests", "hits", "misses", "processes", "failed_launches"):
            within(f"{label}:key:{key[:8]}:{name}", held[name])
        for reason, count in held["launch_causes"].items():
            within(f"{label}:key:{key[:8]}:launch_cause:{reason}", count)
        for reason, count in held["reasons"].items():
            within(f"{label}:key:{key[:8]}:reason:{reason}", count)
        for reason, count in held["refused_writes"].items():
            within(f"{label}:key:{key[:8]}:refused:{reason}", count)
    for identity, held in row.get("unbound", {}).items():
        for name in ("requests", "misses", "processes", "failed_launches"):
            within(f"{label}:unbound:{identity}:{name}", held[name])
        for reason, count in held["launch_causes"].items():
            within(f"{label}:unbound:{identity}:launch_cause:{reason}", count)


def key_work(value, label):
    """Read one bound key's multiplicity: every request, process, hit, miss and concrete reason."""
    if not isinstance(value, dict):
        raise ValueError(f"{label}: a bound key's work is not a record")
    for name in ("requests", "hits", "misses", "processes", "failed_launches"):
        if name not in value:
            raise ValueError(f"{label}: bound key count {name} is absent")
        number(value[name], f"{label}:{name}")
    if "launch_causes" not in value:
        raise ValueError(f"{label}: bound key launch_causes are absent")
    for name in ("reasons", "refused_writes", "launch_causes"):
        if not isinstance(value[name], dict):
            raise ValueError(
                f"{label}: bound key {name} are not a mapping of concrete causes"
            )
        for reason, count in value[name].items():
            if not isinstance(reason, str) or not reason.strip():
                raise ValueError(f"{label}:{name}: a cause without its concrete reason")
            number(count, f"{label}:{name}:{reason}")
    for reason in value["reasons"]:
        if not any(
            reason.startswith(f"{kind}: ") and reason[len(kind) + 2 :].strip()
            for kind in MISS_CLASSES
        ):
            raise ValueError(
                f"{label}: a miss reason without its stable class and a concrete cause: {reason!r}"
            )
    if (
        summed(f"{label}:hits+misses", (value["hits"], value["misses"]))
        != value["requests"]
    ):
        raise ValueError(f"{label}: requests do not close into hits and misses")
    if (
        summed(
            f"{label}:processes+failures",
            (value["processes"], value["failed_launches"]),
        )
        != value["misses"]
    ):
        raise ValueError(
            f"{label}: misses do not close into started processes and failed launches"
        )
    if summed(f"{label}:reasons", value["reasons"].values()) != value["misses"]:
        raise ValueError(f"{label}: misses without their concrete reasons do not close")
    if (
        summed(f"{label}:launch_causes", value["launch_causes"].values())
        != value["failed_launches"]
    ):
        raise ValueError(
            f"{label}: failed launches without their concrete causes do not close"
        )
    if (
        summed(f"{label}:refused", value["refused_writes"].values())
        > value["processes"]
    ):
        raise ValueError(
            f"{label}: refused writes do not close against the completed processes that could have written one"
        )
    return {
        name: value[name]
        for name in ("requests", "hits", "misses", "processes", "failed_launches")
    } | {
        "reasons": dict(value["reasons"]),
        "refused_writes": dict(value["refused_writes"]),
        "launch_causes": dict(value["launch_causes"]),
    }


def unbound_work(value, identity, label):
    """Read one unbound identity's multiplicity, closed by whether the cache was ever asked."""
    if not isinstance(value, dict):
        raise ValueError(f"{label}: an unbound identity's work is not a record")
    for name in ("requests", "misses", "processes", "failed_launches", "launch_causes"):
        if name not in value:
            raise ValueError(f"{label}: unbound count {name} is absent")
    for name in ("requests", "misses", "processes", "failed_launches"):
        number(value[name], f"{label}:{name}")
    if not isinstance(value["launch_causes"], dict):
        raise ValueError(
            f"{label}: unbound launch_causes are not a mapping of concrete causes"
        )
    for reason, count in value["launch_causes"].items():
        if not isinstance(reason, str) or not reason.strip():
            raise ValueError(
                f"{label}:launch_causes: a cause without its concrete reason"
            )
        number(count, f"{label}:launch_causes:{reason}")
    started = summed(
        f"{label}:processes+failures", (value["processes"], value["failed_launches"])
    )
    if value["requests"] != started:
        raise ValueError(
            f"{label}: unbound requests do not close into started processes and failed launches"
        )
    expected_misses = value["requests"] if identity.startswith("unbound") else 0
    if value["misses"] != expected_misses:
        raise ValueError(f"{label}: unbound misses do not close")
    if (
        summed(f"{label}:launch_causes", value["launch_causes"].values())
        != value["failed_launches"]
    ):
        raise ValueError(
            f"{label}: failed launches without their concrete causes do not close"
        )
    return {
        name: value[name]
        for name in ("requests", "misses", "processes", "failed_launches")
    } | {"launch_causes": dict(value["launch_causes"])}


def merge_key(into, held):
    """Sum one key's multiplicity across every record of a test, never dropping a repeat."""
    for name in ("requests", "hits", "misses", "processes", "failed_launches"):
        into[name] += held[name]
    for reason, count in held["reasons"].items():
        into["reasons"][reason] = into["reasons"].get(reason, 0) + count
    for reason, count in held["refused_writes"].items():
        into["refused_writes"][reason] = into["refused_writes"].get(reason, 0) + count
    for reason, count in held["launch_causes"].items():
        into["launch_causes"][reason] = into["launch_causes"].get(reason, 0) + count


def merge_unbound(into, held):
    """Sum one unbound identity's multiplicity."""
    for name in ("requests", "misses", "processes", "failed_launches"):
        into[name] += held[name]
    for reason, count in held["launch_causes"].items():
        into["launch_causes"][reason] = into["launch_causes"].get(reason, 0) + count


def empty_key():
    return {
        "requests": 0,
        "hits": 0,
        "misses": 0,
        "processes": 0,
        "failed_launches": 0,
        "reasons": {},
        "refused_writes": {},
        "launch_causes": {},
    }


def empty_unbound():
    return {
        "requests": 0,
        "misses": 0,
        "processes": 0,
        "failed_launches": 0,
        "launch_causes": {},
    }


def suite_redundancy(inventory):
    """A repeated normal cold build is redundant suite-wide; a v2/v3 record proves no causal refusal excuse."""
    errors = []
    for key, held in sorted(inventory.items()):
        cold = summed(
            f"redundancy:{key[:8]}:cold",
            (
                count
                for reason, count in held["reasons"].items()
                if reason.startswith("cold:")
            ),
        )
        if cold < 2:
            continue
        refused = summed(
            f"redundancy:{key[:8]}:refused", held["refused_writes"].values()
        )
        causes = "; ".join(
            f"{reason} x{count}"
            for reason, count in sorted(held["refused_writes"].items())
        )
        explanation = (
            f", beside {refused} refused record writes ({causes}) whose causal proof the record cannot supply"
            if refused
            else ""
        )
        errors.append(
            f"redundant cold builds of one bound input: {key} built cold {cold} times across the suite{explanation}"
        )
    return errors


def destination_conflict(output, inputs):
    """Why `output` may not be written, by canonical or physical identity with any input."""
    resolved_out = os.path.realpath(output)
    for path in inputs:
        resolved = os.path.realpath(path)
        if resolved_out == resolved:
            return f"the destination {output} is the input {path}"
    for path in inputs:
        try:
            out_stat = os.stat(output)
            in_stat = os.stat(path)
        except FileNotFoundError:
            continue
        except OSError as failure:
            return (
                f"the identity of the destination {output} or the input {path} "
                f"is not established ({failure}); refusing rather than guess"
            )
        if (out_stat.st_dev, out_stat.st_ino) == (in_stat.st_dev, in_stat.st_ino):
            return f"the destination {output} is one physical file with the input {path}"
    return None


def publish_atomically(destination, write):
    """Replaces `destination` through a staged file, so a failed write preserves the old one."""
    staged = tempfile.NamedTemporaryFile(
        mode="w", dir=str(destination.parent), prefix=".published-", delete=False
    )
    try:
        with staged:
            write(staged)
        os.replace(staged.name, destination)
    except BaseException:
        try:
            os.unlink(staged.name)
        except OSError:
            pass
        raise


def publish_report(output, report):
    """Writes the reviewable diagnostic report to its own destination, atomically."""
    publish_atomically(output, lambda staged: (json.dump(report, staged, indent=2), staged.write("\n")))


def publish_ledger(baseline, ledger):
    """Replaces the baseline atomically, so a failed write preserves the old one."""
    publish_atomically(
        baseline, lambda staged: (json.dump(ledger, staged, indent=2, sort_keys=True), staged.write("\n"))
    )


def protected_inputs(directory, junit):
    """Every measured byte a publication may not touch: the records, the machine note and the JUnit."""
    inputs = list(sorted([*directory.glob("cost-*.json"), *directory.glob("host-work-*.json")]))
    machine = directory / "machine.json"
    if machine.exists():
        inputs.append(machine)
    inputs.append(junit)
    return inputs


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
    coverage_gaps = []
    observations = observed_work(pathlib.Path(directory), tests.keys())
    paths = sorted(pathlib.Path(directory).glob("cost-*.json"))
    if not paths:
        raise ValueError(
            "no cost records: run with NJUTEST_TEST_COST_DIR and nextest labels"
        )
    unobserved = set()
    for path in paths:
        record = json.loads(path.read_text(), object_pairs_hook=unique)
        if record["schema"] in ("njutest-test-cost-v1", "njutest-test-cost-v2"):
            raise ValueError(
                f"{path}: a {record['schema']} record never measured launch provenance or cargo roles; re-measure under the v3 accounting"
            )
        if record["schema"] != "njutest-test-cost-v3":
            raise ValueError(f"{path}: unknown cost schema")
        if "origin" in record and record["origin"]["kind"] == "product":
            continue
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
        declared = work.get("unobserved_cargo")
        if (
            not isinstance(declared, list)
            or not declared
            or any(not isinstance(name, str) or not name for name in declared)
        ):
            raise ValueError(
                f"{path}: the cargo inventory does not name its unobserved command classes"
            )
        unobserved.update(declared)
        if not isinstance(work["build_keys"], dict):
            raise ValueError(f"{path}: content-addressed build keys are absent")
        for key_identity, held in work["build_keys"].items():
            if (
                not isinstance(key_identity, str)
                or len(key_identity) != 64
                or not all(char in "0123456789abcdef" for char in key_identity)
            ):
                raise ValueError(f"{path}: invalid content-addressed build key")
            merge_key(
                row["build_keys"].setdefault(key_identity, empty_key()),
                key_work(held, f"{path}:{key_identity[:8]}"),
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
                row["unbound"].setdefault(identity, empty_unbound()),
                unbound_work(held, identity, f"{path}:{identity}"),
            )
        audit(f"{path}:{record['binary']}::{record['test']}", row)
        attributed = summed(
            f"{path}:processes",
            [
                *(held["processes"] for held in row["build_keys"].values()),
                *(held["processes"] for held in row["unbound"].values()),
            ],
        )
        if attributed != row["builds"]:
            raise ValueError(
                f"{path}: the process inventory does not close: {row['builds']} builds against {attributed} attributed"
            )
        requested = summed(
            f"{path}:requests",
            [
                *(held["requests"] for held in row["build_keys"].values()),
                *(held["requests"] for held in row["unbound"].values()),
            ],
        )
        if requested != row["build_requests"]:
            raise ValueError(f"{path}: the request inventory does not close")
        hits = summed(
            f"{path}:hits", (held["hits"] for held in row["build_keys"].values())
        )
        if hits != row["build_hits"]:
            raise ValueError(f"{path}: the hit inventory does not close")
        missed = summed(
            f"{path}:misses",
            [
                *(held["misses"] for held in row["build_keys"].values()),
                *(held["misses"] for held in row["unbound"].values()),
            ],
        )
        if missed != row["build_misses"]:
            raise ValueError(f"{path}: the miss inventory does not close")
        failed = summed(
            f"{path}:launch_failures",
            [
                *(held["failed_launches"] for held in row["build_keys"].values()),
                *(held["failed_launches"] for held in row["unbound"].values()),
            ],
        )
        if failed != row["launch_failures"]:
            raise ValueError(f"{path}: the launch failure inventory does not close")
        roles = summed(
            f"{path}:roles", [attributed, row["cargo_probes"], row["cargo_metadata"]]
        )
        if row["observed_cargo_starts"] > roles:
            gap = (
                f"{record['binary']}::{record['test']}: the cargo coverage does not close: "
                f"{row['observed_cargo_starts']} observed starts against {roles} role-attributed"
            )
            if require_pass:
                raise ValueError(f"{path}: {gap}")
            coverage_gaps.append(gap)
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
                if (
                    summed(f"{path}:module-hits+misses", (hits, misses))
                    != sealed["compiles"]
                ):
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
        audit(f"{path}:{record['binary']}::{record['test']}", row)
    inventory = {}
    suite_unbound = {}
    totals = {name: 0 for name in TOTAL_NUMBERS}
    totals["tests"] = len(tests)
    for row in tests.values():
        if require_pass and row["preparations"] > row["module_requests"]:
            raise ValueError(
                f"{row['binary']}::{row['test']}: module preparations exceed recorded requests"
            )
        binaries[row["binary"]]["records"] += row["records"]
        for name in WORK_NUMBERS + (
            "module_requests",
            "preparations",
            "instances",
            "hits",
            "misses",
            "compile_ns",
            "execution_ns",
        ):
            binaries[row["binary"]][name] += row[name]
            totals[name] += row[name]
        totals["records"] += row["records"]
        for key_identity, held in row["build_keys"].items():
            merge_key(
                binaries[row["binary"]]["build_keys"].setdefault(
                    key_identity, empty_key()
                ),
                held,
            )
            merge_key(inventory.setdefault(key_identity, empty_key()), held)
        for identity, held in row["unbound"].items():
            merge_unbound(
                binaries[row["binary"]]["unbound"].setdefault(
                    identity, empty_unbound()
                ),
                held,
            )
            merge_unbound(suite_unbound.setdefault(identity, empty_unbound()), held)
    for binary, row in binaries.items():
        audit(f"binary:{binary}", row)
    for name in TOTAL_NUMBERS:
        within(f"suite:{name}", totals[name])
    audit("suite", {"build_keys": inventory, "unbound": suite_unbound, **totals})
    attributed = summed(
        "suite:processes",
        [
            *(held["processes"] for held in inventory.values()),
            *(held["processes"] for held in suite_unbound.values()),
        ],
    )
    if attributed != totals["builds"]:
        raise ValueError(
            f"the suite process inventory does not close: {totals['builds']} builds against {attributed} attributed"
        )
    failures = summed(
        "suite:launch_failures",
        [
            *(held["failed_launches"] for held in inventory.values()),
            *(held["failed_launches"] for held in suite_unbound.values()),
        ],
    )
    if failures != totals["launch_failures"]:
        raise ValueError("the suite launch failure inventory does not close")
    suite_roles = summed(
        "suite:roles", [attributed, totals["cargo_probes"], totals["cargo_metadata"]]
    )
    if totals["observed_cargo_starts"] > suite_roles and not coverage_gaps:
        coverage_gaps.append(
            f"the suite cargo coverage does not close: {totals['observed_cargo_starts']} observed starts against {suite_roles} role-attributed"
        )
    violations = suite_redundancy(inventory)
    if require_pass and violations:
        raise ValueError("suite work is redundant:\n" + "\n".join(violations))
    return {
        "schema": "njutest-suite-cost-v3",
        "observations": observations,
        "platform": platform.system(),
        "wall_seconds": float(suite.attrib["time"]),
        "failures": int(suite.attrib["failures"]),
        "errors": int(suite.attrib["errors"]),
        "tests": list(tests.values()),
        "binaries": dict(binaries),
        "build_keys": inventory,
        "unbound": suite_unbound,
        "totals": totals,
        "redundancy": violations,
        "coverage_gaps": coverage_gaps,
        "unobserved_cargo": sorted(unobserved),
        "gaps": list(GAPS),
        "toolchain_concurrency": concurrency(tests.values()),
    }



def observed_work(directory, suite_keys=None):
    """Retain actual origins, invocation/module keys, waits and executing hosts for the one work gate."""
    origins = {}
    receipts = []
    gaps = set()
    for path in sorted([*directory.glob("cost-*.json"), *directory.glob("host-work-*.json")]):
        record = json.loads(path.read_text(), object_pairs_hook=unique)
        if "origin" not in record or "machine" not in record:
            gaps.add(f"{path.name}: actual origin or executing machine is absent")
            continue
        origin = record["origin"]
        if origin["kind"] == "suite":
            pair = (origin["binary"], origin["test"])
            if suite_keys is not None and pair not in suite_keys:
                raise ValueError(f"{path}: actual suite origin is outside its JUnit inventory: {pair}")
            name = f"suite:{pair[0]}::{pair[1]}"
        elif origin["kind"] == "product":
            if not origin["program"] or not isinstance(origin["command"], list) or not origin["command"]:
                raise ValueError(f"{path}: actual product command is absent")
            name = f"product:{origin['program']}"
        else:
            raise ValueError(f"{path}: unknown actual work origin")
        machine = record["machine"]
        if not machine["os"] or number(machine["cpus"], f"{path}:cpus") == 0:
            raise ValueError(f"{path}: executing-machine observations are incomplete")
        held = origins.setdefault(name, {kind: {} for kind in ("executions", "probes", "modules", "waits")})
        receipts.append({"path": str(path), "origin": origin, "machine": machine, "record": record})
        if record["schema"] == "njutest-host-work-v1":
            invocation = record["invocation"]
            role = invocation["role"]
            counted(held["executions"], role, 1)
            if record["launch"]["kind"] == "unobserved":
                gaps.add(f"{path.name}: {record['launch']['reason']}")
            waits = record["waits"]
        elif record["schema"] == "njutest-test-cost-v3":
            work = record["work"]
            for kind in ("executions", "probes"):
                if kind not in work:
                    gaps.add(f"{path.name}: keyed {kind} are absent")
                    continue
                for identity, actual in work[kind].items():
                    if not identity or actual["requests"] != actual["processes"] + actual["failed_launches"]:
                        raise ValueError(f"{path}: actual {kind} requests do not close: {identity}")
                    counted(held[kind], actual["role"], number(actual["requests"], f"{path}:{kind}:{identity}"))
            if "host_waits" not in work:
                gaps.add(f"{path.name}: host waits are absent")
                waits = []
            else:
                waits = work["host_waits"]
            modules = list(work["platform"])
            if record["sealed"] is not None:
                modules.append(record["sealed"])
            for observation in modules:
                if "modules" not in observation:
                    gaps.add(f"{path.name}: physical module keys are absent")
                    continue
                for identity, actual in observation["modules"].items():
                    if not actual["module"] or not actual["configuration"]:
                        raise ValueError(f"{path}: physical module/configuration identity is absent")
                    counted(held["modules"], identity, number(actual["requests"], f"{path}:module:{identity}"))
            gaps.update(work["unobserved_cargo"])
        else:
            raise ValueError(f"{path}: unsupported actual-work schema {record['schema']}")
        for wait in waits:
            if not wait["owner"] or not wait["cause"] or not wait["machine"]["os"] or not wait["machine"]["cpus"]:
                raise ValueError(f"{path}: actual host-wait identity is incomplete")
            number(wait["elapsed_ns"], f"{path}:wait duration")
            counted(held["waits"], wait["cause"], 1)
    return {"counts": origins, "receipts": receipts, "gaps": sorted(gaps)}


def counted(mapping, key, amount):
    """Count an actual observed operation without assigning an absent field a count."""
    previous = mapping[key] if key in mapping else 0
    mapping[key] = summed(f"observed:{key}", (previous, amount))


def observed_growth(actual, expected):
    """Use the same count-growth decision for suite/product executions, probes, physical modules and waits."""
    errors = []
    for origin in actual:
        if origin not in expected:
            errors.append(f"actual work origin added: {origin}")
            continue
        for kind in ("executions", "probes", "modules", "waits"):
            for identity, count in actual[origin][kind].items():
                if identity not in expected[origin][kind]:
                    errors.append(f"{origin}: {kind} work added: {identity}")
                    continue
                limit = number(expected[origin][kind][identity], f"baseline:{origin}:{kind}:{identity}")
                if number(count, f"actual:{origin}:{kind}:{identity}") > limit:
                    errors.append(f"{origin}: {kind}:{identity} grew from {limit} to {count}")
    return errors


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
    binaries = {
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
    for binary in binaries:
        row = report["binaries"][binary]
        binaries[binary]["builds"] = row["builds"]
        binaries[binary]["misses"] = row["module_requests"]
    direct = {
        "njutest::toolchain_build",
        "njutest::toolchain_edits",
        "rust-mutants::toolchain_cargo",
        "xtask::toolchain_bundle",
    }
    for binary in direct & binaries.keys():
        if not binaries[binary]["builds"] or not binaries[binary]["records"]:
            raise ValueError(f"{binary}: direct Cargo builds were not measured")
    guests = "rust-mutants-sealed::toolchain_guests"
    if guests in binaries:
        binaries[guests]["records"] -= report["binaries"][guests]["builds"]
    result = {
        "binaries": binaries,
        "unobserved_cargo": list(report["unobserved_cargo"]),
        "gaps": list(report["gaps"]),
    }
    if "observations" in report:
        result["observations"] = report["observations"]["counts"]
    return result


def growth(actual, expected):
    """Refuse newly unmeasured work, omitted binaries, every count increase and any silently changed observation gap."""
    errors = []
    if "observations" in actual or "observations" in expected:
        if "observations" not in actual or "observations" not in expected:
            errors.append("complete actual work observations are absent; re-measure the complete passed inputs")
        else:
            errors.extend(observed_growth(actual["observations"], expected["observations"]))
    if actual["binaries"].keys() != expected["binaries"].keys():
        errors.append(
            f"toolchain binaries changed: added={sorted(actual['binaries'].keys() - expected['binaries'].keys())}, missing={sorted(expected['binaries'].keys() - actual['binaries'].keys())}"
        )
    for binary in actual["binaries"].keys() & expected["binaries"].keys():
        for name in ("tests", "builds", "build_requests", "module_requests", "misses"):
            limit = number(
                expected["binaries"][binary][name], f"baseline:{binary}:{name}"
            )
            if actual["binaries"][binary][name] > limit:
                errors.append(
                    f"{binary}: {name} grew from {limit} to {actual['binaries'][binary][name]}"
                )
        if (
            actual["binaries"][binary]["records"]
            < expected["binaries"][binary]["records"]
        ):
            errors.append(f"{binary}: cost records disappeared")
        if actual["binaries"][binary]["tests"] != expected["binaries"][binary]["tests"]:
            errors.append(f"{binary}: the complete test inventory changed")
    for name in ("gaps", "unobserved_cargo"):
        if actual.get(name) != expected.get(name):
            errors.append(
                f"the observation {name} changed: recorded={expected.get(name)}, measured={actual.get(name)}; re-record the ledger after the reviewed change"
            )
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
    if args.record and args.measure_only:
        raise ValueError(
            "--record and --measure-only cannot be combined: recording is a strict act"
        )
    output = args.directory / "suite-cost.json"
    measured_inputs = protected_inputs(args.directory, args.junit)
    inputs = [*measured_inputs, args.baseline]
    reason = destination_conflict(output, inputs)
    if reason is None and args.record:
        reason = destination_conflict(args.baseline, measured_inputs)
    if reason is not None:
        raise ValueError(f"destination refused: {reason}")
    report = measured(args.directory, args.junit, require_pass=not args.measure_only)
    if not args.measure_only:
        if args.record:
            missing = []
            if report["unobserved_cargo"]:
                missing.append(
                    f"unobserved cargo classes: {', '.join(report['unobserved_cargo'])}"
                )
            if report["gaps"]:
                missing.append(f"uninstrumented meters: {', '.join(report['gaps'])}")
            if missing:
                raise ValueError(
                    "a baseline cannot certify missing observations: "
                    + "; ".join(missing)
                    + "; record once the producer measures them"
                )
            ledger = (
                json.loads(args.baseline.read_text())
                if args.baseline.exists()
                else {"schema": LEDGER_SCHEMA, "platforms": {}}
            )
            ledger["schema"] = LEDGER_SCHEMA
            ledger["platforms"][report["platform"]] = budget(report)
            publish_ledger(args.baseline, ledger)
        else:
            ledger = json.loads(args.baseline.read_text())
            if ledger["schema"] != LEDGER_SCHEMA:
                raise ValueError("unknown suite cost ledger schema")
            errors = growth(budget(report), ledger["platforms"][report["platform"]])
            if errors:
                raise ValueError("suite work grew:\n" + "\n".join(errors))
    publish_report(output, report)
    print(
        f"{len(report['tests'])} tests; suite wall {report['wall_seconds']:.3f}s; per-test counts: {output}"
    )
    for binary, row in sorted(
        report["binaries"].items(), key=lambda pair: pair[1]["seconds"], reverse=True
    )[:15]:
        print(
            f"{binary}: {row['seconds']:.3f}s summed, {row['preparations']} modules ({row['hits']} hits/{row['misses']} misses), {row['builds']} Cargo processes/{row['build_requests']} requests ({row['build_hits']} hits), {row['units']} fresh units, compile {row['compile_ns'] / 1e9:.3f}s / execute {row['execution_ns'] / 1e9:.3f}s"
        )
    for violation in report["redundancy"]:
        print(f"redundant work, not certified: {violation}")
    for gap in report["coverage_gaps"]:
        print(f"unattributed cargo work, not certified: {gap}")
    if not args.record and not args.measure_only:
        print("suite compile/build counts stayed within the committed ledger")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, ET.ParseError) as error:
        print(f"suite-cost: {error}", file=sys.stderr)
        sys.exit(1)
