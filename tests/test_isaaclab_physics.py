"""Tests for the physics a vineyard runs under.

Requires Isaac Lab; skipped entirely where it isn't installed. What is pinned
here is the agreement with two things this module cannot see: the prim name
the Rust generator gives a flexible organ (`CABLE` in `src/scene/mod.rs`), and
the body labels Newton's rod importer derives from it -- and, for the spawner,
that the scene it spawns is one the backend in force can step.
"""

from __future__ import annotations

import re
import types

import pytest

# Not `pytest.importorskip`: `vinerylab.isaaclab` re-raises a missing Isaac Lab
# under its own message, and pytest skips only when the error names the module
# it was asked to import.
try:
    from vinerylab.isaaclab import ShootCfg, VineyardCfg, physics, vineyard
except ImportError:
    pytest.skip("Isaac Lab is not installed", allow_module_level=True)

from isaaclab.physics import PhysicsEvent  # noqa: E402
from isaaclab.sim import SimulationContext  # noqa: E402
from isaaclab_newton.physics import (  # noqa: E402
    MJWarpSolverCfg,
    NewtonCfg,
    NewtonManager,
    VBDSolverCfg,
)
from isaaclab_physx.physics import PhysxCfg  # noqa: E402
from newton import JointType  # noqa: E402

VINEYARD = "/World/Vineyard"
ROBOT = "/World/envs/env_0/Robot"
FLEXIBLE = VineyardCfg(shoot=ShootCfg(stray=0.05))
"""A vineyard with flexible shoots, as far as a cfg can promise any."""


@pytest.fixture
def cfg() -> object:
    return physics.make_physics_cfg_newton(FLEXIBLE, ROBOT, [f"{ROBOT}/.*FOOT"])


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
    # No vineyard path in the selector: the label alone keeps the robot out.
    assert not re.fullmatch(f"(?:{selector})(?:/.*)?", f"{ROBOT}/LF_FOOT")


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


@pytest.mark.parametrize(
    "shoot", [ShootCfg(), ShootCfg(stray=0.05, flexible=False)], ids=["held", "static strays"]
)
def test_a_vineyard_without_flexible_shoots_runs_on_mjwarp_alone(shoot):
    """A scene with no rod in it has nothing for a second entry to own -- the
    coupler refuses an entry that matches no body. The robot still wants its
    substeps: `NewtonCfg` defaults to one, where the quadruped falls over."""
    cfg = physics.make_physics_cfg_newton(VineyardCfg(shoot=shoot), ROBOT, [ROBOT])
    assert isinstance(cfg.solver_cfg, MJWarpSolverCfg)
    assert cfg.num_substeps == physics.SUBSTEPS


@pytest.mark.parametrize(
    ("backend", "steps"),
    [
        (NewtonCfg(), False),
        (PhysxCfg(), False),
        (NewtonCfg(solver_cfg=VBDSolverCfg()), True),
        (physics.make_physics_cfg_newton(FLEXIBLE, ROBOT, [ROBOT]), True),
    ],
    ids=["mjwarp", "physx", "vbd", "coupled"],
)
def test_only_a_vbd_solver_steps_a_rod(backend, steps):
    assert physics.steps_rods(backend) is steps


@pytest.fixture
def running(monkeypatch) -> types.SimpleNamespace:
    """A simulation running on the backend the test sets -- none while it is
    unset -- and a record of whether the rods were tuned for it."""
    state = types.SimpleNamespace(physics=None, tuned=False)
    monkeypatch.setattr(
        SimulationContext,
        "instance",
        lambda: types.SimpleNamespace(cfg=state) if state.physics is not None else None,
    )
    monkeypatch.setattr(vineyard, "tune_shoots", lambda: setattr(state, "tuned", True))
    return state


@pytest.mark.parametrize(
    ("backend", "flexible", "tuned"),
    [
        (None, True, False),
        (NewtonCfg(), False, False),
        (PhysxCfg(), False, False),
        (NewtonCfg(solver_cfg=VBDSolverCfg()), True, True),
    ],
    ids=["no sim", "mjwarp", "physx", "vbd"],
)
def test_the_spawner_keeps_the_scene_to_what_the_backend_can_step(
    running, backend, flexible, tuned
):
    """The physics config built for a vineyard is provisional -- a `--physics`
    override or a preset can replace it before the scene is built -- and the
    spawner is the one place that sees both. MJWarp cannot build a model that
    holds a rod, so under a backend with no rod solver the strays go static;
    under one with, they are tuned."""
    running.physics = backend
    spawned = vineyard.for_backend(FLEXIBLE)
    assert spawned.shoot.flexible is flexible
    assert running.tuned is tuned
    assert FLEXIBLE.shoot.flexible, "the caller's cfg is left as it was"


def test_a_vineyard_with_nothing_to_bend_is_spawned_as_it_is(running):
    running.physics = NewtonCfg(solver_cfg=VBDSolverCfg())
    held = VineyardCfg()
    assert vineyard.for_backend(held) is held
    assert not running.tuned


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


def test_a_rods_linear_slots_are_softened_and_its_angular_ones_stiffened(builder):
    """Two factors, one per pair: stretch and shear are what the solver
    converges worst on, and bend and twist are what a cane stands up with."""
    physics.tune_shoots(stiffen=10.0, damping=0.0, stretch=0.5)
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)

    assert builder.joint_target_ke == pytest.approx([1.0] * 6 + [100.0, 50.0, 40.0, 20.0] + [1.0])


def test_only_a_rods_angular_slots_take_damping_from_their_own_stiffness(builder):
    """A fraction of each slot's own stiffness is a time constant, so bend and
    twist settle alike. Stretch and shear are left undamped: VBD adds damping
    to a slot's stiffness every sweep, and there it keeps the canes swinging."""
    physics.tune_shoots(stiffen=1.0, damping=0.1, stretch=1.0)
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)

    assert builder.joint_target_kd == pytest.approx([0.0] * 8 + [0.4, 0.2] + [0.0])


def test_tune_shoots_registers_once_while_its_registration_stands(builder):
    """The spawner and a script may both call it. A second callback would
    multiply the joints over again, and nothing would report it."""
    first = physics.tune_shoots(stiffen=10.0, damping=0.0)
    assert physics.tune_shoots(stiffen=10.0, damping=0.0) is first
    NewtonManager.dispatch_event(PhysicsEvent.MODEL_INIT)
    assert builder.joint_target_ke[8] == pytest.approx(40.0)

    # Dropped by the manager when the simulation stops; the next call stands anew.
    NewtonManager.clear_callbacks()
    assert physics.tune_shoots() is not first


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
