"""Choosing the cuts, and making them, from the scene's own ground truth.

What Bumblebee recovers from stereo -- the vine's canes, where each bud sits
on them, which cut leaves the right number -- the generated scene knows
outright. `read_vines` walks the stage for it: every cane is a rod, every
bud a prim under the rod segment that carries it, so a bud's place in the
running simulation is the segment's pose applied to the bud's own.

The rule is the paper's: keep `KEEP_BUDS` on every cane and cut midway
between the last kept bud and the next. Each cut is a pose for the shear's
mouth -- the blades along the approach, the pivot along the cane -- aimed
afresh when its turn comes and reached in two stages, a planned move to
`STANDOFF` out and a straight line in, and the cuts on a vine are taken
nearest neighbour first. Once the arm has settled the shear closes, and the
moving blade cuts what it sweeps through: the cut is wherever the
blade's edge reaches a cane's axis, made by `Shears` at that point, and a
cane the blade never reaches -- pushed out of the mouth on the way in, or
never in it -- is a miss. `Stroke` is that closing and opening, a tick at a
time, the moving blade's collider stood at the cane once the blades hold it
so the blade closes through the cane rather than crushing it; `Pruning` runs
the whole sequence one vine at a time, and `Tally` keeps the paper's own
score: cuts made, cuts made at the right place, and how long a vine took.
"""

from __future__ import annotations

import dataclasses
import math
from typing import TYPE_CHECKING

import numpy as np

from kinematics import frames, solve

if TYPE_CHECKING:
    from pxr import Usd

    from vinerylab.isaaclab import Shears

    from bumblebee import Bumblebee

KEEP_BUDS = 2
"""Buds left on every cane: a two-bud spur, as a spur-pruned cordon is cut to."""

STANDOFF = 0.15
"""How far out from the cut the arm plans to before closing in on a straight
line, in meters."""

CLOSE_IN = 0.01
"""How far the mouth moves per control tick on the straight approach, in
meters: half a metre a second, at the controller's rate."""

JOINT_STEP = 0.05
"""How far a joint moves per control tick on a planned move, in radians or
meters: a move takes as many ticks as its longest joint swing needs. It is
the arm's top speed, 2.5 rad/s at the controller's rate, under a UR5's
3.1."""

TIP = 1.2
"""How far the blades are turned down, in radians, to tip out a piece cut
free that lies in the mouth."""

FOUL = 0.02
"""How far off the blades' plane a point of a piece cut free may lie and
still be in the mouth, in meters: past the axis of a piece lying on a plate,
and past half the spacing of the points `Stroke.fouled` takes along a
capsule, so one crossing the plane has a point this close."""

ARRIVED = 0.08
"""How close every joint has to be to its target, in radians or meters, for
the arm to count as having arrived: the arm's weight holds the shoulder a
few hundredths short of a stiff drive's target."""

STILL = 0.002
"""How little every joint may still be moving per control tick, in radians
or meters, for the arm to count as settled: a tenth of a radian a second.
The approach ends once the arm is close to its target and settled on it, so
the shear closes on the cane where the arm holds it, not where the mouth is
passing through on its way there."""

BLADE_STEP = 0.03
"""How far the blade's target turns per control tick, closing or opening, in
radians: shut in thirty ticks, the better part of the second an electric
pruner takes, and the plate advances a couple of millimetres between one look
at what it has reached and the next, well within its own width."""

PATIENCE = 400
"""Control ticks a stage may take before the cut is given up on: eight
seconds at the controller's rate. An arm held off its pose by a post or a
wire, or a blade a post keeps from closing, would otherwise wait there for
the rest of the run."""

SLACK = 0.05
"""How far short of fully shut or fully open the blade may stop, in radians,
to count as there: the drive settles a little off its target."""

GRIP = 0.001
"""How far off a cane's surface the moving blade's collider stands once the
blades hold it, in meters: a hair clear of the wood, not in it."""

REACH_TOLERANCE = (0.01, 0.1)
"""How close the solver has to get a cut pose, in meters and radians, for
the cut to count as reachable."""

BUD = "Bud_"
"""What a bud prim is named, by the generator."""

SEEDS = ((1.57, -1.2, 1.2, -1.57, -1.57, 0.0), (1.57, -0.8, 1.6, -2.3, -1.57, 0.0))
"""Arm postures the solver restarts from when the one the arm is in folds it
away from the target: reaching out over the rail, elbow up, wrist turned
down. The slide stays where it is."""


