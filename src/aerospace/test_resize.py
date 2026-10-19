"""Divider policy tests; never contact AeroSpace or WindowServer."""
import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import call, patch

spec = importlib.util.spec_from_file_location("resize", Path(__file__).with_name("resize-step.py"))
resize = importlib.util.module_from_spec(spec)
spec.loader.exec_module(resize)


class DividerTests(unittest.TestCase):
    def test_fraction_steps_tolerate_rounding_and_clamp(self):
        for direction, current, expected in [
            ("larger", 300, 400), ("larger", 399, 600), ("larger", 900, 900),
            ("smaller", 900, 800), ("smaller", 601, 400), ("smaller", 300, 300),
        ]:
            with self.subTest(direction=direction, current=current):
                self.assertEqual(resize.next_width(current, 1200, direction), expected)

    def fixtures(self):
        windows = [{"window-id": number, "window-layout": "h_tiles",
                    "workspace-root-container-layout": "h_tiles", "window-is-fullscreen": False}
                   for number in (1, 2)]
        frames = {1: {"X": 0, "Y": 0, "Width": 400, "Height": 800},
                  2: {"X": 420, "Y": 0, "Width": 800, "Height": 800}}
        return windows, frames

    def test_divider_direction_is_independent_of_focus_and_excludes_gaps(self):
        for focused in (1, 2):
            with self.subTest(focused=focused):
                windows, frames = self.fixtures()
                replies = [json.dumps([{"window-id": focused, "workspace": "example"}]),
                           json.dumps(windows), ""]
                with patch.object(resize, "aerospace", side_effect=replies) as command, \
                     patch.object(resize, "window_bounds", return_value=frames), \
                     patch.object(sys, "argv", ["resize-step", "right"]):
                    resize.main()
                self.assertEqual(command.call_args, call("resize", "--window-id", str(focused), "width", "600"))

    def test_unsupported_layouts_do_not_read_geometry_or_resize(self):
        for field, value in [("window-layout", "floating"),
                             ("workspace-root-container-layout", "v_tiles"),
                             ("window-is-fullscreen", True)]:
            with self.subTest(field=field):
                windows, _ = self.fixtures()
                windows[0][field] = value
                replies = [json.dumps([{"window-id": 1, "workspace": "example"}]), json.dumps(windows)]
                with patch.object(resize, "aerospace", side_effect=replies) as command, \
                     patch.object(resize, "window_bounds") as geometry, \
                     patch.object(sys, "argv", ["resize-step", "right"]):
                    resize.main()
                geometry.assert_not_called()
                self.assertEqual(command.call_count, 2)

    def test_inconsistent_geometry_is_not_resized(self):
        windows, frames = self.fixtures()
        frames[2]["Y"] = 100
        replies = [json.dumps([{"window-id": 1, "workspace": "example"}]), json.dumps(windows)]
        with patch.object(resize, "aerospace", side_effect=replies) as command, \
             patch.object(resize, "window_bounds", return_value=frames), \
             patch.object(sys, "argv", ["resize-step", "right"]):
            resize.main()
        self.assertEqual(command.call_count, 2)


if __name__ == "__main__":
    unittest.main()
