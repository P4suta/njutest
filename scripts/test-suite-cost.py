#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

"""Behavioral checks for growth, incomplete observations and foreign suite records."""

import json
import pathlib
import runpy
import tempfile
import unittest

COST = runpy.run_path(str(pathlib.Path(__file__).with_name("suite-cost.py")))


class SuiteCost(unittest.TestCase):
    def test_concurrency_closes_started_and_finished_tests(self):
        rows = [{"binary": "engine::toolchain_fixture", "seconds": 2,
            "timestamp": f"2026-10-01T00:00:0{second}+00:00"} for second in (0, 1)]
        measured = COST["concurrency"](rows)
        self.assertEqual(measured["peak"], 2)
        self.assertEqual(measured["active_seconds"], 3)
        self.assertAlmostEqual(measured["mean_when_active"], 4 / 3)

    def test_cache_hits_cannot_hide_extra_build_requests_or_module_compilations(self):
        before = {"engine::toolchain_fixture": {"tests": 1, "records": 1,
            "builds": 0, "build_requests": 3, "module_requests": 2, "misses": 0}}
        for changed in ("build_requests", "misses"):
            after = json.loads(json.dumps(before))
            after["engine::toolchain_fixture"][changed] += 1
            self.assertTrue(any(changed in error for error in COST["growth"](after, before)))

    def test_direct_compiler_binaries_cannot_establish_silent_zero_build_budgets(self):
        for binary in ("njutest::toolchain_build", "njutest::toolchain_edits", "rust-mutants::toolchain_cargo", "xtask::toolchain_bundle"):
            report = {"binaries": {binary: {"tests": 1, "records": 0, "builds": 0, "build_requests": 0, "cold_builds": 0, "module_requests": 0, "misses": 0}}}
            with self.assertRaises(ValueError):
                COST["budget"](report)

    def test_a_warm_cache_does_not_hide_new_module_requests_or_build_calls(self):
        before = {"engine::toolchain_fixture": {"tests": 1, "records": 1, "builds": 3, "build_requests": 3, "cold_builds": 3, "module_requests": 2, "misses": 2}}
        for changed in ("builds", "module_requests"):
            after = json.loads(json.dumps(before))
            after["engine::toolchain_fixture"][changed] += 1
            errors = COST["growth"](after, before)
            self.assertTrue(any(changed in error for error in errors))

    def test_a_missing_binary_or_cost_record_is_refused(self):
        before = {"engine::toolchain_fixture": {"tests": 1, "records": 1, "builds": 3, "build_requests": 3, "cold_builds": 3, "module_requests": 2, "misses": 2}}
        self.assertTrue(COST["growth"]({}, before))
        after = {"engine::toolchain_fixture": {"tests": 1, "records": 0, "builds": 0, "build_requests": 0, "cold_builds": 0, "module_requests": 0, "misses": 0}}
        self.assertTrue(COST["growth"](after, before))

    def test_cold_guest_builds_can_warm_but_extra_builds_cannot_disappear(self):
        binary = "rust-mutants-sealed::toolchain_guests"

        def report(builds):
            return {"binaries": {binary: {"tests": 1, "records": 1 + builds,
                "builds": builds, "build_requests": 3, "cold_builds": max(3, builds), "module_requests": 2, "misses": 0}}}

        before = COST["budget"](report(3))
        for builds in (0, 1, 2, 3):
            self.assertFalse(COST["growth"](COST["budget"](report(builds)), before))
        self.assertTrue(COST["growth"](COST["budget"](report(4)), before))

    def test_an_incomplete_record_cannot_underreport_a_passed_suites_cost(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            junit = root / "suite.xml"
            junit.write_text('<testsuites tests="1" failures="0" errors="0" time="1"><testsuite name="engine::toolchain_fixture"><testcase name="fixture" time="1"/></testsuite></testsuites>')
            (root / "cost-incomplete.json").write_text('{"schema":')
            with self.assertRaises(ValueError):
                COST["measured"](root, junit)

    def test_another_suite_cannot_supply_this_suites_missing_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            junit = root / "suite.xml"
            junit.write_text('<testsuites tests="1" failures="0" errors="0" time="1"><testsuite name="engine::toolchain_fixture"><testcase name="fixture" time="1"/></testsuite></testsuites>')
            (root / "cost-foreign.json").write_text(json.dumps({"schema": "njutest-test-cost-v1", "binary": "engine::toolchain_other", "test": "fixture"}))
            with self.assertRaises(ValueError):
                COST["measured"](root, junit)

    def test_a_probe_cannot_hide_more_preparations_than_requested_modules(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            junit = root / "suite.xml"
            junit.write_text('<testsuites tests="1" failures="0" errors="0" time="1"><testsuite name="engine::toolchain_fixture"><testcase name="fixture" time="1"/></testsuite></testsuites>')
            (root / "cost-probe.json").write_text(json.dumps({
                "schema": "njutest-test-cost-v1", "binary": "engine::toolchain_fixture", "test": "fixture",
                "work": {"error": None, "builds": 0, "build_ms": 0, "units": 0, "platform_requests": 1,
                    "build_requests": 0, "build_hits": 0, "build_misses": 0, "uncacheable": 0, "build_keys": [],
                    "platform": [{"compiles": 2, "instances": 0,
                        "compilation": {"hits": 0, "misses": 2, "duration_ns": 1}}]}, "sealed": None,
            }))
            with self.assertRaises(ValueError):
                COST["measured"](root, junit)


if __name__ == "__main__":
    unittest.main()
