#!/usr/bin/env python3
"""Manage pinned upstream Pi packages and the local dots companion."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
NPM_PIN = re.compile(r"npm:((?:@[a-z0-9._-]+/)?[a-z0-9._-]+)@(\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?)")


def run(args, cwd):
    subprocess.run(args, cwd=cwd, check=True)


def sources(root):
    config = json.loads((root / "pi-packages.json").read_text())
    if not isinstance(config, dict) or set(config) != {"npm"} or not isinstance(config["npm"], list):
        raise ValueError("pi-packages.json must contain an npm array")
    result = {}
    for spec in config["npm"]:
        match = NPM_PIN.fullmatch(spec) if isinstance(spec, str) else None
        if not match:
            raise ValueError("Upstream packages require an exact npm version")
        name = match[1]
        if name in result:
            raise ValueError(f"Duplicate upstream package: {name}")
        result[name] = spec
    return result


def packages(root):
    directory = root / "pi-plugins/dots"
    manifest = json.loads((directory / "package.json").read_text())
    name = manifest.get("name")
    if not isinstance(name, str) or not name or not isinstance(manifest.get("pi"), dict):
        raise ValueError("Companion requires a name and explicit pi manifest")
    lock = json.loads((directory / "package-lock.json").read_text())
    if lock.get("name") != name or lock.get("lockfileVersion", 0) < 2:
        raise ValueError("Companion requires a matching npm lockfile (version 2 or newer)")
    if "check" not in manifest.get("scripts", {}):
        raise ValueError("Companion check script is missing")
    return [(directory, ["check"])]


def registrations(agent_dir):
    file = agent_dir / "settings.json"
    entries = json.loads(file.read_text()).get("packages", []) if file.exists() else []
    if not isinstance(entries, list):
        raise ValueError("Pi settings packages must be an array")
    return entries


def source_identity(spec, agent_dir):
    if spec.startswith("npm:"):
        return spec[4:].rsplit("@", 1)[0] if "@" in spec[5:] else spec[4:], spec
    if spec.startswith(("git:", "https://", "http://", "ssh://", "git://")):
        name = spec.rstrip("/").rsplit("/", 1)[-1].split("@", 1)[0].removesuffix(".git")
        return name, spec
    path = Path(spec).expanduser()
    path = (path if path.is_absolute() else agent_dir / path).resolve()
    manifest = path / "package.json"
    name = json.loads(manifest.read_text()).get("name") if manifest.is_file() else path.name
    return name, path


def check_registrations(package_list, upstream, agent_dir):
    desired = dict(upstream)
    for directory, _checks in package_list:
        name = json.loads((directory / "package.json").read_text())["name"]
        if name in desired:
            raise ValueError(f"Duplicate managed package: {name}")
        desired[name] = directory.resolve()
    registered = set()
    for registration in registrations(agent_dir):
        spec = registration.get("source") if isinstance(registration, dict) else registration
        if not isinstance(spec, str):
            raise ValueError("Invalid Pi package registration")
        name, identity = source_identity(spec, agent_dir)
        if name in desired:
            if identity != desired[name] or name in registered:
                raise ValueError(f"Another copy/version of {name} is registered. Remove it with pi remove before pi-install; settings were not changed.")
            registered.add(name)
    return registered


def install(package_list, upstream, agent_dir):
    registered = check_registrations(package_list, upstream, agent_dir)
    for directory, _checks in package_list:
        run(["npm", "ci", "--ignore-scripts"], directory)
    # Reconcile pinned upstream installations through Pi, without rewriting existing filters.
    for name, spec in upstream.items():
        run(["pi", "update" if name in registered else "install", "--no-approve", spec], agent_dir)
    for directory, _checks in package_list:
        name = json.loads((directory / "package.json").read_text())["name"]
        if name not in registered:
            run(["pi", "install", "--no-approve", str(directory.resolve())], directory)
    print("Packages installed. Restart Pi when ready; no live reload was performed.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("deps", "check", "install"))
    args = parser.parse_args()
    try:
        upstream = sources(ROOT)
        package_list = packages(ROOT)
        if args.command == "install":
            agent_dir = Path(os.environ.get("PI_CODING_AGENT_DIR", "~/.pi/agent")).expanduser().resolve()
            agent_dir.mkdir(parents=True, exist_ok=True)
            install(package_list, upstream, agent_dir)
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
