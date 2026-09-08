"""Tests for the demo's debug markers.

`markers` reaches Isaac Lab through `isaaclab.markers`, so this is skipped
entirely where that isn't installed.
"""

from __future__ import annotations

import sys
import types

import pytest

markers = pytest.importorskip("markers", reason="Isaac Lab is not installed")


# A flat scene leaves Newton's count unset; a cloned one has already counted.
@pytest.mark.parametrize("counted, expected", [(None, 0), (4, 4)])
def test_newton_env_count_only_filled_in_when_missing(counted, expected, monkeypatch):
    manager = types.SimpleNamespace(get_num_envs=lambda: counted, _num_envs=counted)
    monkeypatch.setitem(sys.modules, "isaaclab_newton.physics", types.SimpleNamespace(NewtonManager=manager))
    markers._give_newton_an_env_count()
    assert manager._num_envs == expected


def test_newton_env_count_without_newton(monkeypatch):
    """Nothing to fix up on a backend that never imported Newton."""
    monkeypatch.delitem(sys.modules, "isaaclab_newton.physics", raising=False)
    markers._give_newton_an_env_count()
