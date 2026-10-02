#!/usr/bin/env python3
"""Explicit test-only dependency install; checks are offline and verify the pin."""
import json
import pathlib
import re
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
LOCK = json.loads((HERE / "deps.json").read_text())
REVISION = LOCK["revision"]
assert re.fullmatch(r"[0-9a-f]{40}", REVISION), "expected a full Git SHA"
DESTINATION = HERE.parents[1] / "target" / "test-deps" / "mini.test" / REVISION


def git(path, *args):
    return subprocess.run(
        ["git", "-C", str(path), *args], check=True, capture_output=True,
        text=True, timeout=120,
    ).stdout.strip()


def check():
    if not DESTINATION.is_dir():
        raise RuntimeError("mini.test is missing; run make herdr-test-deps")
    if git(DESTINATION, "rev-parse", "HEAD") != REVISION:
        raise RuntimeError("mini.test checkout does not match tests/deps.json")
    if git(DESTINATION, "status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError("mini.test checkout has local modifications")
    if not (DESTINATION / "lua" / "mini" / "test.lua").is_file():
        raise RuntimeError("mini.test module is missing")
    return DESTINATION


def install():
    if DESTINATION.exists():
        return check()
    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="fetch-", dir=DESTINATION.parent) as temp:
        source = pathlib.Path(temp) / "checkout"
        source.mkdir()
        git(source, "init", "--quiet")
        git(source, "remote", "add", "origin", LOCK["repository"])
        git(source, "fetch", "--depth=1", "origin", REVISION)
        git(source, "-c", "advice.detachedHead=false", "checkout", "--detach", "FETCH_HEAD")
        if git(source, "rev-parse", "HEAD") != REVISION:
            raise RuntimeError("fetched revision does not match lock")
        source.rename(DESTINATION)
    return check()


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["install"]:
            print(install())
        elif sys.argv[1:] == ["check"]:
            print(check())
        else:
            raise RuntimeError("usage: deps.py install|check")
    except (RuntimeError, subprocess.SubprocessError, OSError) as error:
        sys.exit(str(error))
