"""Tests for cutting a rod while it is simulated.

Requires Isaac Lab; skipped entirely where it isn't installed. Runs a bare
Newton rod -- no Kit, no coupler -- stepped the way Isaac Lab steps it: the
queued model changes first, then the solver.
"""

from __future__ import annotations

import pytest

try:
    from vinerylab.isaaclab import cutting
except ImportError:
    pytest.skip("Isaac Lab is not installed", allow_module_level=True)

# Past the skip: `vinerylab` itself needs only usd-core, so numpy comes with
# Isaac Lab or not at all.
import numpy as np  # noqa: E402
import warp as wp  # noqa: E402
from isaaclab_newton.physics import NewtonManager  # noqa: E402
from newton import BodyFlags, ModelBuilder  # noqa: E402
from newton.solvers import SolverVBD  # noqa: E402
from pxr import Usd  # noqa: E402

SEGMENT = 0.1
DT = 1e-3
GRAVITY = 9.81


class Rig:
    """A straight rod of four 10 cm segments along x, a meter up, its first
    body held still the way the importer bolts a shoot to the wood."""

    def __init__(self, monkeypatch):
        builder = ModelBuilder(gravity=(0.0, 0.0, -GRAVITY))
        positions = [wp.vec3(SEGMENT * i, 0.0, 1.0) for i in range(5)]
        self.bodies, self.joints = builder.add_rod(
            positions,
            radius=0.01,
            bend_stiffness=100.0,
            label="/rod",
            body_frame_origin="com",
        )
        builder.body_mass[self.bodies[0]] = 0.0
        builder.body_inertia[self.bodies[0]] = wp.mat33(0.0)
        builder.body_flags[self.bodies[0]] = int(BodyFlags.KINEMATIC)
        builder.color()
        self.model = builder.finalize()
        self.solver = SolverVBD(self.model, iterations=10)
        self.state, self.next = self.model.state(), self.model.state()
        self.control = self.model.control()
        monkeypatch.setattr(NewtonManager, "_model", self.model)
        monkeypatch.setattr(NewtonManager, "_solver", self.solver)
        monkeypatch.setattr(NewtonManager, "_state_0", self.state)
        monkeypatch.setattr(NewtonManager, "_model_changes", set())
        self.shears = cutting.Shears(Usd.Stage.CreateInMemory())

    def step(self, seconds: float) -> None:
        for _ in range(round(seconds / DT)):
            for change in NewtonManager._model_changes:
                self.solver.notify_model_changed(change)
            NewtonManager._model_changes = set()
            self.solver.step(self.state, self.next, self.control, None, DT)
            self.state, self.next = self.next, self.state
            NewtonManager._state_0 = self.state

    def height(self) -> np.ndarray:
        return self.state.body_q.numpy()[self.bodies, 2]

    def spans(self) -> np.ndarray:
        """Each capsule's axis along x, as (start, end) per body."""
        return np.stack(self.shears._capsules(), axis=1)[:, :, 0]


@pytest.fixture
def rig(monkeypatch) -> Rig:
    return Rig(monkeypatch)


def plane_at(x: float) -> tuple:
    """A knife across the rod at `x`: a square meter in the yz plane."""
    return (x, -0.5, 0.5), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)


def test_a_cut_between_joints_moves_the_joint_to_the_cut(rig):
    """The segment the knife crosses ends at the knife, and the next one starts
    there; the mass follows the length, so the rod weighs what it did."""
    mass = rig.model.body_mass.numpy().sum()

    assert rig.shears.cut_through(*plane_at(0.14)) == 1

    spans = rig.spans()
    assert spans[1, 1] == pytest.approx(0.14, abs=1e-5)
    assert spans[2, 0] == pytest.approx(0.14, abs=1e-5)
    assert spans[2, 1] == pytest.approx(0.3, abs=1e-5)
    assert rig.model.body_mass.numpy().sum() == pytest.approx(mass)
    assert rig.shears.loose.tolist() == [False, False, True, True]
    # Only the knife's segment and the one after it were touched.
    assert spans[[0, 3]].ravel() == pytest.approx([0.0, 0.1, 0.3, 0.4], abs=1e-5)


def test_the_piece_falls_and_the_stub_stays(rig):
    before = rig.height()
    rig.shears.cut_through(*plane_at(0.14))
    rig.step(0.5)

    drop = before - rig.height()
    assert drop[2:] == pytest.approx(GRAVITY * 0.5**2 / 2, rel=0.02)
    assert abs(drop[1]) < 0.01
    # Cut free, the piece passes back through the knife without being cut again.
    assert rig.shears.cut_through(*plane_at(0.14)) == 0


@pytest.mark.parametrize(
    ("body", "at", "loose"),
    [
        (1, 0.05, [False, True, True, True]),  # a splinter from its root joint
        (1, 0.95, [False, False, True, True]),  # a splinter from its tip joint
        (3, 0.5, [False, False, False, True]),  # the last segment, which goes whole
        (3, 0.95, [False] * 4),  # a splinter off the rod's end, which is no cut
        (0, 0.0, [False, True, True, True]),  # the first body, which hangs from nothing
    ],
)
def test_a_cut_next_to_a_joint_is_made_at_it(rig, body, at, loose):
    """Nothing is resized: a body that small would have no mass to divide by."""
    scale = rig.model.shape_scale.numpy()
    assert rig.shears.cut(rig.bodies[body], at) == any(loose)

    assert rig.shears.loose.tolist() == loose
    assert (rig.model.shape_scale.numpy() == scale).all()


def test_a_landed_piece_lies_on_the_ground(rig):
    """The rods do not collide with the ground, so each body is laid on it once
    it is found below it -- level, whichever way it came down -- and stays."""
    ground = 0.8
    rig.shears.cut(rig.bodies[1], 0.5)
    # Tipped up at its free end, the way a piece of a leaning shoot falls.
    spin = np.zeros((4, 6), dtype=np.float32)
    spin[2:, 4] = 2.0
    rig.state.body_qd.assign(spin)
    for _ in range(40):
        rig.step(0.02)
        rig.shears.settle(lambda x, y: ground)

    assert (rig.model.body_flags.numpy()[rig.bodies[2:]] == int(BodyFlags.KINEMATIC)).all()
    start, end = rig.shears._capsules()
    assert start[2:, 2] == pytest.approx(ground + 0.01)
    assert end[2:, 2] == pytest.approx(ground + 0.01)
    rig.step(0.2)
    assert np.stack(rig.shears._capsules())[:, 2:] == pytest.approx(np.stack([start, end])[:, 2:])
