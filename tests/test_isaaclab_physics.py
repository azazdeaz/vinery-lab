"""Tests for the coupled physics config that bends the flexible shoots.

Requires Isaac Lab; skipped entirely where it isn't installed. What is pinned
here is the agreement with two things this module cannot see: the prim name
the Rust generator gives a flexible organ (`CABLE` in `src/scene/mod.rs`), and
the body labels Newton's rod importer derives from it.
"""

from __future__ import annotations

import re
import types

import pytest

# Not `pytest.importorskip`: `vinerylab.isaaclab` re-raises a missing Isaac Lab
# under its own message, and pytest skips only when the error names the module
# it was asked to import.
try:
    from vinerylab.isaaclab import physics
except ImportError:
    pytest.skip("Isaac Lab is not installed", allow_module_level=True)

from isaaclab.physics import PhysicsEvent  # noqa: E402
from isaaclab_newton.physics import NewtonManager  # noqa: E402
from newton import JointType  # noqa: E402

VINEYARD = "/World/Vineyard"
ROBOT = "/World/envs/env_0/Robot"


@pytest.fixture
def cfg() -> object:
    return physics.make_coupled_physics_cfg(VINEYARD, ROBOT, [f"{ROBOT}/.*FOOT"])


@pytest.fixture
def entries(cfg) -> dict:
    return {entry.name: entry for entry in cfg.solver_cfg.entries}


def test_the_cable_selector_matches_the_bodies_newton_builds(entries):
    """A rod's capsules have no prim of their own: the importer labels them
    after the curve's path, and this regex is the only handle on them. Selecting
    nothing is an error at build time, so this fails loudly -- but only once a
    simulator is running, which is late."""
    (selector,) = entries["shoots"].bodies
    label = f"{VINEYARD}/Planting/Row_00/Vine_007/Shoot_03_1/{physics.CABLE}_edge_body_5"

    # The coupler wraps a selector this way before matching it in full.
    assert re.fullmatch(f"(?:{selector})(?:/.*)?", label)
    # The curve prim itself is not a body, and the vineyard's meshes are not rods.
    assert not re.fullmatch(selector, label.removesuffix("_edge_body_5"))


def test_the_robot_owns_the_static_scene(entries):
    """A static shape belongs to exactly one entry, and an entry that lists any
    shape stops seeing the rest. On the wrong entry the robot walks through the
    terrain."""
    assert entries["rigid"].include_static_shapes
    assert not entries["shoots"].include_static_shapes


def test_only_the_named_robot_bodies_can_bend_a_shoot(cfg):
    """The proxy mapping is the whole of the coupling: a body outside it passes
    through a cane without touching it."""
    (proxy,) = cfg.solver_cfg.proxies
    assert (proxy.source, proxy.destination) == ("rigid", "shoots")
    assert proxy.bodies == [f"{ROBOT}/.*FOOT"]


def test_the_shoots_step_finer_than_the_robot(cfg, entries):
    """Rod joints lose a cane that is standing upright once their step grows
    past about a millisecond. Taking the extra substeps on the entry keeps the
    cost off the robot and off the coupling passes."""
    assert cfg.num_substeps >= 4
    assert entries["shoots"].substeps > entries["rigid"].substeps


@pytest.fixture
def builder(monkeypatch) -> object:
    """A model builder holding one rod joint between two of something else.

    Four slots per rod, starting at the joint's own DoF: a walk that reads the
    start index or the slot count wrong writes into a neighbouring joint
    instead, and nothing about that fails loudly.

    Bodies 0 and 1 are the rod's, carrying shapes 0 and 1; bodies 2 and 3 stand
    for the robot, carrying shapes 2 and 3; shapes 4 and 5 are the static scene,
    which is body -1 because a static shape has no body. The other two joints
    are there to be walked past.
    """
    builder = types.SimpleNamespace(
        joint_type=[JointType.D6, JointType.ROD, JointType.REVOLUTE],
        joint_qd_start=[0, 6, 10],
        joint_target_ke=[1.0] * 6 + [200.0, 100.0, 4.0, 2.0] + [1.0],
        joint_target_kd=[0.0] * 11,
        joint_parent=[3, 0, 2],
        joint_child=[2, 1, 3],
        body_shapes={-1: [4, 5], 0: [0], 1: [1], 2: [2], 3: [3]},
        shape_collision_group=[1] * 6,
    )
    monkeypatch.setattr(NewtonManager, "_builder", builder, raising=False)
    # `tune_shoots` registers a callback per call, and nothing here deregisters.
    NewtonManager.clear_callbacks()
    return builder


def test_only_a_rods_angular_slots_are_stiffened(builder):
    """Stretch and shear already hold a cane together; raising them only
    shortens the substep it survives. The two angular slots are what a cane
    stands up with."""
    physics.tune_shoots(stiffen=10.0, damping=0.0)
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)

    assert builder.joint_target_ke == pytest.approx([1.0] * 6 + [200.0, 100.0, 40.0, 20.0] + [1.0])


def test_every_rod_slot_takes_damping_from_its_own_stiffness(builder):
    """The four slots differ by orders of magnitude, so one damping in absolute
    units would be either nothing or a clamp. A fraction of each slot's own
    stiffness is a time constant, and the same number then means the same
    settling in all four."""
    physics.tune_shoots(stiffen=1.0, damping=0.1)
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)

    assert builder.joint_target_kd == pytest.approx([0.0] * 6 + [20.0, 10.0, 0.4, 0.2] + [0.0])


def test_the_rod_capsules_and_the_static_scene_share_a_group(builder):
    """The robot has to keep the default group: it is what the rods are left
    colliding with. A walk that reached its bodies -- following the wrong
    joints, or a rod joint's own DoF index instead of its bodies -- would take
    the robot out of the rods' reach and nothing would report it."""
    physics.tune_shoots()
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)

    group = physics.SHOOT_GROUP
    assert builder.shape_collision_group == [group, group, 1, 1, group, group]


def test_the_shoot_group_drops_rod_pairs_and_keeps_the_robot(builder):
    """Pinned against Newton's own test rather than restated, because the
    convention is a bare integer sign with nothing to make a change in it
    fail: were negative groups to start colliding with their own, the scene
    would still run and quietly cost N(N-1)/2 candidate pairs again."""
    from newton import ModelBuilder

    collides = ModelBuilder()._test_group_pair
    robot = 1  # `ModelBuilder.ShapeConfig.collision_group`'s default.

    assert not collides(physics.SHOOT_GROUP, physics.SHOOT_GROUP)
    assert collides(physics.SHOOT_GROUP, robot)
    assert collides(robot, physics.SHOOT_GROUP)
