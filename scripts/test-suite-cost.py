#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Behavioral checks for growth, incomplete observations and foreign suite records."""

import json
import pathlib
import runpy
import sys
import tempfile
import unittest

COST = runpy.run_path(str(pathlib.Path(__file__).with_name("suite-cost.py")))

KEY = "ab" * 32
OTHER_KEY = "cd" * 32
UNOBSERVED = ["cargo -vV toolchain banners"]


def work(**overrides):
    """One complete v2 work block; a caller names only what its scenario measures."""
    base = {
        "builds": 0,
        "build_ms": 0,
        "units": 0,
        "build_requests": 0,
        "build_hits": 0,
        "build_misses": 0,
        "build_keys": {},
        "unbound": {},
        "direct_commands": 0,
        "cargo_test_processes": 0,
        "cargo_other_processes": 0,
        "unobserved_cargo": list(UNOBSERVED),
        "platform": [],
        "platform_requests": 0,
        "error": None,
    }
    base.update(overrides)
    return base


def key_work(**overrides):
    """One complete bound-key block."""
    base = {
        "requests": 0,
        "hits": 0,
        "misses": 0,
        "processes": 0,
        "reasons": {},
        "refused_writes": {},
    }
    base.update(overrides)
    return base


def write_record(
    root, name, work_block, binary="engine::toolchain_fixture", test="fixture"
):
    (root / f"cost-{name}.json").write_text(
        json.dumps(
            {
                "schema": "njutest-test-cost-v2",
                "binary": binary,
                "test": test,
                "root": str(root),
                "work": work_block,
                "sealed": None,
            }
        )
    )


def write_junit(root, tests=(("engine::toolchain_fixture", "fixture"),)):
    suites = []
    for binary, test in tests:
        if not suites or suites[-1][0] != binary:
            suites.append((binary, []))
        suites[-1][1].append(test)
    body = ""
    for binary, names in suites:
        cases = "".join('<testcase name="%s" time="1"/>' % name for name in names)
        body += '<testsuite name="%s">%s</testsuite>' % (binary, cases)
    root.joinpath("suite.xml").write_text(
        '<testsuites tests="%d" failures="0" errors="0" time="1">%s</testsuites>'
        % (len(tests), body)
    )


def cold(count=1, refused=None):
    """One complete bound key cold-built `count` times, with optional refused record writes."""
    return work(
        builds=count,
        build_requests=count,
        build_misses=count,
        build_keys={
            KEY: key_work(
                requests=count,
                misses=count,
                processes=count,
                reasons={"cold: the compilation record is absent": count},
                refused_writes=refused or {},
            )
        },
    )


def run_main(arguments):
    """Run the reader's command line with these arguments, restoring the interpreter's own."""
    saved = sys.argv
    sys.argv = ["suite-cost.py", *arguments]
    try:
        COST["main"]()
    finally:
        sys.argv = saved


def budgeted(binaries):
    """A complete budget document: binary rows beside the observation gaps they were recorded with."""
    return {
        "binaries": {name: dict(row) for name, row in binaries.items()},
        "unobserved_cargo": list(UNOBSERVED),
        "gaps": list(COST["GAPS"]),
    }


