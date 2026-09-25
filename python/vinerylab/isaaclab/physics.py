"""Newton physics for a generated vineyard.

Nothing in a vineyard is a rigid body except a flexible shoot, imported as a
**rod**: one capsule rigid body per segment, joined by spring joints and
clamped to the wood it grew from. Only Newton's VBD solver steps one, and a
robot needs MuJoCo, so a scene with any runs the two side by side as named
entries of a coupled solver with the robot's own bodies handed across as
proxies. A scene without runs plain MJWarp, which cannot build a model that
holds a rod at all.

`make_physics_cfg_newton` picks between the two from the vineyard's cfg. The
choice is provisional -- Isaac Lab's `--physics` override, a Hydra preset or a
plain edit can each replace it before the scene is built -- so `spawn_vineyard`
checks the vineyard against the backend in force when it runs: the strays are
spawned static where nothing steps a rod, and the rods are tuned where
something does.

Everything a caller cannot know -- what the cable prims are called, what the
rod bodies end up labelled, which entry has to own the ground -- is decided
here. What a caller *does* know, the robot and the parts of it a shoot may
touch, are the arguments.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import TYPE_CHECKING

from isaaclab.physics import PhysicsCfg, PhysicsEvent
from isaaclab_contrib.coupling import (
    CouplerEntryCfg,
    CouplerProxyCfg,
    CouplerProxyMappingCfg,
)
from isaaclab_newton.physics import MJWarpSolverCfg, NewtonCfg, VBDSolverCfg

if TYPE_CHECKING:
    from isaaclab.physics.physics_manager import CallbackHandle

    from .vineyard_cfg import VineyardCfg

CABLE = "Cable"
"""The prim name every flexible organ's curve takes.

The generator picks it -- `CABLE` in `src/scene/mod.rs` -- and it survives into
the body labels below, so the two have to be changed together."""

_ROD_BODY_SUFFIX = r"_edge_body_\d+"
"""What Newton's rod importer appends to the curve's path for each capsule it
builds. Cable bodies have no prim of their own, so this is the only way to name
them."""

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
Its pushed canes then take about a tenth longer to settle. The shoot tuning
assumes the 1.25 ms step four gives with `SHOOT_SUBSTEPS`: at one substep the
shoots sag about twice as far.
"""

SHOOT_SUBSTEPS = 1
"""Substeps the shoots take inside each of those.

One. More settles a cane worse, not better: VBD adds a joint's damping to its
stiffness as `kd / dt` every sweep, so the shorter the step the more that term
swamps the rest and the less ten sweeps converge. At two, a cane shoved at the
robot's cruising speed takes several times as long to come within a centimetre
of rest, and some end centimetres off where they started.
"""


SHOOT_STIFFEN = 3.0
"""What a rod joint's bend and twist stiffness is multiplied by.

A trade between the pose the generator drew and a believable swing. A
cantilever's sag under its own weight and its first frequency are tied by
gravity alone -- the tip sags about 1.5 g / omega^2, whatever the mass and
stiffness -- so a cane swinging at the ~0.8 Hz its cable material implies sags
a quarter metre out of the drawn pose. At this factor a 1.4 m cane sags about
9 cm and swings at about 1.1 Hz.

Not lower. At two, `VBDSolverCfg.iterations` (ten) sweeps no longer hold a
chain that soft: a cane creeps for seconds after a push, and some come to rest
centimetres from where they started.
"""

SHOOT_STRETCH = 0.01
"""What a rod joint's stretch and shear stiffness is multiplied by.

Not a preference. VBD solves a rod chain by Gauss-Seidel sweeps, one body at a
time against its neighbours, and a joint far stiffer than a segment's inertia
over one step (m / dt^2) is what those converge worst on. The cable material
makes stretch and shear several hundred times that. The part a sweep leaves
unconverged becomes velocity: at the authored values, a cane soft enough to
swing like one never comes to rest, damped or not. A hundredth brings them
to a few times the inertia, which still holds a 1.4 m cane to under a
millimetre of stretch. A thousandth is too soft the other way, and the canes
stop settling again.
"""

