"""Checks each driver's tests/declared-values.json against its declaration, and the
check itself. The driver's own test suite runs the same cases through its parser.

    python3 -m unittest discover -s tools/tests
"""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import driverhub_contract as contract  # noqa: E402

URL = {"schemes": ["wss"], "path": "/x-driver/v1", "allowCredentials": False, "allowQuery": False, "allowFragment": False}
DECLARATION = {"schemaVersion": 2, "inputs": [
    {"name": "endpoint", "type": "string", "required": True, "env": "ENDPOINT", "url": URL},
    {"name": "mode", "type": "string", "required": False, "default": "a", "env": "MODE", "enum": ["a", "b"]},
]}
GOOD = [
    {"input": "endpoint", "value": "wss://h.example.test/x-driver/v1", "declared": True, "parser": True},
    {"input": "endpoint", "value": "ws://h.example.test/x-driver/v1", "declared": False, "parser": False},
    {"input": "mode", "value": "b", "declared": True, "parser": True},
    {"input": "mode", "value": "B", "declared": False, "parser": False},
]


def fixture_files():
    listed = subprocess.check_output(["git", "ls-files", "Drivers", "examples"], cwd=contract.ROOT, text=True).split()
    return [contract.ROOT / name for name in listed if name.endswith("/tests/declared-values.json")]


class DeclaredValues(unittest.TestCase):
    def errors(self, cases, declaration=DECLARATION):
        with tempfile.TemporaryDirectory() as scratch:
            driver = Path(scratch)
            (driver / "tests").mkdir()
            (driver / "driver-config.json").write_text(json.dumps(declaration))
            (driver / "tests" / "declared-values.json").write_text(json.dumps({"cases": cases}))
            return contract.declared_values_errors(driver / "tests" / "declared-values.json")

    def test_repository_drivers(self):
        for path in fixture_files():
            with self.subTest(path=path.relative_to(contract.ROOT).as_posix()):
                self.assertEqual(contract.declared_values_errors(path), [])

    def test_agreement(self):
        self.assertEqual(self.errors(GOOD), [])

    def test_case_must_match_the_declaration(self):
        cases = [dict(GOOD[0], declared=False, parser=False)] + GOOD[1:]
        self.assertIn("the declaration accepts this value", str(self.errors(cases)))

    def test_declaration_must_not_accept_what_the_parser_rejects(self):
        cases = [dict(GOOD[0], parser=False)] + GOOD[1:]
        self.assertIn("name the inexpressible rule", str(self.errors(cases)))
        cases[0]["undeclared"] = "hosts under .invalid"
        self.assertEqual(self.errors(cases), [])

    def test_stricter_declaration_needs_a_reason(self):
        cases = GOOD + [{"input": "endpoint", "value": "wss://h.example.test/x-driver/./v1", "declared": False, "parser": True}]
        self.assertIn("say why", str(self.errors(cases)))
        cases[-1]["note"] = "dot segments are not canonical"
        self.assertEqual(self.errors(cases), [])

    def test_every_constrained_input_needs_both_outcomes(self):
        self.assertIn("input 'mode' declares a value constraint", str(self.errors(GOOD[:2])))

    def test_unknown_input(self):
        self.assertIn("names no declared input", str(self.errors(GOOD + [dict(GOOD[0], input="other")])))

    def test_invalid_declaration(self):
        self.assertIn("unsupported schemaVersion", str(self.errors(GOOD, {**DECLARATION, "schemaVersion": 9})))


class ValueRules(unittest.TestCase):
    """Spot checks beyond the shared fixtures in Drivers/schemas/tests."""

    def test_secrets_are_not_checked(self):
        item = {"name": "t", "type": "string", "required": True, "secret": True, "minLength": 5, "env": "T"}
        self.assertEqual(contract.value_errors(item, ""), [])

    def test_unsupported_versions_fail_clearly(self):
        for version in (0, 3, "2", True, 2.0, None):
            with self.subTest(version=version):
                self.assertEqual(contract.config_errors({"schemaVersion": version, "inputs": []}),
                                 [("/schemaVersion", "unsupported schemaVersion; this validator reads 1 and 2")])


if __name__ == "__main__":
    unittest.main()
