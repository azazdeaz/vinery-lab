"""Tests for the planner and the cutting sequence.

The rule and the sequence run on a stand-in for `Shears` and an arm that goes
where it is told, so they need no simulation; reading the ground truth needs
a stage, so that part wants `pxr`, and the full sequence wants the robot,
which reaches Isaac Lab.
"""

from __future__ import annotations

import numpy as np
import pytest

import kinematics
import pruner
from pruner import BLADE_STEP, KEEP_BUDS, Bud, Cane, Cut, Tally, Vine

IDENTITY = np.array([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])

NO_CROSSING = (np.zeros(0, dtype=int), np.zeros(0), np.zeros((0, 2)))


def on_rectangle(point, corner, u, v) -> tuple[float, float] | None:
    """Where `point` is on the rectangle, as (a, b), if it is on it."""
    offset = point - corner
    normal = np.cross(u, v)
    a, b = offset @ u / (u @ u), offset @ v / (v @ v)
    if abs(offset @ normal) < 0.01 * np.linalg.norm(normal) and 0 <= a <= 1 and 0 <= b <= 1:
        return a, b
    return None


class FakeShears:
    """Rods standing still where `poses` puts their bodies. `cut_through`
    counts a cane as cut when the rectangle passes through its planned cut
    point, and takes the buds above it; no cane is ever held, so the blade's
    collider follows the blade."""

    def __init__(self, poses: dict[int, np.ndarray], canes: list[Cane]):
        self.poses = poses
        self.bodies = np.array(sorted(poses), dtype=int)
        self.loose = np.zeros(len(self.bodies), dtype=bool)
        self.labels = [f"/rod/{body}" for body in range(max(poses) + 1)] + ["/robot/blade_link"]
        self.canes = canes
        self.removed: set[str] = set()

    def pose(self, body: int) -> np.ndarray:
        return self.poses[body]

    def cut_through(self, corner, u, v) -> int:
        cuts = 0
        for cane in self.canes:
            if cane.buds[KEEP_BUDS].prim in self.removed:
                continue
            point = (
                pruner.bud_position(cane.buds[KEEP_BUDS - 1], self)
                + pruner.bud_position(cane.buds[KEEP_BUDS], self)
            ) / 2
            if on_rectangle(point, corner, u, v) is not None:
                self.removed |= {bud.prim for bud in cane.buds[KEEP_BUDS:]}
                cuts += 1
        return cuts

    def crossing(self, corner, u, v):
        return NO_CROSSING

    def collider(self, body: int):
        return 0, IDENTITY.copy()

    def place(self, shape: int, pose: np.ndarray) -> None:
        pass


class HeldCane:
    """One cane lying across the mouth, which nothing moves, and the moving
    blade's collider wherever `Stroke` last put it."""

    labels = ["/robot/blade_link"]
    radius = np.array([0.005])
    REST = np.array([0.006, -0.01, 0.04, 0.0, 0.0, 0.0, 1.0])

    def __init__(self, point: np.ndarray):
        self.point, self.cut, self.placed = point, False, []

    def crossing(self, corner, u, v):
        on = None if self.cut else on_rectangle(self.point, corner, u, v)
        return NO_CROSSING if on is None else (np.array([0]), np.array([0.5]), np.array([on]))

    def cut_through(self, corner, u, v) -> int:
        if self.cut or on_rectangle(self.point, corner, u, v) is None:
            return 0
        self.cut = True
        return 1

    def collider(self, body: int):
        return 7, self.REST.copy()

    def place(self, shape: int, pose: np.ndarray) -> None:
        assert shape == 7
        self.placed.append(pose)


def cane(prim: str, body: int, stations: list[float]) -> Cane:
    """A cane standing on one body, its buds `stations` up the body's Z."""
    return Cane(
        prim,
        [Bud(f"{prim}/Bud_{i:02}", body, np.array([0.0, 0.0, s])) for i, s in enumerate(stations)],
    )