@dataclasses.dataclass(frozen=True)
class Bud:
    """One bud, by the rod body that carries it and its place in that body's
    frame."""

    prim: str
    body: int
    offset: np.ndarray


@dataclasses.dataclass(frozen=True)
class Cane:
    prim: str
    buds: list[Bud]
    """From the base of the cane up."""


@dataclasses.dataclass(frozen=True)
class Vine:
    prim: str
    position: np.ndarray
    """The trunk's base, in world coordinates."""
    canes: list[Cane]


def read_vines(stage: Usd.Stage, root: str, row: int, shears: Shears) -> list[Vine]:
    """Every vine of row `row` under the vineyard at `root`, with its canes
    and their buds -- the ground truth a pruner plans on.

    A cane that is no rod -- one spawned static under a backend that bends
    none -- carries no buds here, since nothing could cut it.
    """
    from pxr import Usd, UsdGeom

    body_of = {label: body for body, label in enumerate(shears.labels)}
    rows = stage.GetPrimAtPath(f"{root}/Planting").GetChildren()
    vines = []
    for vine in rows[row].GetChildren():
        if not vine.GetName().startswith("Vine_"):
            continue
        canes = []
        for cane in vine.GetChildren():
            buds = []
            for segment in cane.GetChildren():
                body = body_of.get(str(segment.GetPath()))
                if body is None:
                    continue
                for bud in segment.GetChildren():
                    if bud.GetName().startswith(BUD):
                        offset = (
                            UsdGeom.Xformable(bud).GetLocalTransformation().ExtractTranslation()
                        )
                        buds.append(Bud(str(bud.GetPath()), body, np.array(offset, dtype=float)))
            if buds:
                buds.sort(key=lambda bud: int(bud.prim.rsplit("_", 1)[1]))
                canes.append(Cane(str(cane.GetPath()), buds))
        position = (
            UsdGeom.Xformable(vine)
            .ComputeLocalToWorldTransform(Usd.TimeCode.Default())
            .ExtractTranslation()
        )
        vines.append(Vine(str(vine.GetPath()), np.array(position, dtype=float), canes))
    return vines


def bud_position(bud: Bud, shears: Shears) -> np.ndarray:
    """Where `bud` is now, in world coordinates."""
    pose = shears.pose(bud.body)
    return pose[:3] + _rotate(pose[3:], bud.offset)


def attached(bud: Bud, shears: Shears, stage: Usd.Stage) -> bool:
    """Whether `bud` is still on its vine: its body was not cut free, and it
    was not hidden as the part of a body past a cut."""
    from pxr import UsdGeom

    loose = shears.loose[np.searchsorted(shears.bodies, bud.body)]
    hidden = UsdGeom.Imageable(stage.GetPrimAtPath(bud.prim)).ComputeVisibility() == "invisible"
    return not (loose or hidden)


@dataclasses.dataclass
class Cut:
    """One cut to make: where the mouth goes, and the buds either side of it
    that say afterwards whether it was made where it should be."""

    cane: Cane
    pose: np.ndarray
    """The mouth's frame at the cut, in world coordinates, as last aimed."""
    kept: Bud
    """The last bud to keep, below the cut."""
    removed: Bud
    """The first bud to take, above it."""
    push: np.ndarray = dataclasses.field(default_factory=lambda: np.zeros(3))
    """How far the mouth moves on from the cut before the shear closes, in
    the cut's own frame: x along the pivot, y across the mouth -- the fixed
    blade on its +y side, the moving one opening to -y -- and z along the
    blades. The edge the mouth moves into the cane pushes it: +z drives it
    into the crotch, -y against the fixed blade, +y against the moving one.
    Zero for a planned cut."""

    @property
    def standoff(self) -> np.ndarray:
        """The same frame `STANDOFF` back along the blades."""
        pose = self.pose.copy()
        pose[:3, 3] -= STANDOFF * pose[:3, 2]
        return pose

    @property
    def pushed(self) -> np.ndarray:
        """The same frame moved by `push`."""
        pose = self.pose.copy()
        pose[:3, 3] += pose[:3, :3] @ self.push
        return pose

    @property
    def tipped(self) -> np.ndarray:
        """The stand-off frame with the blades turned `TIP` down the cane,
        about the line across the mouth."""
        pose = self.standoff
        c, s = math.cos(TIP), math.sin(TIP)
        pose[:3, :3] = pose[:3, :3] @ np.array([[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]])
        return pose


