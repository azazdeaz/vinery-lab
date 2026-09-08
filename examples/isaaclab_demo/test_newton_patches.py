"""Tests for the demo's workarounds for Newton backend bugs."""

from __future__ import annotations

import sys
import types

import numpy as np
import pytest

import newton_patches


class FakeArray:
    """A warp array: `numpy()` hands out a host copy, `assign()` writes back."""

    def __init__(self, values):
        self.values = np.asarray(values, dtype=np.float32)

    def numpy(self):
        return self.values.copy()

    def assign(self, values):
        self.values = np.asarray(values, dtype=np.float32)


@pytest.fixture
def solver(monkeypatch):
    """A two-world solver whose middle geom is a height field, 0.5 m up."""
    mujoco = pytest.importorskip("mujoco")
    solver = types.SimpleNamespace(
        mj_model=types.SimpleNamespace(
            geom_type=np.array(
                [
                    mujoco.mjtGeom.mjGEOM_PLANE,
                    mujoco.mjtGeom.mjGEOM_HFIELD,
                    mujoco.mjtGeom.mjGEOM_BOX,
                ]
            ),
            geom_pos=np.array([[0, 0, 0], [1, 2, 0.5], [0, 0, 3]], dtype=np.float32),
        ),
        mjw_model=types.SimpleNamespace(geom_pos=FakeArray(np.zeros((2, 3, 3)))),
        mjw_data=types.SimpleNamespace(geom_xpos=FakeArray(np.zeros((2, 3, 3)))),
    )
    manager = types.SimpleNamespace(NewtonManager=types.SimpleNamespace(_solver=solver))
    monkeypatch.setitem(sys.modules, "isaaclab_newton.physics", manager)
    return solver


def test_height_fields_take_the_cpu_models_offset_in_every_world(solver):
    newton_patches.fix_heightfield_offsets()
    for array in (solver.mjw_model.geom_pos, solver.mjw_data.geom_xpos):
        assert (array.values[:, 1] == np.float32([1.0, 2.0, 0.5])).all()
        assert not array.values[:, [0, 2]].any(), "geoms that are not height fields are left alone"


def test_a_scene_without_height_fields_is_untouched(solver):
    solver.mj_model.geom_type[1] = solver.mj_model.geom_type[2]
    newton_patches.fix_heightfield_offsets()
    assert not solver.mjw_model.geom_pos.values.any()


# No Newton backend at all, then Newton imported but not yet solving.
@pytest.mark.parametrize(
    "manager", [None, types.SimpleNamespace(NewtonManager=types.SimpleNamespace(_solver=None))]
)
def test_nothing_to_do_before_a_solver_exists(manager, monkeypatch):
    monkeypatch.delitem(sys.modules, "isaaclab_newton.physics", raising=False)
    if manager is not None:
        monkeypatch.setitem(sys.modules, "isaaclab_newton.physics", manager)
    newton_patches.fix_heightfield_offsets()
