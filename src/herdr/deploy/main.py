"""Publish verified, checkout-independent Herdr releases and register them."""
import argparse
import errno
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[3]


def runtime_manifest(path):
    text = path.read_text(encoding="utf-8")
    source = tomllib.loads(text)
    if len(source.pop("build", [])) != 1:
        raise RuntimeError(f"{path}: expected one source build callback")
    text, count = re.subn(r"(?ms)^\[\[build\]\]\s*\n.*?(?=^\[|\Z)", "", text)
    if count != 1 or tomllib.loads(text) != source:
        raise RuntimeError(f"{path}: cannot remove build callback without changing runtime fields")
    return text


def verify(release, entries):
    for name, text, payload in entries:
        plugin = release / name
        manifest, binary = plugin / "herdr-plugin.toml", plugin / "bin" / f"herdr-{name}"
        if (any(path.is_symlink() for path in (release, plugin, plugin / "bin", manifest, binary))
                or manifest.read_text(encoding="utf-8") != text
                or binary.read_bytes() != payload or not os.access(binary, os.X_OK)):
            raise RuntimeError(f"prepared release changed: {plugin}")


def prepare(home, names=()):
    available = {path.parent.name: path for path in (ROOT / "src/herdr").glob("*/herdr-plugin.toml")}
    selected = sorted(names or available)
    if not selected or len(selected) != len(set(selected)) or any(name not in available for name in selected):
        raise ValueError("select distinct, existing Herdr plugins")
    digest, entries = hashlib.sha256(), []
    for name in selected:
        binary = ROOT / "dist/bin" / f"herdr-{name}"
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise RuntimeError(f"missing staged executable {binary}; build first")
        text, payload = runtime_manifest(available[name]), binary.read_bytes()
        digest.update(name.encode() + b"\0" + hashlib.sha256(text.encode()).digest()
                      + hashlib.sha256(payload).digest())
        entries.append((name, text, payload))
    releases = home / ".local/share/dots/herdr/releases"
    if releases.resolve().is_relative_to(ROOT.resolve()):
        raise RuntimeError("runtime data resolves into the checkout; restow without directory folding")
    releases.mkdir(parents=True, exist_ok=True)
    release = releases / digest.hexdigest()
    if not release.exists() and not release.is_symlink():
        with tempfile.TemporaryDirectory(prefix="prepare-", dir=releases) as temporary:
            stage = Path(temporary) / "release"
            for name, text, payload in entries:
                plugin = stage / name
                (plugin / "bin").mkdir(parents=True)
                binary = plugin / "bin" / f"herdr-{name}"
                binary.write_bytes(payload)
                binary.chmod(0o755)
                (plugin / "herdr-plugin.toml").write_text(text, encoding="utf-8")
            verify(stage, entries)
            try:
                stage.rename(release)
            except OSError as error:
                if error.errno not in (errno.EEXIST, errno.ENOTEMPTY):
                    raise
                # Another publisher may have won with the same snapshot. Never
                # reuse its directory without checking every expected byte.
                verify(release, entries)
    verify(release, entries)
    return [release / name for name in selected]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugins", nargs="*", help="plugin names; default: all manifest-backed plugins")
    args = parser.parse_args()
    for plugin in prepare(Path.home(), args.plugins):
        # Herdr owns registration. A failed link is fatal; earlier links may
        # already have succeeded, so a multi-plugin install is not transactional.
        subprocess.run(["herdr", "plugin", "link", str(plugin)], check=True, timeout=30)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(f"herdr deployment: {error}") from error
