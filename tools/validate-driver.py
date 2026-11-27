#!/usr/bin/env python3
"""Check driver declarations and release assets against the Driver Hub contract.

  validate-driver.py config <driver-config.json>...   declaration: schema and cross-field rules
  validate-driver.py release <dir> [--tag T]           driver-release.json, every asset and the OCI archive
  validate-driver.py fixtures                          run Drivers/schemas/tests/*.cases.json

Needs Python 3.11+ and jsonschema. Exits 1 and prints "<JSON Pointer>: <problem>" lines
on failure. Messages name fields, never input values.
"""

import argparse
import json
from pathlib import Path
import sys

import driverhub_contract as contract

FIXTURES = contract.SCHEMAS / "tests"


def report(label, errors):
    for at, message in errors:
        print(f"{label}{at}: {message}", file=sys.stderr)
    return not errors


def check_configs(paths):
    ok = True
    for path in paths:
        try:
            document = json.loads(Path(path).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            print(f"{path}: unreadable JSON ({type(error).__name__})", file=sys.stderr)
            ok = False
            continue
        if report(f"{path}#", contract.config_errors(document)):
            print(f"{path}: valid Driver Hub declaration")
        else:
            ok = False
    return ok


def check_release(directory, tag):
    try:
        record = contract.verify_release_directory(directory, tag)
    except contract.ContractError as error:
        return report(f"{directory}#", error.errors)
    kind = "prerelease" if contract.is_prerelease(record["version"]) else "stable"
    print(f"{directory}: valid {kind} release {record['driverId']} {record['version']} "
          f"({', '.join(record['image']['platforms'])}, {record['image']['digest']})")
    return True


def run_fixtures():
    cases_run = failures = 0
    for path in sorted(FIXTURES.glob("*.cases.json")):
        suite = json.loads(path.read_text(encoding="utf-8"))
        check = {"driver-config": lambda d, _: contract.config_errors(d),
                 "driver-release": contract.release_errors}[suite["schema"]]
        rules = {}
        for case in suite["cases"]:
            cases_run += 1
            document = contract.merge_patch(suite["base"], case.get("patch", {}))
            errors = check(document, case.get("tag"))
            rules.setdefault(case["rule"], set()).add(case["valid"])
            problem = None
            if case["valid"] and errors:
                problem = f"expected valid, got {errors}"
            elif not case["valid"]:
                if not errors:
                    problem = "expected invalid, but it passed"
                elif not any(at == case["at"] for at, _ in errors):
                    problem = f"expected an error at {case['at']}, got {errors}"
            if problem:
                failures += 1
                print(f"FAIL {path.name}: {case['rule']}: {case.get('note', '')} {problem}", file=sys.stderr)
        incomplete = sorted(rule for rule, seen in rules.items() if seen != {True, False})
        for rule in incomplete:
            failures += 1
            print(f"FAIL {path.name}: rule {rule!r} needs at least one valid and one invalid case", file=sys.stderr)
    if not cases_run:
        print(f"no fixtures found in {FIXTURES}", file=sys.stderr)
        return False
    print(f"{cases_run} fixture cases, {failures} failures")
    return failures == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    config = commands.add_parser("config", help="validate driver-config.json files")
    config.add_argument("paths", nargs="+", type=Path)
    release = commands.add_parser("release", help="validate a directory holding the four release assets")
    release.add_argument("directory", type=Path)
    release.add_argument("--tag", help="the release tag, e.g. drivers/rust-example/v0.4.0")
    commands.add_parser("fixtures", help="run the schema fixtures")
    args = parser.parse_args()
    if args.command == "config":
        ok = check_configs(args.paths)
    elif args.command == "release":
        ok = check_release(args.directory, args.tag)
    else:
        ok = run_fixtures()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
