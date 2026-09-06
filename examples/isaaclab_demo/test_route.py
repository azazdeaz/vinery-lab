"""Tests for the demo's alley route.

`route` reaches Isaac Lab through `vinerylab.isaaclab`, so this is skipped
entirely where that isn't installed.
"""

from __future__ import annotations

import numpy as np
import pytest

route = pytest.importorskip("route", reason="Isaac Lab is not installed")

SPACING = 2.4  # m between the two rows an alley runs between


def posts(first: float, last: float, across: float) -> np.ndarray:
    """A row's posts, every 6 m from `first` to `last` along the AB line."""
    along = np.arange(first, last + 1e-6, 6.0)
    return np.column_stack([along, np.full_like(along, across), np.zeros_like(along)])


# Which of the alley's two rows is the one clipped short. The row a headland
# turn goes around is as often the neighbour as it is the row the alley was
# derived from, so both orders have to hold.
@pytest.mark.parametrize("swapped", [False, True])
def test_alley_clears_both_rows(swapped: bool):
    """The alley runs out past the further end of either row, and stays centred."""
    ends = [(0.0, 60.0), (12.0, 36.0)]  # the row spanning the block, then a clipped one
    if swapped:
        ends.reverse()
    rows = [posts(first, last, index * SPACING) for index, (first, last) in enumerate(ends)]

    alley = route._paved(*rows, a=np.zeros(2), ab=np.array([1.0, 0.0]))

    assert alley[0, 0] <= 0.0 - route.RUNOUT
    assert alley[-1, 0] >= 60.0 + route.RUNOUT
    assert np.allclose(alley[:, 1], SPACING / 2)
    assert np.allclose(np.diff(alley[:, 0]), route.STRIDE)
