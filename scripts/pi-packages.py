#!/usr/bin/env python3
"""Manage dots-owned local Pi packages and pinned editable subtrees."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
COMMIT = re.compile(r"[0-9a-f]{40}")


def run(args, cwd):
    subprocess.run(args, cwd=cwd, check=True)


def sources(root):
    with (root / "pi-sources.toml").open("rb") as file:
        entries = tomllib.load(file)["sources"]
    if not isinstance(entries, dict) or not entries:
        raise ValueError("sources must be a nonempty table")
    for name, entry in entries.items():
        if not NAME.fullmatch(name):
            raise ValueError(f"Invalid source name: {name}")
        if not isinstance(entry, dict) or set(entry) != {"url", "ref", "checks"}:
            raise ValueError(f"{name}: expected url, ref, and checks")
        if not isinstance(entry["url"], str) or not entry["url"].startswith("https://"):
            raise ValueError(f"{name}: url must use HTTPS")
        if not isinstance(entry["ref"], str) or not COMMIT.fullmatch(entry["ref"]):
            raise ValueError(f"{name}: ref must be a full lowercase Git commit SHA")
        if not isinstance(entry["checks"], list) or not entry["checks"]:
            raise ValueError(f"{name}: checks must be a nonempty list of npm script names")
        if any(not isinstance(check, str) or not re.fullmatch(r"[a-zA-Z0-9:_-]+", check)
               or check.startswith("-") for check in entry["checks"]):
            raise ValueError(f"{name}: invalid check script name")
    return entries


def subtree(root, entries, operation, source):
    if source not in entries:
        raise ValueError("choose SOURCE from: " + ", ".join(entries))
    status = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=all"], cwd=root, text=True,
    )
    if status.strip():
        raise ValueError("subtree operations create commits; commit your changes first (including untracked files)")
    prefix = Path("pi-plugins/vendor") / source
    exists = (root / prefix).exists()
    if operation == "import" and exists:
        raise ValueError(f"{source}: already imported; use pi-update")
    if operation == "update" and not exists:
        raise ValueError(f"{source}: not imported; use pi-import")
    entry = entries[source]
    run(["git", "subtree", "add" if operation == "import" else "pull",
         f"--prefix={prefix}", entry["url"], entry["ref"], "--squash"], root)
    print("Subtree operation complete. Review the diff and run make pi-deps pi-check.")


def packages(root, entries):
    result = []
    for name, entry in entries.items():
        result.append((root / "pi-plugins/vendor" / name, entry["checks"]))
    result.append((root / "pi-plugins/dots", ["check"]))
    # Validate all packages before installing dependencies or changing Pi settings.
    names = set()
    for directory, checks in result:
        if not (directory / "package.json").is_file():
            raise ValueError(f"Missing {directory.relative_to(root)}/package.json; run make pi-import SOURCE={directory.name} first")
        manifest = json.loads((directory / "package.json").read_text())
        name = manifest.get("name")
        if not isinstance(name, str) or not name or name in names:
            raise ValueError(f"{directory.name}: missing or duplicate package name")
        names.add(name)
        if not isinstance(manifest.get("pi"), dict):
            raise ValueError(f"{name}: explicit pi manifest required")
        lock = json.loads((directory / "package-lock.json").read_text())
        if lock.get("name") != name or lock.get("lockfileVersion", 0) < 2:
            raise ValueError(f"{name}: expected a matching npm lockfile (version 2 or newer)")
        scripts = manifest.get("scripts", {})
        if any(check not in scripts for check in checks):
            raise ValueError(f"{name}: a configured check script is missing")
    return result


def check_registrations(package_list, agent_dir):
    settings_file = agent_dir / "settings.json"
    if not settings_file.exists():
        return
    settings = json.loads(settings_file.read_text())
    registrations = settings.get("packages", [])
    if not isinstance(registrations, list):
        raise ValueError("Pi settings packages must be an array")
    desired = {
        json.loads((directory / "package.json").read_text())["name"]: directory.resolve()
        for directory, _checks in package_list
    }
    for registration in registrations:
        spec = registration.get("source") if isinstance(registration, dict) else registration
        if not isinstance(spec, str):
            raise ValueError("Invalid Pi package registration")
        if spec.startswith("npm:"):
            name = spec[4:].split("@", 1)[0] if not spec[4:].startswith("@") else "@" + spec[5:].split("@", 1)[0]
            conflict = name in desired
        elif spec.startswith(("git:", "https://", "http://", "ssh://", "git://")):
            # Also catch the ordinary upstream/fork registrations for this package.
            basename = spec.rstrip("/").rsplit("/", 1)[-1].split("@", 1)[0].removesuffix(".git")
            conflict = basename in desired
        else:
            path = Path(spec).expanduser()
            if not path.is_absolute():
                path = agent_dir / path
            path = path.resolve()
            if path in desired.values():
                # Preserve existing Pi resource filters; no re-registration needed.
                continue
            manifest_path = path / "package.json"
            name = json.loads(manifest_path.read_text()).get("name") if manifest_path.is_file() else None
            conflict = name in desired
        if conflict:
            raise ValueError("Another copy of a managed Pi package is registered. Remove it with pi remove before pi-install; settings were not changed.")


def install(package_list, agent_dir):
    check_registrations(package_list, agent_dir)
    for directory, _checks in package_list:
        run(["npm", "ci", "--ignore-scripts"], directory)
    settings_file = agent_dir / "settings.json"
    settings = json.loads(settings_file.read_text()) if settings_file.exists() else {}
    registered = set()
    for registration in settings.get("packages", []):
        spec = registration.get("source") if isinstance(registration, dict) else registration
        if isinstance(spec, str) and not spec.startswith(("npm:", "git:", "https://", "http://", "ssh://", "git://")):
            path = Path(spec).expanduser()
            registered.add((path if path.is_absolute() else agent_dir / path).resolve())
    for directory, _checks in package_list:
        if directory.resolve() not in registered:
            run(["pi", "install", "--no-approve", str(directory.resolve())], directory)
    print("Local packages registered. Use /reload or restart Pi when ready; no live reload was performed.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("import", "update", "deps", "check", "install"))
    parser.add_argument("source", nargs="?")
    args = parser.parse_args()
    try:
        entries = sources(ROOT)
        if args.command in ("import", "update"):
            subtree(ROOT, entries, args.command, args.source)
            return
        package_list = packages(ROOT, entries)
        if args.command == "install":
            agent_dir = Path(os.environ.get("PI_CODING_AGENT_DIR", "~/.pi/agent")).expanduser().resolve()
            install(package_list, agent_dir)
        elif args.command == "deps":
            for directory, _checks in package_list:
                run(["npm", "ci", "--ignore-scripts"], directory)
        else:
            for directory, checks in package_list:
                for check in checks:
                    run(["npm", "run", check], directory)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"pi packages: {error}\n")


if __name__ == "__main__":
    main()
