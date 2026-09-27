"""Choosing the cuts, and making them, from the scene's own ground truth.

What Bumblebee recovers from stereo -- the vine's canes, where each bud sits
on them, which cut leaves the right number -- the generated scene knows
outright. `read_vines` walks the stage for it: every cane is a rod, every
bud a prim under the rod segment that carries it, so a bud's place in the
running simulation is the segment's pose applied to the bud's own.

The rule is the paper's: keep `KEEP_BUDS` on every cane and cut midway
between the last kept bud and the next. Each cut is a pose for the shear's
mouth -- the blades along the approach, the pivot along the cane -- reached
in two stages, a planned move to `STANDOFF` out and a straight line in, and
the cuts on a vine are taken nearest neighbour first. `Pruning` runs that
sequence one vine at a time, and `Tally` keeps the paper's own score: cuts
made, cuts made at the right place, and how long a vine took.
"""

from __future__ import annotations

import dataclasses
from typing import TYPE_CHECKING

import numpy as np

from kinematics import Joint, solve

if TYPE_CHECKING:
    from pxr import Usd

    from vinerylab.isaaclab import Shears

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
meters: a move takes as many ticks as its longest joint swing needs."""

ARRIVED = 0.08
"""How close every joint has to be to its target, in radians or meters, for
the arm to count as having arrived: the arm's weight holds the shoulder a
few hundredths short of a stiff drive's target."""

PATIENCE = 400
"""Control ticks a stage may take before the cut is given up on: eight
seconds at the controller's rate. An arm held off its pose by a post or a
wire would otherwise wait there for the rest of the run."""

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
    """The mouth's frame at the cut, in world coordinates."""
    kept: Bud
    """The last bud to keep, below the cut."""
    removed: Bud
    """The first bud to take, above it."""

    @property
    def standoff(self) -> np.ndarray:
        """The same frame `STANDOFF` back along the blades."""
        pose = self.pose.copy()
        pose[:3, 3] -= STANDOFF * pose[:3, 2]
        return pose


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
        below, above = bud_position(kept, shears), bud_position(removed, shears)
        tangent = above - below
        tangent /= np.linalg.norm(tangent)
        approach = side - (side @ tangent) * tangent
        if np.linalg.norm(approach) < 1e-3:
            continue
        approach /= np.linalg.norm(approach)
        pose = np.eye(4)
        pose[:3, 0] = tangent
        pose[:3, 1] = np.cross(approach, tangent)
        pose[:3, 2] = approach
        pose[:3, 3] = (below + above) / 2
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

    `control` returns the slide and arm positions to hold, and whether the
    shear is to be closed, and moves the sequence on as each stage's target
    is reached: a planned move out to the stand-off, a straight line in, the
    shear closing -- which is when the cut is made -- and opening, and the
    straight line back out. Poses are solved in the robot's own frame, so
    the caller hands in where the robot is.
    """

    def __init__(
        self,
        chain: tuple[Joint, ...],
        tool: np.ndarray,
        mouth: tuple[np.ndarray, np.ndarray, np.ndarray],
        cuts: list[Cut],
        base: np.ndarray,
        q: np.ndarray,
        shears: Shears,
        stage: Usd.Stage,
        tally: Tally,
    ):
        self.chain, self.tool, self.mouth = chain, tool, mouth
        self.shears, self.stage, self.tally = shears, stage, tally
        self.base = base
        self.q = np.array(q, dtype=float)
        self.close = False
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
        """Joint positions for `pose`, from where the arm is or, failing that,
        from each of `SEEDS`, and whether any of them reached it."""
        local = self._local(pose)
        best = solve(self.chain, self.tool, local, self.q)
        for seed in SEEDS:
            if best[1] <= REACH_TOLERANCE[0] and best[2] <= REACH_TOLERANCE[1]:
                break
            best = min(
                best, solve(self.chain, self.tool, local, [self.q[0], *seed]), key=lambda r: r[1]
            )
        return best[0], best[1] <= REACH_TOLERANCE[0] and best[2] <= REACH_TOLERANCE[1]

    def _local(self, pose: np.ndarray) -> np.ndarray:
        """`pose` in the robot's base frame."""
        return np.linalg.inv(self.base) @ pose

    def _next(self) -> None:
        """Start on the next cut, or finish."""
        self.cut = self.cuts.pop(0) if self.cuts else None
        self.ticks = 0
        if self.cut is None:
            self.stage_name = "done"
            return
        self.stage_name = "move"
        self.goal, _ = self._solve(self.cut.standoff)
        self.line = self.cut.standoff[:3, 3].copy()

    @property
    def done(self) -> bool:
        return self.stage_name == "done"

    def control(
        self, q: np.ndarray, hand: np.ndarray, shear: float, opening: float
    ) -> tuple[np.ndarray, bool]:
        """Advance one tick. `q` is where the slide and arm are, `hand` the
        pose of the body the shear is on in world coordinates, `shear` the
        blade's angle and `opening` its open one. Returns the positions to
        hold and whether the shear is to be closed."""
        self.q = np.array(q, dtype=float)
        if self.cut is None:
            return self.target, False
        cut = self.cut
        self.ticks += 1
        if self.ticks > PATIENCE and self.stage_name in ("move", "approach", "retract"):
            self.stage_name = "next"
        if self.stage_name == "move":
            # A planned move: the target walks to the goal a step at a time,
            # and the stage ends once the arm has arrived on it.
            if np.abs(self.goal - self.q).max() < ARRIVED:
                self.stage_name, self.ticks = "approach", 0
            else:
                self.target = self.target + np.clip(
                    self.goal - self.target, -JOINT_STEP, JOINT_STEP
                )
                return self.target, False
        if self.stage_name in ("approach", "retract"):
            # The straight line, a step of the mouth at a time, solved from
            # where the arm is so each step starts from the last -- and the
            # end of it held until the arm has caught up.
            end = cut.pose[:3, 3] if self.stage_name == "approach" else cut.standoff[:3, 3]
            gap = end - self.line
            distance = np.linalg.norm(gap)
            if distance < 1e-6 and np.abs(self.target - self.q).max() < ARRIVED:
                self.stage_name = "close" if self.stage_name == "approach" else "next"
                self.ticks = 0
            else:
                self.line = self.line + gap * min(CLOSE_IN / max(distance, 1e-9), 1.0)
                pose = cut.pose.copy()
                pose[:3, 3] = self.line
                self.target, _, _ = solve(
                    self.chain, self.tool, self._local(pose), self.q, iterations=50
                )
        if self.stage_name == "close":
            self.close = True
            if shear < 0.05:
                # The blades have met: whatever lies across the mouth is cut.
                corner, across, up = (
                    hand[:3, 3] + hand[:3, :3] @ self.mouth[0],
                    hand[:3, :3] @ self.mouth[1],
                    hand[:3, :3] @ self.mouth[2],
                )
                made = self.shears.cut_through(corner, across, up) > 0
                self.tally.made += made
                self.tally.correct += made and (
                    attached(cut.kept, self.shears, self.stage)
                    and not attached(cut.removed, self.shears, self.stage)
                )
                self.stage_name = "open"
        elif self.stage_name == "open":
            self.close = False
            if shear > opening - 0.05:
                self.stage_name = "retract"
        elif self.stage_name == "next":
            self._next()
        return self.target, self.close


def _rotate(quat: np.ndarray, v: np.ndarray) -> np.ndarray:
    """`v` turned by the (x, y, z, w) rotation `quat`."""
    x, y, z, w = quat
    u = np.array([x, y, z])
    return v + 2 * np.cross(u, np.cross(u, v) + w * v)
