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

import re
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

VINE = "Vine"
"""The prim name every vine takes, its index appended.

The generator picks it -- `PART` in `src/elements/vine.rs`. A rod's body label
and a wood collider's shape label both pass through the vine's prim, which is
what puts a vine's canes and its wood in one collision group below."""

_VINE_PATH = re.compile(rf"(.*/{VINE}_[^/]*)/")
"""Matches a label up to and including its vine prim."""

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

VINE_GROUPS = 2
"""The first collision group a vine takes; each vine gets the next one up.

Not a preference. Newton appends one candidate pair per colliding shape pair
while `Model.finalize` builds `shape_contact_pairs`, and sizes every broad- and
narrow-phase buffer from that list, so N rod segments that may all meet cost
N(N-1)/2 pairs before a step is taken: ten thousand segments is 55 million
pairs and 4 GiB. Filtering happens as the list is built, not inside a kernel,
so a group that declines a pair never allocates it.

A group is one signed integer per shape. A positive group meets itself and
every negative one; a negative group meets everything but its own; zero meets
nothing. That is the whole algebra, and it is enough: a vine's rods and its
wood share a positive group, so a cut piece catches on its own vine's stub,
canes and cordon, and segments of different vines never pair. The ground, the
trellis and the robot's bodies are negative, so every rod meets them. The
robot takes a group per body, which pairs its parts exactly as the default
group did.

Measured on the pruning demo's row, 953 segments over twelve vines with a
trunk and two cordons each: 62,058 contact pairs against 19,364 with no rod
meeting a rod, at the same step time. Every rod meeting every other would be
454,000.
"""

SCENE_GROUP = -1
"""Collision group of the ground and of every static shape that is not a
vine's wood: negative, so every rod and the robot meet it, at one pair per rod
segment each. A post costs that much, a wire would too. The straddler demo's
widest scene, 9,800 segments among some eighty posts, pays 900,000 pairs and
8% of its step time for them."""


_tuning: CallbackHandle | None = None
"""The registration `tune_shoots` made, while it stands."""


def tune_shoots(
    stiffen: float = SHOOT_STIFFEN,
    damping: float = SHOOT_DAMPING,
    stretch: float = SHOOT_STRETCH,
) -> CallbackHandle:
    """Retune every rod joint's stiffness and damping, and group what collides.

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
    solves; see `VINE_GROUPS`.

    A rod meets its own vine's canes and wood, the ground, the posts and the
    robot, and passes through every other vine.
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
        rods: set[int] = set()
        for joint, kind in enumerate(builder.joint_type):
            if kind != JointType.ROD:
                continue
            # Both ends, so a chain's first body is counted too: it is the
            # first rod joint's parent and no rod joint's child.
            rods.update((builder.joint_parent[joint], builder.joint_child[joint]))
            # A rod's four slots, in the builder's order: stretch, shear, bend,
            # twist.
            dof = builder.joint_qd_start[joint]
            for slot in (dof, dof + 1):
                builder.joint_target_ke[slot] *= stretch
            for slot in (dof + 2, dof + 3):
                builder.joint_target_ke[slot] *= stiffen
                builder.joint_target_kd[slot] = damping * builder.joint_target_ke[slot]

        # Groups: a vine's for its canes and wood, allotted as they turn up;
        # `SCENE_GROUP` for what stands under no vine; one of its own for
        # every other body. Group 0 is a site, which collides with nothing,
        # and stays.
        vines: dict[str, int] = {}

        def vine_group(label: str) -> int:
            vine = _VINE_PATH.match(label)
            if vine is None:
                return SCENE_GROUP
            return vines.setdefault(vine.group(1), VINE_GROUPS + len(vines))

        def regroup(shapes: Sequence[int], group: int) -> None:
            for shape in shapes:
                if builder.shape_collision_group[shape] != 0:
                    builder.shape_collision_group[shape] = group

        # `body_shapes` is keyed by body index, and a static shape has none,
        # so -1 is the whole of the static scene.
        for body in range(builder.body_count):
            label = builder.body_label[body]
            group = vine_group(label) if body in rods else -(2 + body)
            regroup(builder.body_shapes.get(body, ()), group)
        for shape in builder.body_shapes[-1]:
            regroup((shape,), vine_group(builder.shape_label[shape]))

    _tuning = NewtonManager.register_callback(tune, PhysicsEvent.MODEL_INIT)
    return _tuning


def has_flexible_shoots(vineyard: VineyardCfg) -> bool:
    """Whether the vineyard authors a flexible shoot: a stray one, or every
    cane of a dormant vineyard, with `ShootCfg.flexible` on.

    `stray` is a share drawn shoot by shoot, so a parcel small enough can draw
    none; the coupled solver then refuses an entry that owns no body, at
    reset, with its own error. Set `stray` to zero for such a scene.
    """
    # ponytail: read off the cfg, as the scene itself is only generated inside
    # the app, after the physics config was built.
    shoot = vineyard.shoot
    return shoot.flexible and (shoot.stray > 0.0 or shoot.dormant)


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
                    # one entry at most, and the robot walking on the terrain
                    # is the one that cannot do without it.
                    include_static_shapes=True,
                ),
                CouplerEntryCfg(
                    name="shoots",
                    solver_cfg=VBDSolverCfg(),
                    # Under any path: only the rod importer labels a body so.
                    bodies=[rf".*/{CABLE}{_ROD_BODY_SUFFIX}"],
                    # An entry that lists no shape still sees its own bodies'
                    # capsules, and the static shapes another entry owns: the
                    # ground, for a cut piece to land on.
                    include_body_shapes=False,
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