def squared(point: np.ndarray, tangent: np.ndarray, approach: np.ndarray) -> np.ndarray:
    """The mouth's frame at `point`, squared up to a cane along `tangent`
    approached along `approach`, unit vectors at right angles: the pivot
    along the cane and the blades along the approach, so the cane lies
    across the mouth."""
    pose = np.eye(4)
    pose[:3, 0] = tangent
    pose[:3, 1] = np.cross(approach, tangent)
    pose[:3, 2] = approach
    pose[:3, 3] = point
    return pose


def aimed(kept: Bud, removed: Bud, shears: Shears, approach: np.ndarray) -> np.ndarray | None:
    """The mouth's frame for the cut midway between `kept` and `removed`,
    where they are now: squared up to the cane, approached as near along
    `approach` as lying across the cane allows. None where `approach` runs
    along the cane."""
    below, above = bud_position(kept, shears), bud_position(removed, shears)
    tangent = above - below
    tangent /= np.linalg.norm(tangent)
    approach = approach - (approach @ tangent) * tangent
    if np.linalg.norm(approach) < 1e-3:
        return None
    return squared((below + above) / 2, tangent, approach / np.linalg.norm(approach))


def plan(vine: Vine, shears: Shears, side: np.ndarray) -> list[Cut]:
    """The cuts that prune `vine` to `KEEP_BUDS` a cane, unordered.

    `side` is the horizontal direction from the robot to the row. The mouth
    approaches along it, squared up to the cane: blades along the approach,
    pivot along the cane, so the cane lies across the mouth.
    """
    cuts = []
    for cane in vine.canes:
        if len(cane.buds) <= KEEP_BUDS:
            continue
        kept, removed = cane.buds[KEEP_BUDS - 1], cane.buds[KEEP_BUDS]
        pose = aimed(kept, removed, shears, side)
        if pose is not None:
            cuts.append(Cut(cane, pose, kept, removed))
    return cuts


def nearest_first(cuts: list[Cut], start: np.ndarray) -> list[Cut]:
    """`cuts` in the order a greedy nearest-neighbour tour from `start`
    visits them -- Bumblebee's own ordering."""
    ordered, here = [], start
    left = list(cuts)
    while left:
        nearest = min(left, key=lambda cut: np.linalg.norm(cut.pose[:3, 3] - here))
        left.remove(nearest)
        ordered.append(nearest)
        here = nearest.pose[:3, 3]
    return ordered


def reached(result: tuple[np.ndarray, float, float]) -> bool:
    """Whether a `solve` result got its pose, within `REACH_TOLERANCE`."""
    return result[1] <= REACH_TOLERANCE[0] and result[2] <= REACH_TOLERANCE[1]


def reach(chain, tool: np.ndarray, pose: np.ndarray, q) -> tuple[np.ndarray, bool]:
    """Joint positions for `pose`, in the chain's base frame, from `q` or,
    failing that, from each of `SEEDS` with the slide left where it is; and
    whether any of them reached it. The closest try either way."""
    best = solve(chain, tool, pose, q)
    for seed in SEEDS:
        if reached(best):
            break
        best = min(best, solve(chain, tool, pose, [q[0], *seed]), key=lambda r: r[1])
    return best[0], reached(best)


