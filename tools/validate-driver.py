#!/usr/bin/env python3
"""Check driver declarations and release assets against the Driver Hub contract.

  validate-driver.py directory [--all] [--report] <dir>...  can Driver Hub build and deploy this driver?
  validate-driver.py config <driver-config.json>...        declaration: schema and cross-field rules
  validate-driver.py release <dir> [--tag T]                driver-release.json, every asset and the OCI archive
  validate-driver.py fixtures                               run Drivers/schemas/tests/*.cases.json

driver-config.json may use schemaVersion 1 or 2; each is
validated against its own schema.

Needs Python 3.11+ and jsonschema. Exits 1 and prints "<JSON Pointer>: <problem>" lines
on failure, except with --report. Messages name fields, never input values.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import driverhub_contract as contract

FIXTURES = contract.SCHEMAS / "tests"
CHECKLIST = "Drivers/RELEASING.md#driver-hub-checklist"
V2_STATUS = "Drivers/RELEASING.md#declaration-schemaversion-2"


def driver_directories():
    """Every contributed driver, plus examples that declare Driver Hub inputs."""
    files = subprocess.check_output(["git", "ls-files", "Drivers", "examples"], cwd=contract.ROOT, text=True).split()
    found = set()
    for name in files:
        parts = name.split("/")
        if parts[0] == "Drivers" and len(parts) >= 4 and parts[1] != "schemas" and parts[-1] != ".keep":
            found.add("/".join(parts[:3]))
        if parts[0] == "examples" and len(parts) == 3 and parts[2] == "driver-config.json":
            found.add("/".join(parts[:2]))
    return sorted(found)


def directory_findings(directory):
    """Static Driver Hub checks for one driver directory: (problems, notes)."""
    path = (contract.ROOT / directory).resolve()
    relative = path.relative_to(contract.ROOT).as_posix()
    problems, notes = [], []
    if not contract.BUILD_DIRECTORY.match(relative):
        problems.append(("", "Driver Hub can only build directories named like Drivers/<Vendor>/<Driver> "
                             "(letters, digits, '_', '-' and '.', not starting with '.')"))
    if not (path / "Dockerfile").is_file():
        problems.append(("Dockerfile", "no Dockerfile at the directory root, so Driver Hub cannot build an image from it"))
    config = path / "driver-config.json"
    if not config.is_file():
        problems.append(("driver-config.json", "no driver-config.json, so Driver Hub cannot configure or deploy it"))
    else:
        try:
            document = json.loads(config.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            problems.append(("driver-config.json", f"unreadable JSON ({type(error).__name__})"))
        else:
            errors = contract.config_errors(document)
            problems += [("driver-config.json", f"{at}: {message}") for at, message in errors]
            version = contract.config_version(document)
            if not errors and version not in contract.HUB_READS:
                problems.append(("driver-config.json", f"/schemaVersion: version {version} is valid here, but no Driver Hub "
                                                       f"release reads it yet, so Driver Hub refuses the declaration ({V2_STATUS})"))
            if not errors and version in contract.HUB_READS and version != 1:
                notes.append(("driver-config.json", f"schemaVersion {version} needs a current Driver Hub; "
                                                    f"older releases refuse it ({V2_STATUS})"))
            if not errors and "controlHealth" not in document:
                notes.append(("driver-config.json", "no controlHealth, so Driver Hub shows communication as not observable"))
    return relative, problems, notes


def check_directories(directories, github):
    checklist = CHECKLIST
    if all(os.environ.get(name) for name in ("GITHUB_SERVER_URL", "GITHUB_REPOSITORY", "GITHUB_SHA")):
        checklist = f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}/blob/{os.environ['GITHUB_SHA']}/{CHECKLIST}"
    summary = ["## Driver Hub compatibility", "",
               f"Static checks only, and not required to merge or release. See the [checklist]({checklist}).", "",
               "| Driver directory | Result |", "| --- | --- |"]
    ok = True
    for directory in directories:
        relative, problems, notes = directory_findings(directory)
        ok = ok and not problems
        for file, message in problems + notes:
            level = "warning" if (file, message) in problems else "notice"
            # Annotate the file when it exists; a missing file is reported on its directory.
            where = f"{relative}/{file}" if file and (contract.ROOT / relative / file).exists() else relative
            if github:
                print(f"::{level} file={where},title=Driver Hub::{message}")
            print(f"{where}: {level}: {message}", file=sys.stderr)
        if problems:
            result = "Not deployable by Driver Hub: " + "; ".join(m for _, m in problems)
        else:
            result = "Deployable by Driver Hub (static checks)" + "".join(f"; {m}" for _, m in notes)
            print(f"{relative}: {result}")
        cell = result.replace("|", "\\|")
        summary.append(f"| `{relative}` | {cell} |")
    if not directories:
        summary.append("| (none) | No driver directories found. |")
    if github and os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as stream:
            stream.write("\n".join(summary) + "\n")
    return ok


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


def run_value_fixtures(path, suite):
    """Value cases: each names a declared input, an effective value and the expected
    reason codes (an empty list means accepted). Returns (cases run, failures)."""
    cases_run = failures = 0
    outcomes = {}
    for name, item in suite["inputs"].items():
        declaration = {"schemaVersion": 2, "inputs": [{"name": name, **item}]}
        errors = contract.config_errors(declaration)
        if errors:
            failures += 1
            print(f"FAIL {path.name}: input {name!r} is not a valid declaration: {errors}", file=sys.stderr)
    for case in suite["cases"]:
        cases_run += 1
        item = {"name": case["input"], **suite["inputs"][case["input"]]}
        got = contract.value_errors(item, case["value"])
        expected = sorted(case["errors"])
        outcomes.setdefault(case["input"], set()).add(not expected)
        undocumented = sorted(set(expected + got) - set(suite["reasons"]))
        if undocumented:
            failures += 1
            print(f"FAIL {path.name}: reason codes missing from \"reasons\": {undocumented}", file=sys.stderr)
        if got != expected:
            failures += 1
            print(f"FAIL {path.name}: {case['input']}: {case.get('note', '')} expected {expected}, got {got}", file=sys.stderr)
    for name in suite["inputs"]:
        if outcomes.get(name) != {True, False}:
            failures += 1
            print(f"FAIL {path.name}: input {name!r} needs at least one accepted and one rejected value", file=sys.stderr)
    return cases_run, failures


def run_fixtures():
    cases_run = failures = 0
    for path in sorted(FIXTURES.glob("*.cases.json")):
        suite = json.loads(path.read_text(encoding="utf-8"))
        if suite["schema"] == "driver-config-values":
            ran, failed = run_value_fixtures(path, suite)
            cases_run += ran
            failures += failed
            continue
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
    directory = commands.add_parser("directory", help="report whether Driver Hub can build and deploy drivers")
    directory.add_argument("directories", nargs="*", type=Path)
    directory.add_argument("--all", action="store_true", help="every driver directory in the repository")
    directory.add_argument("--report", action="store_true",
                           help="GitHub annotations and job summary; always exit 0 (informational)")
    args = parser.parse_args()
    if args.command == "directory":
        directories = (driver_directories() if args.all else []) + [str(d) for d in args.directories]
        ok = check_directories(directories, args.report) or args.report
    elif args.command == "config":
        ok = check_configs(args.paths)
    elif args.command == "release":
        ok = check_release(args.directory, args.tag)
    else:
        ok = run_fixtures()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