def test_a_cut_is_planned_midway_between_the_last_kept_bud_and_the_next():
    """Squared up to the cane: pivot along it, blades along the approach."""
    tall = cane("/tall", 7, [0.1, 0.22, 0.34, 0.46])
    short = cane("/short", 8, [0.1, 0.22])
    shears = FakeShears({7: np.array([1.0, 2.0, 0.0, *IDENTITY[3:]]), 8: IDENTITY.copy()}, [tall])

    cuts = pruner.plan(Vine("/vine", np.zeros(3), [tall, short]), shears, np.array([0.0, 1.0, 0.0]))

    assert [cut.cane.prim for cut in cuts] == ["/tall"], "two buds is nothing to cut"
    pose = cuts[0].pose
    assert pose[:3, 3] == pytest.approx([1.0, 2.0, 0.28])
    assert pose[:3, 0] == pytest.approx([0.0, 0.0, 1.0]), "the pivot lies along the cane"
    assert pose[:3, 2] == pytest.approx([0.0, 1.0, 0.0]), "the blades point along the approach"
    assert pose[:3, :3] @ pose[:3, :3].T == pytest.approx(np.eye(3))
    assert cuts[0].standoff[:3, 3] == pytest.approx([1.0, 2.0 - pruner.STANDOFF, 0.28])
    assert (cuts[0].kept.prim, cuts[0].removed.prim) == ("/tall/Bud_01", "/tall/Bud_02")


def test_cuts_are_taken_nearest_neighbour_first():
    def at(x: float) -> Cut:
        pose = np.eye(4)
        pose[0, 3] = x
        return Cut(cane(f"/{x}", 0, [0.0, 0.1, 0.2]), pose, None, None)

    ordered = pruner.nearest_first([at(0.0), at(5.0), at(1.0)], np.array([0.9, 0.0, 0.0]))

    assert [cut.pose[0, 3] for cut in ordered] == [1.0, 0.0, 5.0]


def test_the_tally_reads_as_the_papers_score():
    tally = Tally(vines=2, planned=10, reachable=8, made=7, correct=6, seconds=300.0)
    assert tally.summary() == (
        "2 vines, 10 cuts planned: 80% reachable, 70% made, 60% made at the right place, 150 s per vine"
    )
    assert "n/a" in Tally().summary()


def test_the_ground_truth_is_read_off_the_stage():
    """A vine's canes are the rods under it, a cane's buds the prims under its
    segments, in order up the cane and placed in each segment's own frame."""
    pytest.importorskip("pxr.Usd")
    from pxr import Gf, Usd, UsdGeom

    stage = Usd.Stage.CreateInMemory()
    vine = "/World/V/Planting/Row_000/Vine_000"
    UsdGeom.Xform.Define(stage, vine).AddTranslateOp().Set(Gf.Vec3d(3.0, 4.0, 0.5))
    for path, offset in [
        (f"{vine}/Shoot_00_0/Cable_edge_body_1/Bud_00", 0.03),
        (f"{vine}/Shoot_00_0/Cable_edge_body_1/Bud_01", 0.07),
        (f"{vine}/Shoot_00_0/Cable_edge_body_2/Bud_02", 0.02),
        (f"{vine}/Shoot_01_0/Stem/Bud_00", 0.05),  # a static cane, with no rod body
    ]:
        UsdGeom.Xform.Define(stage, path).AddTranslateOp().Set(Gf.Vec3d(0.0, 0.0, offset))
    labels = [
        "/elsewhere",
        f"{vine}/Shoot_00_0/Cable_edge_body_2",
        f"{vine}/Shoot_00_0/Cable_edge_body_1",
    ]
    shears = FakeShears({1: IDENTITY.copy(), 2: IDENTITY.copy()}, [])
    shears.labels = labels

    [read] = pruner.read_vines(stage, "/World/V", 0, shears)

    assert read.prim == vine and read.position == pytest.approx([3.0, 4.0, 0.5])
    [rod] = read.canes
    assert [(bud.prim.rsplit("/", 1)[1], bud.body, bud.offset[2]) for bud in rod.buds] == [
        ("Bud_00", 2, 0.03),
        ("Bud_01", 2, 0.07),
        ("Bud_02", 1, 0.02),
    ]
    # Still on the vine, until its body is cut free or it is hidden past a cut.
    assert pruner.attached(rod.buds[0], shears, stage)
    UsdGeom.Imageable(stage.GetPrimAtPath(rod.buds[0].prim)).MakeInvisible()
    assert not pruner.attached(rod.buds[0], shears, stage)
    shears.loose[np.searchsorted(shears.bodies, 1)] = True
    assert not pruner.attached(rod.buds[2], shears, stage)