class Stroke:
    """The shear's moving blade, driven a control tick at a time: shut,
    cutting what it sweeps through, or open again.

    The blade's collider follows the blade until the cane it carries across
    the mouth lies against the fixed blade. From there it stands where it is
    while the blade closes on through the cane -- nothing is crushed between
    the blades, and nothing the cane leans on is pulled from under it -- and
    follows the blade again once that has opened back past it.
    """

    def __init__(self, machine: Bumblebee, shears: Shears):
        # Imported here: `bumblebee` reaches Isaac Lab, which the planning
        # above does not need.
        from bumblebee import BLADE

        self.machine, self.shears = machine, shears
        self.jaw = machine.shear.opening
        """The blade angle to hold."""
        self.swept = 0
        """Canes cut on the way shut; cleared once the blade is sent open."""
        self.held: float | None = None
        """The blade angle the collider stands at, while the blade is shut
        past it."""
        self.angle = machine.shear.opening
        """The blade's angle at the last tick."""
        self.turned = 0.0
        """How far back from the blade the collider was last placed, in
        radians."""
        blade = next(i for i, label in enumerate(shears.labels) if label.endswith("/" + BLADE))
        self._shape, self._rest = shears.collider(blade)

    def tick(self, hand: np.ndarray, shear: float, closing: bool) -> bool:
        """Turn the target a `BLADE_STEP` shut, or open, and place the
        collider. `hand` is the pose of the body the shear is on in world
        coordinates and `shear` the blade's angle: shutting, the plate at
        that angle cuts whatever it has reached since the last tick. Returns
        whether the blade has got there -- the two blades met, or the mouth
        fully open."""
        s = self.machine.shear
        # The collider is written once a tick and rides the blade until the
        # next, so it is placed for where the blade will be by then.
        ahead = shear + min(0.0, shear - self.angle)
        self.angle = shear
        if closing:
            self.jaw = max(self.jaw - BLADE_STEP, 0.0)
            if self.held is None:
                self.held = self._pinch(hand, ahead, shear)
        else:
            self.jaw, self.swept = min(self.jaw + BLADE_STEP, s.opening), 0
            # Released once the blade has opened back to it, or fully open:
            # the drive need not reach an angle the collider was held at.
            if self.held is not None and shear >= min(self.held, s.opening - SLACK):
                self.held = None
        turned = 0.0 if self.held is None else max(0.0, self.held - ahead)
        if turned != self.turned:
            self.turned = turned
            self.shears.place(self._shape, _swung(self._rest, turned))
        if not closing:
            return shear > s.opening - SLACK
        # The blades meeting cut whatever still lies between them: the last
        # sweep is the plate at the stop, a thin cane's axis being closer to
        # it than the blade gets before it counts as shut.
        shut = shear < SLACK
        self.swept += self.shears.cut_through(
            *_placed(hand, *self.machine.blade(0.0 if shut else shear))
        )
        return shut

    def _pinch(self, hand: np.ndarray, ahead: float, shear: float) -> float | None:
        """The angle to stand the collider at: where a cane across the mouth,
        within the blades' reach, lies against the fixed blade and the moving
        edge reaches it by `ahead`, the angle at which it does -- or the
        blade's own `shear`, if that is past it already. None while no cane
        is held."""
        s = self.machine.shear
        hits, _, on = self.shears.crossing(*_placed(hand, *self.machine.wedge()))
        held = []
        for i, (a, b) in zip(hits, on, strict=True):
            y, z, radius = s.reach * (a - 1), s.blade * b, self.shears.radius[i] + GRIP
            within = -y <= radius and math.hypot(y, z) <= s.blade
            if within and (pinch := s.pinch(y, z, radius)) >= ahead:
                held.append(min(pinch, shear))
        # ponytail: a second cane in the mouth is cut unheld, since standing
        # at it would squeeze the first; two in one mouth is rare.
        return max(held, default=None)

    def fouled(self, hand: np.ndarray) -> bool:
        """Whether a piece cut free lies in the mouth, `hand` being the pose
        of the body the shear is on: one that fell across it or onto the
        blades, which the blade does not cut and the arm would carry on to
        the next cut."""
        s = self.machine.shear
        corner, u, v = _placed(hand, *self.machine.wedge())
        start, end = self.shears.capsules()
        loose = self.shears.loose
        # Points along every loose capsule, from the wedge's corner: a piece
        # lying flat on a plate crosses no plane of the mouth's.
        along = np.linspace(0.0, 1.0, 5)[:, None]
        points = start[loose, None] + along * (end - start)[loose, None] - corner
        normal = np.cross(u, v)
        a, b = points @ u / (u @ u), points @ v / (v @ v)
        off = np.abs(points @ normal) / np.linalg.norm(normal)
        # Out to the back of either plate, across the mouth.
        w = s.width / s.reach
        return bool(((-w <= a) & (a <= 1 + w) & (0 <= b) & (b <= 1) & (off <= FOUL)).any())