class SuiteCost(unittest.TestCase):
    def test_concurrency_closes_started_and_finished_tests(self):
        rows = [
            {
                "binary": "engine::toolchain_fixture",
                "seconds": 2,
                "timestamp": f"2026-10-01T00:00:0{second}+00:00",
            }
            for second in (0, 1)
        ]
        measured = COST["concurrency"](rows)
        self.assertEqual(measured["peak"], 2)
        self.assertEqual(measured["active_seconds"], 3)
        self.assertAlmostEqual(measured["mean_when_active"], 4 / 3)

    def test_cache_hits_cannot_hide_extra_build_requests_or_module_compilations(self):
        before = budgeted(
            {
                "engine::toolchain_fixture": {
                    "tests": 1,
                    "records": 1,
                    "builds": 0,
                    "build_requests": 3,
                    "module_requests": 2,
                    "misses": 0,
                }
            }
        )
        for changed in ("build_requests", "misses"):
            after = json.loads(json.dumps(before))
            after["binaries"]["engine::toolchain_fixture"][changed] += 1
            self.assertTrue(
                any(changed in error for error in COST["growth"](after, before))
            )

    def test_direct_compiler_binaries_cannot_establish_silent_zero_build_budgets(self):
        for binary in (
            "njutest::toolchain_build",
            "njutest::toolchain_edits",
            "rust-mutants::toolchain_cargo",
            "xtask::toolchain_bundle",
        ):
            report = budgeted(
                {
                    binary: {
                        "tests": 1,
                        "records": 0,
                        "builds": 0,
                        "build_requests": 0,
                        "module_requests": 0,
                        "misses": 0,
                    }
                }
            )
            with self.assertRaises(ValueError):
                COST["budget"](report)

    def test_a_warm_cache_does_not_hide_new_module_requests_or_build_calls(self):
        before = budgeted(
            {
                "engine::toolchain_fixture": {
                    "tests": 1,
                    "records": 1,
                    "builds": 3,
                    "build_requests": 3,
                    "module_requests": 2,
                    "misses": 2,
                }
            }
        )
        for changed in ("builds", "module_requests"):
            after = json.loads(json.dumps(before))
            after["binaries"]["engine::toolchain_fixture"][changed] += 1
            errors = COST["growth"](after, before)
            self.assertTrue(any(changed in error for error in errors))

    def test_a_missing_binary_or_cost_record_is_refused(self):
        before = budgeted(
            {
                "engine::toolchain_fixture": {
                    "tests": 1,
                    "records": 1,
                    "builds": 3,
                    "build_requests": 3,
                    "module_requests": 2,
                    "misses": 2,
                }
            }
        )
        self.assertTrue(COST["growth"](budgeted({}), before))
        after = budgeted(
            {
                "engine::toolchain_fixture": {
                    "tests": 1,
                    "records": 0,
                    "builds": 0,
                    "build_requests": 0,
                    "module_requests": 0,
                    "misses": 0,
                }
            }
        )
        self.assertTrue(COST["growth"](after, before))

    def test_guest_build_multiplicity_is_preserved_without_a_reserved_cold_ceiling(
        self,
    ):
        binary = "rust-mutants-sealed::toolchain_guests"

        def report(builds):
            return budgeted(
                {
                    binary: {
                        "tests": 1,
                        "records": 1 + builds,
                        "builds": builds,
                        "build_requests": builds,
                        "module_requests": 2,
                        "misses": 0,
                    }
                }
            )

        self.assertEqual(COST["budget"](report(1))["binaries"][binary]["builds"], 1)
        before = COST["budget"](report(3))
        for builds in (0, 1, 2, 3):
            self.assertFalse(COST["growth"](COST["budget"](report(builds)), before))
        self.assertTrue(COST["growth"](COST["budget"](report(4)), before))

    def test_an_incomplete_record_cannot_underreport_a_passed_suites_cost(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            (root / "cost-incomplete.json").write_text('{"schema":')
            with self.assertRaises(ValueError):
                COST["measured"](root, root / "suite.xml")

    def test_another_suite_cannot_supply_this_suites_missing_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            (root / "cost-foreign.json").write_text(
                json.dumps(
                    {
                        "schema": "njutest-test-cost-v2",
                        "binary": "engine::toolchain_other",
                        "test": "fixture",
                    }
                )
            )
            with self.assertRaises(ValueError):
                COST["measured"](root, root / "suite.xml")

    def test_a_probe_cannot_hide_more_preparations_than_requested_modules(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "probe",
                work(
                    platform_requests=1,
                    platform=[
                        {
                            "compiles": 2,
                            "instances": 0,
                            "compilation": {"hits": 0, "misses": 2, "duration_ns": 1},
                        }
                    ],
                ),
            )
            with self.assertRaises(ValueError):
                COST["measured"](root, root / "suite.xml")

    def test_a_record_without_multiplicity_cannot_certify_counts_it_never_measured(
        self,
    ):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            (root / "cost-historical.json").write_text(
                json.dumps(
                    {
                        "schema": "njutest-test-cost-v1",
                        "binary": "engine::toolchain_fixture",
                        "test": "fixture",
                        "root": str(root),
                        "work": {
                            "error": None,
                            "builds": 2,
                            "build_ms": 1,
                            "units": 0,
                            "platform_requests": 0,
                            "build_requests": 2,
                            "build_hits": 0,
                            "build_misses": 2,
                            "uncacheable": 0,
                            "build_keys": [KEY],
                            "platform": [],
                        },
                        "sealed": None,
                    }
                )
            )
            with self.assertRaisesRegex(ValueError, "multiplicity"):
                COST["measured"](root, root / "suite.xml")

    def test_split_records_that_cold_build_one_bound_input_twice_are_redundant(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            for name in ("cold-a", "cold-b"):
                write_record(
                    root,
                    name,
                    work(
                        builds=1,
                        build_requests=1,
                        build_misses=1,
                        build_keys={
                            KEY: key_work(
                                requests=1,
                                misses=1,
                                processes=1,
                                reasons={"cold: the compilation record is absent": 1},
                            )
                        },
                    ),
                )
            with self.assertRaisesRegex(ValueError, "redundant"):
                COST["measured"](root, root / "suite.xml")

    def test_key_accounting_that_does_not_close_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "open",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=0,
                    build_keys={
                        KEY: key_work(
                            requests=1,
                            misses=0,
                            processes=1,
                            reasons={"cold: the compilation record is absent": 1},
                        )
                    },
                ),
            )
            with self.assertRaisesRegex(ValueError, "close"):
                COST["measured"](root, root / "suite.xml")
            write_record(
                root,
                "open",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=1,
                    build_hits=1,
                    build_keys={
                        KEY: key_work(
                            requests=1,
                            hits=1,
                            misses=1,
                            processes=1,
                            reasons={"cold: the compilation record is absent": 1},
                        )
                    },
                ),
            )
            with self.assertRaisesRegex(ValueError, "close"):
                COST["measured"](root, root / "suite.xml")

    def test_a_corruption_repair_stays_visible_with_its_concrete_reason(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "repair",
                work(
                    builds=2,
                    build_requests=2,
                    build_misses=2,
                    build_keys={
                        KEY: key_work(
                            requests=2,
                            misses=2,
                            processes=2,
                            reasons={
                                "cold: the compilation record is absent": 1,
                                "repair: the recorded artifact changed": 1,
                            },
                        )
                    },
                ),
            )
            report = COST["measured"](root, root / "suite.xml")
            row = report["tests"][0]
            self.assertEqual(row["build_keys"][KEY]["processes"], 2)
            self.assertEqual(
                row["build_keys"][KEY]["reasons"][
                    "repair: the recorded artifact changed"
                ],
                1,
            )
            self.assertEqual(row["builds"], 2)

    def test_refused_writes_do_not_prove_a_later_cold_build_was_needed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "refused",
                cold(2, {"the record could not be written": 1}),
            )
            with self.assertRaisesRegex(ValueError, "redundant"):
                COST["measured"](root, root / "suite.xml")
            report = COST["measured"](root, root / "suite.xml", require_pass=False)
            self.assertEqual(len(report["redundancy"]), 1)
            self.assertIn("the record could not be written", report["redundancy"][0])
            self.assertIn("causal", report["redundancy"][0])

    def test_an_unbound_command_publishes_its_reason_and_closes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "unbound",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=1,
                    unbound={
                        "unbound: inherited environment": {
                            "requests": 1,
                            "misses": 1,
                            "processes": 1,
                        }
                    },
                ),
            )
            report = COST["measured"](root, root / "suite.xml")
            self.assertEqual(
                report["tests"][0]["unbound"]["unbound: inherited environment"][
                    "processes"
                ],
                1,
            )

    def test_an_unbound_command_whose_counts_do_not_close_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "open-unbound",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=1,
                    unbound={
                        "unbound: inherited environment": {
                            "requests": 1,
                            "misses": 1,
                            "processes": 0,
                        }
                    },
                ),
            )
            with self.assertRaisesRegex(ValueError, "close"):
                COST["measured"](root, root / "suite.xml")

    def test_a_cargo_inventory_without_its_unobserved_classes_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "claimed-complete",
                work(cargo_other_processes=0, unobserved_cargo=[]),
            )
            with self.assertRaisesRegex(ValueError, "unobserved"):
                COST["measured"](root, root / "suite.xml")

    def test_a_direct_command_run_twice_names_its_repetition(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "direct",
                work(
                    builds=2,
                    build_requests=2,
                    unbound={
                        "direct: native edit oracle build": {
                            "requests": 2,
                            "misses": 0,
                            "processes": 2,
                        }
                    },
                ),
            )
            report = COST["measured"](root, root / "suite.xml")
            self.assertEqual(
                report["tests"][0]["unbound"]["direct: native edit oracle build"][
                    "processes"
                ],
                2,
            )

    def test_a_key_cold_built_once_in_each_of_two_tests_is_suite_redundancy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_fixture", "first"),
                    ("engine::toolchain_fixture", "second"),
                ),
            )
            write_record(root, "a", cold(), test="first")
            write_record(root, "b", cold(), test="second")
            with self.assertRaisesRegex(ValueError, "redundant"):
                COST["measured"](root, root / "suite.xml")

    def test_a_key_cold_built_once_in_each_of_two_binaries_is_suite_redundancy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_first", "fixture"),
                    ("engine::toolchain_second", "fixture"),
                ),
            )
            write_record(root, "a", cold(), binary="engine::toolchain_first")
            write_record(root, "b", cold(), binary="engine::toolchain_second")
            with self.assertRaisesRegex(ValueError, "redundant"):
                COST["measured"](root, root / "suite.xml")

    def test_one_refused_write_cannot_excuse_a_third_cold_build(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "excused",
                cold(3, {"the record could not be written": 1}),
            )
            with self.assertRaisesRegex(ValueError, "redundant"):
                COST["measured"](root, root / "suite.xml")

    def test_a_valid_cold_build_then_hits_across_tests_parses(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_fixture", "first"),
                    ("engine::toolchain_fixture", "second"),
                ),
            )
            write_record(root, "a", cold(), test="first")
            write_record(
                root,
                "b",
                work(
                    build_requests=1,
                    build_hits=1,
                    build_keys={KEY: key_work(requests=1, hits=1)},
                ),
                test="second",
            )
            report = COST["measured"](root, root / "suite.xml")
            rows = {row["test"]: row for row in report["tests"]}
            self.assertEqual(rows["first"]["build_keys"][KEY]["misses"], 1)
            self.assertEqual(rows["second"]["build_keys"][KEY]["hits"], 1)
            self.assertEqual(rows["second"]["build_keys"][KEY]["processes"], 0)

    def test_a_miss_without_its_stable_class_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "unclassified",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=1,
                    build_keys={
                        KEY: key_work(
                            requests=1,
                            misses=1,
                            processes=1,
                            reasons={"the record was simply absent": 1},
                        )
                    },
                ),
            )
            with self.assertRaisesRegex(ValueError, "class"):
                COST["measured"](root, root / "suite.xml")

    def test_a_suite_total_beyond_the_u64_width_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_fixture", "first"),
                    ("engine::toolchain_fixture", "second"),
                ),
            )
            half = 2**63
            for name in ("first", "second"):
                write_record(
                    root,
                    name,
                    work(
                        build_requests=half,
                        build_hits=half,
                        build_keys={KEY: key_work(requests=half, hits=half)},
                    ),
                    test=name,
                )
            with self.assertRaisesRegex(ValueError, "u64"):
                COST["measured"](root, root / "suite.xml")

    def test_measure_only_reports_suite_redundancy_without_certifying_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_fixture", "first"),
                    ("engine::toolchain_fixture", "second"),
                ),
            )
            write_record(root, "a", cold(), test="first")
            write_record(root, "b", cold(), test="second")
            report = COST["measured"](root, root / "suite.xml", require_pass=False)
            self.assertEqual(len(report["redundancy"]), 1)
            self.assertIn(KEY[:8], report["redundancy"][0])
            self.assertEqual(
                report["build_keys"][KEY]["reasons"][
                    "cold: the compilation record is absent"
                ],
                2,
            )

    def test_the_ledger_certifies_its_observation_gaps(self):
        report = budgeted(
            {
                "engine::toolchain_fixture": {
                    "tests": 1,
                    "records": 1,
                    "builds": 1,
                    "build_requests": 1,
                    "module_requests": 0,
                    "misses": 0,
                }
            }
        )
        before = COST["budget"](report)
        self.assertEqual(before["gaps"], list(COST["GAPS"]))
        self.assertEqual(before["unobserved_cargo"], list(UNOBSERVED))
        shrunk = json.loads(json.dumps(before))
        shrunk["gaps"] = before["gaps"][:1]
        self.assertTrue(
            any("gaps" in error for error in COST["growth"](shrunk, before))
        )
        claimed = json.loads(json.dumps(before))
        claimed["unobserved_cargo"] = []
        self.assertTrue(
            any("unobserved" in error for error in COST["growth"](claimed, before))
        )


    def test_record_refuses_to_certify_while_observations_are_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(root, "a", cold())
            existing = root / "ledger.json"
            kept = b'{"schema":"njutest-suite-cost-budget-v3","platforms":{}}\n'
            existing.write_bytes(kept)
            with self.assertRaisesRegex(ValueError, "missing observation"):
                run_main(
                    [str(root), str(root / "suite.xml"), "--record", "--baseline", str(existing)]
                )
            self.assertEqual(existing.read_bytes(), kept)
            fresh = root / "absent.json"
            with self.assertRaisesRegex(ValueError, "missing observation"):
                run_main(
                    [str(root), str(root / "suite.xml"), "--record", "--baseline", str(fresh)]
                )
            self.assertFalse(fresh.exists())

    def test_record_and_measure_only_together_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(root, "a", cold())
            with self.assertRaisesRegex(ValueError, "cannot be combined"):
                run_main([str(root), str(root / "suite.xml"), "--record", "--measure-only"])

    def test_duration_totals_roll_up_per_binary_and_suite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(
                root,
                (
                    ("engine::toolchain_fixture", "first"),
                    ("engine::toolchain_fixture", "second"),
                ),
            )
            for name, (compile_ns, execution_ns) in (
                ("first", (100, 50)),
                ("second", (200, 20)),
            ):
                write_record(
                    root,
                    name,
                    work(
                        platform_requests=1,
                        platform=[
                            {
                                "compiles": 1,
                                "instances": 1,
                                "compilation": {
                                    "hits": 0,
                                    "misses": 1,
                                    "duration_ns": compile_ns,
                                },
                                "execution_ns": execution_ns,
                            }
                        ],
                    ),
                    test=name,
                )
            report = COST["measured"](root, root / "suite.xml")
            rows = {row["test"]: row for row in report["tests"]}
            self.assertEqual(rows["first"]["compile_ns"], 100)
            self.assertEqual(rows["second"]["execution_ns"], 20)
            self.assertEqual(
                report["binaries"]["engine::toolchain_fixture"]["compile_ns"], 300
            )
            self.assertEqual(
                report["binaries"]["engine::toolchain_fixture"]["execution_ns"], 70
            )
            self.assertEqual(report["totals"]["compile_ns"], 300)
            self.assertEqual(report["totals"]["execution_ns"], 70)

    def test_a_refusal_sum_beyond_u64_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "overflow",
                cold(),
            )
            path = root / "cost-overflow.json"
            record = json.loads(path.read_text())
            record["work"]["build_keys"][KEY]["refused_writes"] = {
                "first": 2**63,
                "second": 2**63,
            }
            path.write_text(json.dumps(record))
            with self.assertRaisesRegex(ValueError, "u64"):
                COST["measured"](root, root / "suite.xml")

    def test_more_refused_writes_than_processes_do_not_close(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(root, "refusals", cold(refused={"one": 1, "another": 1}))
            with self.assertRaisesRegex(ValueError, "close"):
                COST["measured"](root, root / "suite.xml")

    def test_a_whitespace_only_cause_is_absent_not_concrete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            write_junit(root)
            write_record(
                root,
                "blank",
                work(
                    builds=1,
                    build_requests=1,
                    build_misses=1,
                    build_keys={
                        KEY: key_work(
                            requests=1,
                            misses=1,
                            processes=1,
                            reasons={"cold: \t ": 1},
                        )
                    },
                ),
            )
            with self.assertRaisesRegex(ValueError, "concrete"):
                COST["measured"](root, root / "suite.xml")
            write_record(
                root,
                "blank",
                cold(refused={"   ": 1}),
            )
            with self.assertRaisesRegex(ValueError, "concrete"):
                COST["measured"](root, root / "suite.xml")


if __name__ == "__main__":
    unittest.main()
