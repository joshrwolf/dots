#!/usr/bin/env python3
"""Import, update, and link shared and machine-local skills owned by dots."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = ROOT / "skills"
VENDOR = PACKAGE / ".local/share/agent-skills"
NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")


def sources():
    with (ROOT / "skill-sources.toml").open("rb") as file:
        entries = tomllib.load(file)["sources"]
    if not isinstance(entries, dict):
        raise ValueError("sources must be a table")
    for name, entry in entries.items():
        if not NAME.fullmatch(name):
            raise ValueError(f"Invalid source name: {name}")
        if not isinstance(entry, dict) or set(entry) != {"url", "ref", "skills"}:
            raise ValueError(f"{name}: expected url, ref, and skills")
        for field in ("url", "ref"):
            if not isinstance(entry[field], str) or not entry[field] or entry[field].startswith("-"):
                raise ValueError(f"{name}: invalid {field}")
        if not isinstance(entry["skills"], list) or not entry["skills"]:
            raise ValueError(f"{name}: skills must be a nonempty list of directory patterns")
        for pattern in entry["skills"]:
            if not isinstance(pattern, str) or not pattern or Path(pattern).is_absolute() or ".." in Path(pattern).parts:
                raise ValueError(f"{name}: skill patterns must stay inside the subtree")
    return entries


def skill_name(skill):
    text = (skill / "SKILL.md").read_text()
    if not text.startswith("---\n"):
        raise ValueError(f"Missing frontmatter: {skill}")
    frontmatter, separator, _ = text[4:].partition("\n---")
    match = re.search(r"^name:\s*[\"']?([a-z0-9-]+)[\"']?\s*$", frontmatter, re.MULTILINE)
    if not separator or not match or not NAME.fullmatch(match[1]):
        raise ValueError(f"Invalid skill name: {skill}")
    return match[1]


def link_skills(entries):
    shared = PACKAGE / ".agents/skills"
    claude = PACKAGE / ".claude/skills"
    desired = {}
    for source, entry in entries.items():
        checkout = VENDOR / source
        for pattern in entry["skills"]:
            directories = [checkout] if pattern == "." else sorted(checkout.glob(pattern))
            if not directories:
                raise ValueError(f"{source}: no skills match {pattern}; import the source first")
            for skill in directories:
                if not skill.resolve().is_relative_to(checkout.resolve()):
                    raise ValueError(f"Skill escapes its subtree: {skill}")
                name = skill_name(skill)
                if name.startswith("local-"):
                    raise ValueError(f"Imported skill uses reserved local- prefix: {name}")
                for link, destination in ((shared / name, skill), (claude / name, shared / name)):
                    if link in desired:
                        raise ValueError(f"Duplicate skill name across registered sources: {name}")
                    desired[link] = os.path.relpath(destination, link.parent)

    local_skills = sorted(shared.glob("local-*"))
    for skill in local_skills:
        if not NAME.fullmatch(skill.name) or not (skill / "SKILL.md").is_file():
            raise ValueError(f"Local skill needs a valid directory name and SKILL.md: {skill}")
        if skill_name(skill) != skill.name:
            raise ValueError(f"Local skill frontmatter name must match its directory: {skill}")
        desired[claude / skill.name] = os.path.relpath(skill, claude)

    # Validate every destination before mutating anything; personal skills win.
    for link, target in desired.items():
        if link.is_symlink():
            if str(link.readlink()) != target:
                raise ValueError(f"Refusing to replace a different link: {link}")
        elif link.exists():
            raise ValueError(f"Refusing to replace a local skill: {link}")

    # This namespace belongs to registered imports, including removed sources.
    managed_prefix = os.path.relpath(VENDOR, shared) + "/"
    for link in shared.glob("*"):
        if link.is_symlink() and link not in desired and str(link.readlink()).startswith(managed_prefix):
            alias = claude / link.name
            if alias.is_symlink() and str(alias.readlink()) == os.path.relpath(link, claude):
                alias.unlink()
            link.unlink()
    # Only remove stale local aliases that point to our shared discovery path.
    for link in claude.glob("local-*"):
        target = os.path.relpath(shared / link.name, claude)
        if link not in desired and link.is_symlink() and str(link.readlink()) == target:
            link.unlink()
    for link, target in desired.items():
        link.parent.mkdir(parents=True, exist_ok=True)
        if not link.is_symlink():
            link.symlink_to(target)
    imported_count = (len(desired) - len(local_skills)) // 2
    print(f"Linked {imported_count} imported and {len(local_skills)} local skills for Codex, Claude, and Pi")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("links", "import", "update"))
    parser.add_argument("source", nargs="?")
    args = parser.parse_args()
    try:
        entries = sources()
        if args.command != "links":
            if args.source not in entries:
                parser.error("choose SOURCE from: " + ", ".join(entries))
            entry = entries[args.source]
            prefix = (VENDOR / args.source).relative_to(ROOT)
            operation = "add" if args.command == "import" else "pull"
            subprocess.run([
                "git", "subtree", operation, f"--prefix={prefix}",
                entry["url"], entry["ref"], "--squash",
            ], cwd=ROOT, check=True)
        link_skills(entries)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"skill sources: {error}\n")


if __name__ == "__main__":
    main()