@pytest.mark.parametrize("z", [0.06, 0.01])
def test_the_collider_stands_at_a_held_cane_while_the_blade_closes_through_it(z):
    """A cane against the fixed blade, `z` out along it: the collider follows
    the blade until the moving edge reaches the cane -- or stays put, if it
    is there already when the stroke starts -- stands there while the blade
    closes on, and the cut is made when the edge reaches the cane's axis.
    Opening, it never turns back shut, and rides the blade again once that
    is past it."""
    bumblebee = pytest.importorskip("bumblebee", reason="Isaac Lab is not installed")
    machine = bumblebee.Bumblebee()
    shear = machine.shear
    y = -float(HeldCane.radius[0])
    hand = kinematics.frames(machine.chain, np.array([0.0, *bumblebee.ARM_HOME]))[-1]
    cane = HeldCane((hand @ bumblebee.TOOL @ [shear.bypass, y, z, 1.0])[:3])
    stroke = pruner.Stroke(machine, cane)
    pinch = min(shear.pinch(y, z, HeldCane.radius[0] + pruner.GRIP), shear.opening)

    def stood(closing: bool) -> list[float]:
        """The collider's angle once the blade has followed its target, tick
        by tick until the stroke reports the blade there."""
        nonlocal angle
        angles = []
        while not stroke.tick(hand, angle, closing):
            angle = stroke.jaw
            angles.append(angle + stroke.turned)
        return angles

    angle = shear.opening
    shut = stood(closing=True)
    assert stroke.swept == 1 and stroke.held == pytest.approx(pinch)
    assert shut[0] == pytest.approx(shear.opening - BLADE_STEP), "follows the blade at first"
    assert min(shut[1:]) == pytest.approx(pinch) and shut[-1] == pytest.approx(pinch)
    assert (np.diff(shut[1:]) <= 1e-9).all(), "shutting, it never turns back open"
    opened = stood(closing=False)
    assert opened[0] == pytest.approx(pinch) and (np.diff(opened) >= -1e-9).all()
    assert stroke.held is None and stroke.turned == 0.0
    assert cane.placed[-1] == pytest.approx(HeldCane.REST), "back on the blade"
    turned = max(cane.placed, key=lambda pose: pose[3])
    assert turned[3] > 0 and np.linalg.norm(turned[:3]) == pytest.approx(
        np.linalg.norm(HeldCane.REST[:3])
    ), "swung about the pivot"


@pytest.mark.parametrize(
    ("push", "gone"),
    [
        ((0.0, 0.0, 0.0), False),
        ((0.0, -0.01, 0.0), False),
        ((0.0, 0.0, 0.01), False),
        ((0.0, 0.0, 0.0), True),
    ],
)
def test_the_sequence_stands_off_closes_in_and_cuts_every_cane(monkeypatch, push, gone):
    """With an arm that goes where it is told and a blade that turns as told,
    every reachable cane is cut at its planned point, the shear closing with
    the mouth moved on from it by the cut's push, and the tally says so -- but
    a cane whose bud to take is `gone` before its turn is left alone."""
    bumblebee = pytest.importorskip("bumblebee", reason="Isaac Lab is not installed")
    machine = bumblebee.Bumblebee()
    # Two canes standing beside the robot, out over its rail side.
    canes = [cane("/a", 1, [0.0, 0.12, 0.24, 0.36]), cane("/b", 2, [0.0, 0.12, 0.24, 0.36])]
    shears = FakeShears(
        {
            1: np.array([0.2, 1.3, 1.0, *IDENTITY[3:]]),
            2: np.array([-0.3, 1.25, 1.05, *IDENTITY[3:]]),
        },
        canes,
    )
    monkeypatch.setattr(
        pruner, "attached", lambda bud, shears, stage: bud.prim not in shears.removed
    )
    cuts = pruner.plan(Vine("/vine", np.zeros(3), canes), shears, np.array([0.0, 1.0, 0.0]))
    for cut in cuts:
        cut.push = np.array(push)
    if gone:
        shears.removed.add(cuts[1].removed.prim)
    tally = Tally()
    q = np.array([0.0, *bumblebee.ARM_HOME])
    pruning = pruner.Pruning(machine, cuts, np.eye(4), q, shears, None, tally)
    assert tally.reachable == 2, "both are within reach"

    angle, ticks, stage, closed = machine.shear.opening, 0, None, 0
    while not pruning.done and ticks < 3000:
        hand = kinematics.frames(machine.chain, q)[-1]
        q, angle = pruning.control(q, hand, angle)
        if pruning.stage_name == "close" and stage != "close":
            mouth = kinematics.frames(machine.chain, q)[-1] @ machine.tool
            assert mouth[:3, 3] == pytest.approx(pruning.cut.pushed[:3, 3], abs=0.002)
            closed += 1
        stage = pruning.stage_name
        ticks += 1

    made = 1 if gone else 2
    assert pruning.done and ticks < 3000 and closed == made
    assert (tally.planned, tally.reachable, tally.made, tally.correct) == (2, 2, made, made)
