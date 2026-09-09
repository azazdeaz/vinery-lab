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

SHOOT_SUBSTEPS = 4
"""Substeps the shoots take inside each of those.

They want a shorter step still -- at one substep the canes sway without ever
settling -- and taking it here rather than by raising `SUBSTEPS` keeps the cost
off the robot and off the coupling passes between them.
"""


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
        A physics config to hand to `SimulationCfg(physics=...)`.
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
