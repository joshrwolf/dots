#!/usr/bin/env python3
"""Move a horizontal tile divider through quarters, thirds, and halves."""

import argparse
import ctypes
import fcntl
import json
import plistlib
import subprocess
import sys


def window_bounds():
    """Read window geometry from WindowServer without requesting AX access."""
    cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
    cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
    pointer = ctypes.c_void_p

    def bind(library, name, result, *arguments):
        function = getattr(library, name)
        function.restype = result
        function.argtypes = arguments
        return function

    copy_windows = bind(
        cg, "CGWindowListCopyWindowInfo", pointer, ctypes.c_uint32, ctypes.c_uint32
    )
    serialize = bind(
        cf, "CFPropertyListCreateData", pointer,
        pointer, pointer, ctypes.c_long, ctypes.c_ulong, pointer,
    )
    length = bind(cf, "CFDataGetLength", ctypes.c_long, pointer)
    data_pointer = bind(cf, "CFDataGetBytePtr", pointer, pointer)
    release = bind(cf, "CFRelease", None, pointer)

    windows = copy_windows(1, 0)  # kCGWindowListOptionOnScreenOnly
    if not windows:
        raise RuntimeError("WindowServer geometry is unavailable")
    try:
        data = serialize(None, windows, 200, 0, None)  # binary property list
        if not data:
            raise RuntimeError("Could not decode WindowServer geometry")
        try:
            records = plistlib.loads(ctypes.string_at(data_pointer(data), length(data)))
        finally:
            release(data)
    finally:
        release(windows)
    return {record["kCGWindowNumber"]: record["kCGWindowBounds"] for record in records}


def aerospace(*arguments):
    return subprocess.check_output(["aerospace", *arguments], text=True)


def next_width(current, total, direction):
    steps = [round(total * fraction) for fraction in (1 / 4, 1 / 3, 1 / 2, 2 / 3, 3 / 4)]
    # WindowServer rounds frames; tolerate one point at a step boundary.
    if direction == "smaller":
        return next((width for width in reversed(steps) if width < current - 1), steps[0])
    return next((width for width in steps if width > current + 1), steps[-1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("direction", choices=("left", "right"))
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    focused = json.loads(aerospace(
        "list-windows", "--focused", "--format", "%{window-id} %{workspace}", "--json"
    ))[0]
    windows = json.loads(aerospace(
        "list-windows", "--workspace", focused["workspace"], "--format",
        "%{window-id} %{window-layout} %{workspace-root-container-layout} %{window-is-fullscreen}",
        "--json",
    ))
    tiles = [window for window in windows if window["window-layout"] != "floating"]
    if (
        len(tiles) != 2
        or focused["window-id"] not in {window["window-id"] for window in tiles}
        or any(
            window["window-layout"] != "h_tiles"
            or window["workspace-root-container-layout"] != "h_tiles"
            or window["window-is-fullscreen"]
            for window in tiles
        )
    ):
        return  # Only resize a pair of horizontal tiles; floating windows may coexist.

    bounds = window_bounds()
    frames = [bounds.get(window["window-id"]) for window in tiles]
    if any(frame is None for frame in frames):
        return
    left, right = sorted(frames, key=lambda frame: frame["X"])
    if (
        abs(left["Y"] - right["Y"]) > 1
        or abs(left["Height"] - right["Height"]) > 1
        or left["X"] + left["Width"] > right["X"] + 1
    ):
        return  # A fullscreen transition or layout change is still in progress.

    focused_frame = bounds[focused["window-id"]]
    current = focused_frame["Width"]
    total = sum(frame["Width"] for frame in frames)  # Excludes inner and outer gaps.
    # Step the divider from the left edge regardless of which tile has focus.
    direction = "smaller" if args.direction == "left" else "larger"
    left_width = next_width(left["Width"], total, direction)
    target = left_width if focused_frame is left else round(total - left_width)
    if args.dry_run:
        print(f"width {current:g} -> {target} (available {total:g})")
    elif abs(target - current) > 1:
        aerospace("resize", "--window-id", str(focused["window-id"]), "width", str(target))


if __name__ == "__main__":
    # Drop overlapping key repeats instead of racing two reads of the same width.
    with open(__file__) as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            sys.exit(0)
        try:
            main()
        except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
            sys.exit(f"resize-step: {error}")
