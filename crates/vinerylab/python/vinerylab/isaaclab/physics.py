"""The vineyard's physics: `misina_lab.isaaclab.rods` with the vineyard's names
and numbers.

Nothing in a vineyard is a rigid body except a flexible shoot, imported as a
rod. `VineyardCfg.without_rods` says whether a vineyard authors one, and the
core's spawner and `rods.make_physics_cfg_newton` read it there; what stays
here is which prim a vine is, and how many substeps the demos' robots need.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import TYPE_CHECKING

from misina_lab.isaaclab import rods
from misina_lab.isaaclab.rods import CABLE, PLANT_GROUPS, SCENE_GROUP, steps_rods

from .vineyard_cfg import VineyardCfg

if TYPE_CHECKING:
    from isaaclab.physics.physics_manager import CallbackHandle
    from isaaclab_newton.physics import NewtonCfg

VINE = VineyardCfg.PLANT
"""The prim name every vine takes, its index appended -- `PART` in
`crates/vinerylab/src/elements/vine.rs`. A vine's canes and its wood collide
as one group; see `rods.tune_rods`."""

VINE_GROUPS = PLANT_GROUPS
"""The first collision group a vine takes; each vine gets the next one up.
See `rods.PLANT_GROUPS`."""

SUBSTEPS = 4
"""Physics substeps per simulation step, with or without shoots.

The quadruped's number. ANYmal-C tips over on the demo's headland turns, where
it pivots on the spot, once MJWarp steps it coarser: on two-minute walks of the
route it fell on three of seven at two substeps, as many or more at one, and on
none at four. Three did not fall, but leaned further on the same turns. Plain
MJWarp fails the same way as the coupler, so the short step is the robot's, not
the shoots'.

The straddler drives its rows the same at two, about a fifth faster headless;
a scene with no other robot can pass `substeps=2` to `make_physics_cfg_newton`.
Its pushed canes then take about a tenth longer to settle. The rod tuning
assumes the 1.25 ms step four gives with `rods.ROD_SUBSTEPS`: at one substep
the shoots sag about twice as far.
"""


def tune_shoots(
    stiffen: float = rods.ROD_STIFFEN,
    damping: float = rods.ROD_DAMPING,
    stretch: float = rods.ROD_STRETCH,
) -> CallbackHandle:
    """`rods.tune_rods` for a vineyard: a rod meets its own vine's canes and
    wood, the ground, the posts and the robot, and passes through every other
    vine."""
    return rods.tune_rods(VINE, stiffen=stiffen, damping=damping, stretch=stretch)


def make_physics_cfg_newton(
    vineyard: VineyardCfg,
    robot: str,
    contact_bodies: Sequence[str],
    substeps: int = SUBSTEPS,
) -> NewtonCfg:
    """`rods.make_physics_cfg_newton` for a vineyard, at the quadruped's
    `SUBSTEPS` unless told otherwise."""
    return rods.make_physics_cfg_newton(vineyard, robot, contact_bodies, substeps)


__all__ = [
    "CABLE",
    "SCENE_GROUP",
    "SUBSTEPS",
    "VINE",
    "VINE_GROUPS",
    "make_physics_cfg_newton",
    "steps_rods",
    "tune_shoots",
]
