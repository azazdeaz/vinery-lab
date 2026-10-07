"""Tests for the video's storyboard: which take each cane is filmed for, and
when."""

from __future__ import annotations

import numpy as np
import pytest

video = pytest.importorskip("video", reason="Isaac Lab is not installed")

from pruner import Cane, Cut  # noqa: E402


def test_the_takes_that_push_go_on_every_second_cane_after_the_canes_between():
    """Six canes on two arms of a cordon, the trunk between them, leaning
    toward the robot or away: every cane is cut once, the takes in `TAKES`'
    order but those that push last, and those on every second cane along the
    cordon, whose neighbours are cut by then. The take of several cuts is on
    the cane between them nearest the robot."""
    row = []
    for x, y in zip((0.0, 0.12, 0.25, 0.6, 0.72, 0.85), (-0.08, 0.05, 0.0, -0.06, 0.02, -0.04)):
        pose = np.eye(4)
        pose[:2, 3] = x, y
        row.append(Cut(Cane(f"/cane_{x}", []), pose, None, None))

    order = video.schedule(row, np.array([0.3, 0.0, 0.0]), np.array([0.0, 1.0, 0.0]))

    assert sorted(map(id, (cut for cut, _ in order))) == sorted(map(id, row)), "each cane once"
    pushing = [take for take in video.TAKES if any(take.push)]
    still = [take for take in video.TAKES if not any(take.push)]
    assert [take for _, take in order if take] == still + pushing
    first = next(i for i, (_, take) in enumerate(order) if take in pushing)
    assert {id(cut) for cut, _ in order[:first]} == set(map(id, row[1::2])), "all between first"
    assert {id(cut) for cut, take in order if take in pushing} <= set(map(id, row[0::2]))
    assert [id(cut) for cut, take in order if take and len(take.kept) > 1] == [id(row[3])]