@dataclasses.dataclass
class Tally:
    """The paper's score, summed over the vines pruned so far."""

    vines: int = 0
    planned: int = 0
    reachable: int = 0
    made: int = 0
    correct: int = 0
    seconds: float = 0.0

    def summary(self) -> str:
        share = lambda part, whole: f"{100 * part / whole:.0f}%" if whole else "n/a"  # noqa: E731
        return (
            f"{self.vines} vines, {self.planned} cuts planned: "
            f"{share(self.reachable, self.planned)} reachable, "
            f"{share(self.made, self.planned)} made, "
            f"{share(self.correct, self.planned)} made at the right place, "
            f"{self.seconds / max(self.vines, 1):.0f} s per vine"
        )


class Pruning:
    """One vine's pruning, cut by cut, driven a control tick at a time.

    `control` returns the slide and arm positions to hold and the blade angle
    to hold, and moves the sequence on as each stage's target is reached: a
    planned move out to the stand-off, a straight line in -- and on by the
    cut's `push`, if it has one -- the shear closing once the arm has settled,
    the blade cutting what it sweeps through, and opening, and the straight
    line back out -- then the blades tipped down while a piece cut free lies
    in the mouth. Once the last cut is made the arm folds home, and the
    vine is done. Poses are solved in the robot's own frame, so the caller
    hands in where the robot is.
    """

    def __init__(
        self,
        machine: Bumblebee,
        cuts: list[Cut],
        base: np.ndarray,
        q: np.ndarray,
        shears: Shears,
        stage: Usd.Stage,
        tally: Tally,
    ):
        # Imported here, as in `Stroke`.
        from bumblebee import ARM_HOME

        self.home = np.array([0.0, *ARM_HOME])
        """The slide and arm positions the arm folds back to once the vine is
        pruned, to ride to the next."""
        self.chain, self.tool = machine.chain, machine.tool
        self.shears, self.stage, self.tally = shears, stage, tally
        self.base = base
        self.q = np.array(q, dtype=float)
        self.moved = 0.0
        """How far the joint that moved most moved over the last tick."""
        self.stroke = Stroke(machine, shears)
        self.cuts = self._reachable(cuts)
        self.tally.planned += len(cuts)
        self.tally.reachable += len(self.cuts)
        self.cut: Cut | None = None
        self.stage_name = "move"
        self.ticks = 0
        self.target = self.q.copy()
        self.goal = self.q.copy()
        self._next()

    def _reachable(self, cuts: list[Cut]) -> list[Cut]:
        """The cuts the arm can pose for, from where it stands now."""
        return [
            cut for cut in cuts if all(self._solve(pose)[1] for pose in (cut.standoff, cut.pose))
        ]

    def _solve(self, pose: np.ndarray) -> tuple[np.ndarray, bool]:
        """Joint positions for the world pose `pose`, from where the arm is,
        and whether it reaches it."""
        return reach(self.chain, self.tool, self._local(pose), self.q)

    def _local(self, pose: np.ndarray) -> np.ndarray:
        """`pose` in the robot's base frame."""
        return np.linalg.inv(self.base) @ pose

    def _next(self) -> None:
        """Start on the next cut, or finish."""
        # A cut whose bud to take is gone already -- taken by another cut's
        # blade, or by a piece falling across it -- is left alone: aimed at
        # where that bud is now, it would chase the piece to the ground.
        while self.cuts and not attached(self.cuts[0].removed, self.shears, self.stage):
            self.cuts.pop(0)
        self.cut = self.cuts.pop(0) if self.cuts else None
        self.ticks = 0
        if self.cut is None:
            self.stage_name, self.goal = "home", self.home
            return
        self.stage_name = "move"
        # Aimed afresh at where the buds are now: a cane may have been pushed
        # since the vine was planned, by the arm passing or a piece falling.
        pose = aimed(self.cut.kept, self.cut.removed, self.shears, self.cut.pose[:3, 2])
        if pose is not None:
            self.cut.pose = pose
        self.goal, _ = self._solve(self.cut.standoff)
        self.line = self.cut.standoff[:3, 3].copy()

    @property
    def done(self) -> bool:
        return self.stage_name == "done"

    def control(self, q: np.ndarray, hand: np.ndarray, shear: float) -> tuple[np.ndarray, float]:
        """Advance one tick. `q` is where the slide and arm are, `hand` the
        pose of the body the shear is on in world coordinates and `shear`
        the blade's angle. Returns the positions to hold and the blade angle
        to hold."""
        self.moved = float(np.abs(np.asarray(q, dtype=float) - self.q).max())
        self.q = np.array(q, dtype=float)
        # The robot's frame as it stands now: the chassis creeps on its
        # wheels while the arm works, and a pose solved against where it
        # stood when the vine was reached would be off by that much.
        self.base = hand @ np.linalg.inv(frames(self.chain, self.q)[-1])
        if self.done:
            return self.target, self.stroke.jaw
        self.ticks += 1
        if self.ticks > PATIENCE:
            self.stage_name = "next" if self.cut else "done"
        if self.stage_name in ("move", "tip", "home"):
            # A planned move: the target walks to the goal a step at a time,
            # and the stage ends once the arm has arrived on it -- tipped, once
            # the piece has slid out as well.
            if np.abs(self.goal - self.q).max() >= ARRIVED:
                self.target = self.target + np.clip(
                    self.goal - self.target, -JOINT_STEP, JOINT_STEP
                )
                return self.target, self.stroke.jaw
            if self.stage_name == "tip" and self.stroke.fouled(hand):
                return self.target, self.stroke.jaw
            after = {"move": "approach", "tip": "next", "home": "done"}
            self.stage_name, self.ticks = after[self.stage_name], 0
        cut = self.cut
        if cut is None:
            return self.target, self.stroke.jaw
        if self.stage_name in ("approach", "push", "retract"):
            # The straight line, a step of the mouth at a time, solved from
            # where the arm is so each step starts from the last -- and the
            # end of it held until the arm has caught up.
            end = {"approach": cut.pose, "push": cut.pushed, "retract": cut.standoff}
            gap = end[self.stage_name][:3, 3] - self.line
            distance = np.linalg.norm(gap)
            arrived = distance < 1e-6 and np.abs(self.target - self.q).max() < ARRIVED
            # Pushing and closing wait for the arm to settle as well: a blade
            # shut while the mouth is still moving in sweeps past the cane.
            if arrived and (self.stage_name == "retract" or self.moved < STILL):
                after = {"approach": "push" if cut.push.any() else "close", "push": "close"}
                self.stage_name = after.get(self.stage_name, "next")
                self.ticks = 0
                if self.stage_name == "next" and self.stroke.fouled(hand):
                    self.goal, tips = self._solve(cut.tipped)
                    self.stage_name = "tip" if tips else "next"
            else:
                self.line = self.line + gap * min(CLOSE_IN / max(distance, 1e-9), 1.0)
                pose = cut.pose.copy()
                pose[:3, 3] = self.line
                self.target, _, _ = solve(
                    self.chain, self.tool, self._local(pose), self.q, iterations=50
                )
        if self.stage_name == "close":
            # Once the blades have met, the cut is scored by the buds either
            # side of where it was planned.
            if self.stroke.tick(hand, shear, closing=True):
                made = self.stroke.swept > 0
                self.tally.made += made
                self.tally.correct += made and (
                    attached(cut.kept, self.shears, self.stage)
                    and not attached(cut.removed, self.shears, self.stage)
                )
                self.stage_name = "open"
        else:
            opened = self.stroke.tick(hand, shear, closing=False)
            if self.stage_name == "open" and opened:
                self.stage_name = "retract"
            elif self.stage_name == "next":
                self._next()
        return self.target, self.stroke.jaw


def _placed(frame: np.ndarray, corner, u, v) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """The rectangle `corner`, `u`, `v` given in `frame`, in world coordinates."""
    return frame[:3, 3] + frame[:3, :3] @ corner, frame[:3, :3] @ u, frame[:3, :3] @ v


def _swung(pose: np.ndarray, angle: float) -> np.ndarray:
    """`pose`, a position and (x, y, z, w) rotation in one array of 7, turned
    by `angle` about the x axis of the frame it is in."""
    turn = np.array([math.sin(angle / 2), 0.0, 0.0, math.cos(angle / 2)])
    v, w = pose[3:6], pose[6]
    rotation = [*(turn[3] * v + w * turn[:3] + np.cross(turn[:3], v)), turn[3] * w - turn[:3] @ v]
    return np.array([*_rotate(turn, pose[:3]), *rotation])


def _rotate(quat: np.ndarray, v: np.ndarray) -> np.ndarray:
    """`v` turned by the (x, y, z, w) rotation `quat`."""
    x, y, z, w = quat
    u = np.array([x, y, z])
    return v + 2 * np.cross(u, np.cross(u, v) + w * v)
