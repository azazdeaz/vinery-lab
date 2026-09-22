"""Tests for the demo's row-straddling route.

`route` reaches Isaac Lab through `vinerylab.isaaclab`, so this is skipped
entirely where that isn't installed.
"""

from __future__ import annotations

import numpy as np
import pytest

route = pytest.importorskip("route", reason="Isaac Lab is not installed")

SPACING = 2.4  # m between rows
A = np.zeros(2)
AB = np.array([1.0, 0.0])


def posts(first: float, last: float, across: float) -> np.ndarray:
    """A row's posts, every 6 m from `first` to `last` along the AB line."""
    along = np.arange(first, last + 1e-6, 6.0)
    return np.column_stack([along, np.full_like(along, across), np.zeros_like(along)])


def block(*ends: tuple[float, float]) -> list[np.ndarray]:
    """One row per pair of end positions, a row spacing apart."""
    return [posts(first, last, index * SPACING) for index, (first, last) in enumerate(ends)]


class Slope:
    """Ground climbing at `gradient` along the AB line, for the scene's own
    `vinerylab.usd.Ground`."""

    def __init__(self, gradient: float = 0.0):
        self.gradient = gradient

    def height(self, x: float, y: float) -> float:
        return self.gradient * x


def test_waypoint_heights_are_read_off_the_ground():
    """Not off the posts, which stop where the run-out into the headland begins."""
    passes = route._passes(block((0.0, 60.0), (0.0, 60.0)), A, AB, Slope(0.1))

    assert np.allclose(passes[0][:, 2], 0.1 * passes[0][:, 0])


def test_a_pass_runs_the_row_it_straddles():
    passes = route._passes(block((0.0, 60.0), (0.0, 60.0)), A, AB, Slope())

    assert np.allclose(passes[0][:, 1], 0.0), "the first pass is on the first row"
    assert np.allclose(passes[1][:, 1], SPACING), "the second is one row spacing over"
    assert passes[0][0, 0] <= -route.RUNOUT
    assert passes[0][-1, 0] >= 60.0 + route.RUNOUT
    assert np.allclose(np.diff(passes[0][:, 0]), route.STRIDE)


def test_alternate_passes_run_the_other_way():
    """One continuous path, so no drive back to the start between rows."""
    passes = route._passes(block((0.0, 60.0), (0.0, 60.0), (0.0, 60.0)), A, AB, Slope())

    assert passes[0][0, 0] < passes[0][-1, 0]
    assert passes[1][0, 0] > passes[1][-1, 0]
    assert passes[2][0, 0] < passes[2][-1, 0]


# Which of two neighbouring rows is the one clipped short. Crossing between
# them takes a leg over each, so it has to clear both whichever way round.
@pytest.mark.parametrize("swapped", [False, True])
def test_a_pass_clears_its_neighbours_end_posts(swapped: bool):
    ends = [(0.0, 60.0), (12.0, 36.0)]  # the row spanning the block, then a clipped one
    if swapped:
        ends.reverse()

    passes = route._passes(block(*ends), A, AB, Slope())

    for leg in passes:
        assert leg[:, 0].min() <= 0.0 - route.RUNOUT
        assert leg[:, 0].max() >= 60.0 + route.RUNOUT


def test_the_leg_between_two_passes_repeats_neither_end():
    start, end = np.zeros(3), np.array([0.0, SPACING, 0.0])

    between = route._between(start, end)

    assert not np.isclose(between, start).all(axis=1).any()
    assert not np.isclose(between, end).all(axis=1).any()
    assert np.allclose(np.diff(between[:, 1]), np.diff(between[:, 1])[0])


# A start row is a rotation of the same closed route, so 0, an odd row (driven
# in reverse), an even one and a negative index should all come out whole.
@pytest.mark.parametrize("start", [0, 1, 2, -1])
def test_the_loop_starts_on_the_row_it_is_given(start: int):
    passes = route._passes(block((0.0, 60.0), (0.0, 60.0), (0.0, 60.0)), A, AB, Slope())

    loop = route._loop(passes, start)

    # Two waypoints, so the direction it sets off in is pinned as well as the
    # place: the same point is passed again on the way back, the other way round.
    assert np.allclose(loop[:2], passes[start][:2]), "begins on the row asked for"
    assert len(loop) == len(route._loop(passes, 0)), "a rotation, so no row is dropped"
    # Every step a stride or less, the wrap back to the start included: the
    # route is driven cyclically, so a jump there is a jump across the field.
    step = np.linalg.norm(np.diff(loop, axis=0, append=loop[:1]), axis=1)
    assert step.max() <= route.STRIDE + 1e-6


def test_a_start_row_outside_the_block_is_refused():
    passes = route._passes(block((0.0, 60.0), (0.0, 60.0)), A, AB, Slope())

    with pytest.raises(ValueError, match="has 2 rows"):
        route._loop(passes, 2)