SHOOT_DAMPING = 0.1
"""Damping a rod joint's bend and twist take as a fraction of their own
stiffness, in seconds.

Newton builds every rod joint undamped -- the cable importer never passes
`add_rod` a damping, and the curve material schema has no attribute to carry
one -- so a cane rings around its rest pose instead of arriving at it. This
much is a heavily damped swing, the way a leafy shoot moves in air: pushed
aside and let go, a cane swings back past its rest pose once, by about a fifth
of the push, and is within a centimetre of rest in one to three seconds.

Bend and twist only. VBD adds damping to a slot's stiffness as `kd / dt` every
sweep, so on stretch and shear it would make the slots that already converge
worst stiffer still; damped there, canes pump themselves into a swing that
never stops.
"""

SHOOT_GROUP = -2
"""Collision group the rod capsules and the static scene are moved into.

Not a preference. Newton appends one candidate pair per colliding shape pair
while `Model.finalize` builds `shape_contact_pairs`, and sizes every broad- and
narrow-phase buffer from that list, so N rod segments cost N(N-1)/2 pairs
before a step is taken: ten thousand segments is 55 million pairs and 4 GiB.
Filtering happens as the list is built, not inside a kernel, so a group that
declines a pair never allocates it.

A *negative* group collides with every group but its own, which is the one
thing the group algebra can express in O(1) per shape -- an explicit filter
pair per rod pair is the same quadratic moved onto the host. So this drops
rod-vs-rod, and keeps rod-vs-robot, since the robot keeps the default group 1
and positive meets negative.

The static scene joins the same group rather than getting one of its own,
because two *positive* groups do not collide either: a group that excluded the
rods would also stop the terrain from carrying the robot. Nothing is lost by
it. Under the coupled config `make_physics_cfg_newton` builds a static shape
belongs to the "rigid" entry, so a rod-vs-static pair is counted here and
solved by nobody -- the shoots already fall through the ground, the posts and
the wire.

Measured on a vineyard-shaped rig -- a mesh terrain, 500 static capsules and
2,002 rod segments -- stepped through the coupled solver: 407 MiB with
everything colliding, 213 MiB with the rods alone in this group, 125 MiB with
the static scene in it too.
"""


_tuning: CallbackHandle | None = None
"""The registration `tune_shoots` made, while it stands."""


