"""Tests for the video's storyboard: which take each cane is filmed for, and
when."""

from __future__ import annotations

import numpy as np
import pytest

video = pytest.importorskip("video", reason="Isaac Lab is not installed")

from pruner import Cane, Cut  # noqa: E402


def test_the_takes_go_on_the_canes_nearest_the_robot_and_the_others_are_cut_last():
    """Six canes on two arms of a cordon, the trunk between them, leaning
    toward the robot or away: every cane is cut once, the takes first, in
    `TAKES`' order but those that push after the others. Those that do not
    push go on the canes between nearest the robot, the first on the nearest,
    and those that push on every second cane; the canes no take has -- here
    the cane between leaning away -- come last."""
    row = []
    for x, y in zip((0.0, 0.12, 0.25, 0.6, 0.72, 0.85), (-0.08, 0.05, 0.0, -0.06, 0.02, -0.04)):
        pose = np.eye(4)
        pose[:2, 3] = x, y
        row.append(Cut(Cane(f"/cane_{x}", []), pose, None, None))

    order = video.schedule(row, np.array([0.3, 0.0, 0.0]), np.array([0.0, 1.0, 0.0]))

    assert sorted(map(id, (cut for cut, _ in order))) == sorted(map(id, row)), "each cane once"
    pushing = [take for take in video.TAKES if any(take.push)]
    still = [take for take in video.TAKES if not any(take.push)]
    takes = len(still) + len(pushing)
    assert [take for _, take in order[:takes]] == still + pushing, "the takes first"
    assert [id(cut) for cut, _ in order[: len(still)]] == [id(row[3]), id(row[5])][: len(still)]
    assert {id(cut) for cut, take in order if take in pushing} <= set(map(id, row[0::2]))
    assert [id(cut) for cut, take in order[takes:]] == [id(row[1])], "the cane leaning away last"
