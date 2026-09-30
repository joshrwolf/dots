#!/usr/bin/env python3
"""Publish or check CLI-owned skill artifacts, without contacting any provider."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SKILL = ROOT / "skills/.agents/skills/dots"
ALIAS = ROOT / "skills/.claude/skills/dots"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("generate", "check"))
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    artifacts = {
        SKILL / "SKILL.md": ["skills", "render"],
        SKILL / "references/commands.md": ["skills", "render", "--reference"],
    }
    # Render everything before publishing; never leave half-generated output if
    # the command fails. Only deterministic help/metadata reaches these files.
    rendered = {
        path: subprocess.run([str(binary), *argv], check=True, capture_output=True,
                             text=True, timeout=10).stdout
        for path, argv in artifacts.items()
    }
    target = os.path.relpath(SKILL, ALIAS.parent)
    if ALIAS.is_symlink() and os.readlink(ALIAS) != target:
        raise RuntimeError(f"refusing to replace a different discovery link: {ALIAS}")
    if ALIAS.exists() and not ALIAS.is_symlink():
        raise RuntimeError(f"refusing to replace a discovery directory: {ALIAS}")
    stale = [path for path, text in rendered.items()
             if not path.is_file() or path.read_text() != text]
    if args.mode == "check":
        if stale or not ALIAS.is_symlink():
            raise RuntimeError("CLI skill artifacts are stale; run make cli-skills")
        print("CLI skill artifacts match the command tree")
        return
    for path in stale:
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as file:
            temporary = Path(file.name)
            file.write(rendered[path])
        try:
            temporary.chmod(0o644)
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)
    ALIAS.parent.mkdir(parents=True, exist_ok=True)
    if not ALIAS.is_symlink():
        ALIAS.symlink_to(target)
    print("Generated dots skill and command reference for shared agent discovery")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error