def tune_shoots(
    stiffen: float = SHOOT_STIFFEN,
    damping: float = SHOOT_DAMPING,
    stretch: float = SHOOT_STRETCH,
) -> CallbackHandle:
    """Retune every rod joint's stiffness and damping, and stop rods colliding.

    `spawn_vineyard` calls this when it spawns rods under a solver that steps
    them, so a script calls it only to change the numbers: from inside the
    running app and **before the first `SimulationContext.reset()`**. That is
    what builds the model, and both the stiffnesses and the collision groups
    are read out of the builder when it is finalized and never looked at
    again. The only window to write them is the builder's -- after the
    importer has filled it, before it is finalized -- and Isaac Lab dispatches
    `MODEL_INIT` in exactly that window.

    Registers once. While the registration stands -- the manager drops it when
    the simulation stops -- a further call returns it unchanged, rather than
    multiplying the joints a second time.

    Without the tuning a cane sags out of its drawn pose and swings for as long
    as the run lasts; see `SHOOT_STIFFEN` and `SHOOT_STRETCH`. Without the
    grouping the scene pays N(N-1)/2 candidate pairs for collisions it never
    solves; see `SHOOT_GROUP`.

    `SHOOT_GROUP` is free under the coupled config `make_physics_cfg_newton`
    builds, where a rod-vs-static pair is solved by nobody anyway. Under VBD on
    its own it costs the rods their contact with the ground, the posts and the
    wire -- which they hang clear of.
    """
    global _tuning
    # Imported here and not at module scope: `newton` brings `pxr` with it, and
    # Kit's own `pxr` wins the import only if nothing loaded the pip one first.
    from isaaclab_newton.physics import NewtonManager
    from newton import JointType

    if _tuning is not None and _tuning.id in NewtonManager._callbacks:
        return _tuning

    def tune(_payload) -> None:
        builder = NewtonManager._builder
        for joint, kind in enumerate(builder.joint_type):
            if kind != JointType.ROD:
                continue
            # Both ends, so a chain's first body is reached too: it is the
            # first rod joint's parent and no rod joint's child.
            for body in (builder.joint_parent[joint], builder.joint_child[joint]):
                for shape in builder.body_shapes[body]:
                    builder.shape_collision_group[shape] = SHOOT_GROUP
            # A rod's four slots, in the builder's order: stretch, shear, bend,
            # twist.
            dof = builder.joint_qd_start[joint]
            for slot in (dof, dof + 1):
                builder.joint_target_ke[slot] *= stretch
            for slot in (dof + 2, dof + 3):
                builder.joint_target_ke[slot] *= stiffen
                builder.joint_target_kd[slot] = damping * builder.joint_target_ke[slot]

        # The static scene. `body_shapes` is keyed by body index and a static
        # shape has none, so -1 is the whole of it.
        for shape in builder.body_shapes[-1]:
            builder.shape_collision_group[shape] = SHOOT_GROUP

    _tuning = NewtonManager.register_callback(tune, PhysicsEvent.MODEL_INIT)
    return _tuning


def has_flexible_shoots(vineyard: VineyardCfg) -> bool:
    """Whether the vineyard authors a flexible shoot: a stray one, with
    `ShootCfg.flexible` on.

    `stray` is a share drawn shoot by shoot, so a parcel small enough can draw
    none; the coupled solver then refuses an entry that owns no body, at
    reset, with its own error. Set `stray` to zero for such a scene.
    """
    # ponytail: read off the cfg, as the scene itself is only generated inside
    # the app, after the physics config was built.
    return vineyard.shoot.flexible and vineyard.shoot.stray > 0.0


def steps_rods(physics: PhysicsCfg | None) -> bool:
    """Whether a physics config steps a rod: it has a VBD solver, on its own or
    as an entry of a coupled one. PhysX ignores a rod's curve schema, and
    MJWarp cannot build a model that holds one."""
    solver = getattr(physics, "solver_cfg", None)
    entries = getattr(solver, "entries", None)
    solvers = [entry.solver_cfg for entry in entries] if entries is not None else [solver]
    return any(isinstance(each, VBDSolverCfg) for each in solvers)


def make_physics_cfg_newton(
    vineyard: VineyardCfg,
    robot: str,
    contact_bodies: Sequence[str],
    substeps: int = SUBSTEPS,
) -> NewtonCfg:
    """Newton for a vineyard: plain MJWarp, or -- for a vineyard with flexible
    shoots -- MuJoCo for the robot coupled with VBD for the shoots.

    Args:
        vineyard: The vineyard to be spawned. Only whether it has flexible
            shoots is read; see `has_flexible_shoots`.
        robot: Prim path of the articulation MuJoCo steps, as a regex.
        contact_bodies: Robot bodies a shoot may touch, as full-label regexes.
            These are handed to the VBD half as proxies, and **nothing outside
            this list can bend a shoot** -- a leg that is not named passes
            straight through one.
        substeps: Physics substeps per simulation step, under either
            config; see `SUBSTEPS`.

    Returns:
        A physics config to hand to `SimulationCfg(physics=...)`.
    """
    if not has_flexible_shoots(vineyard):
        return NewtonCfg(num_substeps=substeps)
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
                    # Under any path: only the rod importer labels a body so.
                    bodies=[rf".*/{CABLE}{_ROD_BODY_SUFFIX}"],
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
