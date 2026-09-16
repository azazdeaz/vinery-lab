"""Physics for a vineyard whose shoots bend.

A flexible shoot is imported as a **rod**: one capsule rigid body per segment,
joined by spring joints and clamped to the wood it grew from. Only Newton's VBD
solver steps one, and a quadruped needs MuJoCo, so the two run side by side as
named entries of a coupled solver with the robot's own bodies handed across as
proxies.

Everything a caller cannot know -- what the cable prims are called, what the
rod bodies end up labelled, which entry has to own the ground -- is decided
here. What a caller *does* know, the robot and the parts of it a shoot may
touch, are the two arguments.
"""

from __future__ import annotations

from collections.abc import Sequence

from isaaclab.physics import PhysicsEvent
from isaaclab_contrib.coupling import (
    CouplerEntryCfg,
    CouplerProxyCfg,
    CouplerProxyMappingCfg,
)
from isaaclab_newton.physics import MJWarpSolverCfg, NewtonCfg, VBDSolverCfg

CABLE = "Cable"
"""The prim name every flexible organ's curve takes.

The generator picks it -- `CABLE` in `src/scene/mod.rs` -- and it survives into
the body labels below, so the two have to be changed together."""

_ROD_BODY_SUFFIX = r"_edge_body_\d+"
"""What Newton's rod importer appends to the curve's path for each capsule it
builds. Cable bodies have no prim of their own, so this is the only way to name
them."""

SUBSTEPS = 4
"""Physics substeps per simulation step.

Not a preference: a rod's joints are stiff enough that a shoot standing upright
collapses instead of settling once the substep is longer than about a
millisecond. At the 200 Hz a locomotion policy wants, four is the floor.
"""

SHOOT_SUBSTEPS = 2
"""Substeps the shoots take inside each of those.

They want a shorter step still: at one, a settled cane still drifts about a
centimeter and keeps moving; at two it holds to a few millimeters. Four halves
that again for twice the cost, which buys nothing anyone can see. Taking the
extra steps here rather than by raising `SUBSTEPS` keeps them off the robot and
off the coupling passes between the two.
"""


SHOOT_STIFFEN = 100.0
"""What a rod joint's bend and twist stiffness is multiplied by.

Not a preference. VBD solves a rod chain with `VBDSolverCfg.iterations` (ten)
Gauss-Seidel sweeps per substep, and ten sweeps leave a cane's joints holding
about an eighth of the stiffness they were given; what a sweep does not
converge becomes velocity. At the stiffness the cable material implies -- about
3 N.m/rad for a 9 mm shoot -- that eighth is a hinge too soft to carry the
cane. It swings from its first few joints as a pendulum, through a metre of
height at the pendulum's own period, for as long as the run lasts. Multiplied
by this it droops a few centimetres and stops. Forty sweeps at thirty times
hold the same pose for about twice the cost; four hundred at the authored
stiffness restore the droop but not the calm.

Bend and twist only. Stretch and shear already hold the chain together, and
raising them shortens the substep a rod stays stable at -- see `SUBSTEPS`.
"""

SHOOT_DAMPING = 0.01
"""Damping a rod joint takes as a fraction of its own stiffness, in seconds.

Newton builds every rod joint undamped -- the cable importer never passes
`add_rod` a damping, and the curve material schema has no attribute to carry
one -- so a stiffened cane rings around its drooped pose instead of arriving at
it. This much takes the ring out within a second or two.

It cannot be more. VBD folds damping into every sweep's stiffness as `kd / dt`,
and a chain that stiff converges worse in ten sweeps, not better: at a tenth
the canes give way further under their own weight than undamped, at a fifth
they swing harder than with no damping at all. Damping in this solver softens
a rod before it slows one.
"""


def tune_shoots(stiffen: float = SHOOT_STIFFEN, damping: float = SHOOT_DAMPING) -> None:
    """Stiffen every rod joint against gravity and give it damping.

    Call once from inside the running app and **before the first
    `SimulationContext.reset()`**: that is what builds the model, and the
    solver copies a rod's stiffness and damping out of the model when it is
    constructed and never looks again. The only window to write them is the
    builder's -- after the importer has filled it, before it is finalized --
    and Isaac Lab dispatches `MODEL_INIT` in exactly that window.

    Without this a cane swings from its base like a pendulum for as long as
    the run lasts; see `SHOOT_STIFFEN`.
    """
    # Imported here and not at module scope: `newton` brings `pxr` with it, and
    # Kit's own `pxr` wins the import only if nothing loaded the pip one first.
    from isaaclab_newton.physics import NewtonManager
    from newton import JointType

    def tune(_payload) -> None:
        builder = NewtonManager._builder
        for joint, kind in enumerate(builder.joint_type):
            if kind != JointType.ROD:
                continue
            # A rod's four slots, in the builder's order: stretch, shear, bend,
            # twist.
            dof = builder.joint_qd_start[joint]
            for slot in (dof + 2, dof + 3):
                builder.joint_target_ke[slot] *= stiffen
            for slot in range(dof, dof + 4):
                builder.joint_target_kd[slot] = damping * builder.joint_target_ke[slot]

    NewtonManager.register_callback(tune, PhysicsEvent.MODEL_INIT)


def make_coupled_physics_cfg(
    vineyard: str,
    robot: str,
    contact_bodies: Sequence[str],
    substeps: int = SUBSTEPS,
) -> NewtonCfg:
    """MuJoCo for the robot, VBD for the vineyard's flexible shoots.

    Args:
        vineyard: Prim path the vineyard was spawned at, as a regex.
        robot: Prim path of the articulation MuJoCo steps, as a regex.
        contact_bodies: Robot bodies a shoot may touch, as full-label regexes.
            These are handed to the VBD half as proxies, and **nothing outside
            this list can bend a shoot** -- a leg that is not named passes
            straight through one.

    Returns:
        A physics config to hand to `SimulationCfg(physics=...)`. The shoots
        also want `tune_shoots` called once the app is up.
    """
    return NewtonCfg(
        solver_cfg=CouplerProxyCfg(
            entries=[
                CouplerEntryCfg(
                    name="rigid",
                    solver_cfg=MJWarpSolverCfg(),
                    bodies=[robot],
                    # The ground and the trellis. A static shape belongs to
                    # exactly one entry -- an entry that lists any shape stops
                    # seeing the rest -- and the robot walking on the terrain
                    # is the one that cannot do without it. The shoots then
                    # pass through the ground, which is free: they hang off the
                    # wood and never reach it.
                    include_static_shapes=True,
                ),
                CouplerEntryCfg(
                    name="shoots",
                    solver_cfg=VBDSolverCfg(),
                    bodies=[rf"{vineyard}/.*/{CABLE}{_ROD_BODY_SUFFIX}"],
                    substeps=SHOOT_SUBSTEPS,
                ),
            ],
            proxies=[
                CouplerProxyMappingCfg(
                    source="rigid",
                    destination="shoots",
                    bodies=list(contact_bodies),
                    # Refresh the proxies' contacts every pass: a walking robot
                    # moves a shoot's width within one step.
                    collide_interval=1,
                )
            ],
        ),
        num_substeps=substeps,
    )
