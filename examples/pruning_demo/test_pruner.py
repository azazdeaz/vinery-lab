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
    """Rods standing still where `poses` puts their bodies, each cane on one
    body, crossing a rectangle at its planned cut point if anywhere. A cane
    is as thick as the open mouth is wide, so it lies against the fixed blade
    wherever the mouth stands, with nothing to carry it there. Cutting a cane
    takes the buds above the cut."""

    def __init__(self, poses: dict[int, np.ndarray], canes: list[Cane]):
        self.poses = poses
        self.bodies = np.array(sorted(poses), dtype=int)
        self.loose = np.zeros(len(self.bodies), dtype=bool)
        self.radius = np.full(len(self.bodies), 0.025)
        self.labels = [f"/rod/{body}" for body in range(max(poses) + 1)] + ["/robot/blade_link"]
        self.canes = canes
        self.removed: set[str] = set()

    def pose(self, body: int) -> np.ndarray:
        return self.poses[body]

    def crossing(self, corner, u, v):
        hits, on = [], []
        for cane in self.canes:
            if cane.buds[KEEP_BUDS].prim in self.removed:
                continue
            point = (
                pruner.bud_position(cane.buds[KEEP_BUDS - 1], self)
                + pruner.bud_position(cane.buds[KEEP_BUDS], self)
            ) / 2
            if (place := on_rectangle(point, corner, u, v)) is not None:
                hits.append(np.searchsorted(self.bodies, cane.buds[0].body))
                on.append(place)
        return np.array(hits, dtype=int), np.full(len(hits), 0.5), np.array(on).reshape(-1, 2)

    def cut(self, body: int, at: float) -> bool:
        cane = next(cane for cane in self.canes if cane.buds[0].body == body)
        self.removed |= {bud.prim for bud in cane.buds[KEEP_BUDS:]}
        return True

    def collider(self, body: int):
        return 0, IDENTITY.copy()

    def place(self, shape: int, pose: np.ndarray) -> None:
        pass


class HeldCane:
    """Canes crossing the moving plate's plane at `points`, which nothing
    moves, each until it is cut; and the moving blade's collider wherever
    `Stroke` last put it."""

    labels = ["/robot/blade_link"]
    bodies = np.array([3, 4])
    radius = np.array([0.005, 0.005])
    REST = np.array([0.006, -0.01, 0.04, 0.0, 0.0, 0.0, 1.0])

    def __init__(self, *points: np.ndarray):
        self.points, self.cuts, self.placed = points, [], []

    def crossing(self, corner, u, v):
        cut = {body for body, _ in self.cuts}
        on = [
            None if body in cut else on_rectangle(point, corner, u, v)
            for body, point in zip(self.bodies, self.points, strict=False)
        ]
        hits = [i for i, place in enumerate(on) if place is not None]
        places = np.array([on[i] for i in hits]).reshape(-1, 2)
        return np.array(hits, dtype=int), np.full(len(hits), 0.5), places

    def cut(self, body: int, at: float) -> bool:
        self.cuts.append((body, at))
        return True

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


@pytest.mark.parametrize(
    ("z", "other"),
    [
        (0.06, None),
        (0.01, None),
        (0.06, (-0.03, 0.008)),  # in the open blade already, at its root
        (0.06, (-0.02, 0.07)),  # off the fixed blade, where the edge passes it first
    ],
)
def test_the_collider_stands_at_a_held_cane_while_the_blade_closes_through_it(z, other):
    """A cane against the fixed blade, `z` out along it: the collider follows
    the blade until the moving edge reaches the cane -- or stays put, if it
    is there already when the stroke starts -- and stands there while the
    blade closes on. The held cane is cut as the edge reaches its axis, not a
    second at `other`: one the open blade is in already, or one the edge
    passes first, off the fixed blade. Opening, the collider never turns back
    shut, rides the blade again once that is past it, and the stroke is ready
    for the next."""
    bumblebee = pytest.importorskip("bumblebee", reason="Isaac Lab is not installed")
    machine = bumblebee.Bumblebee()
    shear = machine.shear
    y = -float(HeldCane.radius[0])
    hand = kinematics.frames(machine.chain, np.array([0.0, *bumblebee.ARM_HOME]))[-1]

    def placed(y: float, z: float) -> np.ndarray:
        """The point (y, z) of the moving plate's plane, in world coordinates."""
        return (hand @ bumblebee.TOOL @ [shear.bypass, y, z, 1.0])[:3]

    points = [(y, z), *([other] if other else [])]
    cane = HeldCane(*(placed(*point) for point in points))
    stroke = pruner.Stroke(machine, cane)
    pinch = min(shear.pinch(y, z, HeldCane.radius[0] + pruner.GRIP), shear.opening)

    def stood(closing: bool) -> list[float]:
        """The collider's angle once the blade has followed its target, tick
        by tick until the stroke reports the blade there; and the blade's
        angle at the tick the cut was made, in `cut_at`."""
        nonlocal angle
        angles = []
        while True:
            there = stroke.tick(hand, angle, closing)
            if stroke.cut and not cut_at:
                cut_at.append(angle)
            if there:
                return angles
            angle = stroke.jaw
            angles.append(angle + stroke.turned)

    angle, cut_at = shear.opening, []
    shut = stood(closing=True)
    assert cane.cuts == [(HeldCane.bodies[0], 0.5)] and stroke.held == pytest.approx(pinch)
    axis = shear.pinch(y, z, 0.0)
    assert cut_at[0] <= axis < cut_at[0] + BLADE_STEP, "cut as the edge reaches its axis"
    assert shut[0] == pytest.approx(shear.opening - BLADE_STEP), "follows the blade at first"
    assert min(shut[1:]) == pytest.approx(pinch) and shut[-1] == pytest.approx(pinch)
    assert (np.diff(shut[1:]) <= 1e-9).all(), "shutting, it never turns back open"
    opened = stood(closing=False)
    assert opened[0] == pytest.approx(pinch) and (np.diff(opened) >= -1e-9).all()
    assert stroke.held is None and stroke.turned == 0.0
    assert not stroke.cut and stroke.inside is None, "ready for the next stroke"
    assert cane.placed[-1] == pytest.approx(HeldCane.REST), "back on the blade"
    turned = max(cane.placed, key=lambda pose: pose[3])
    assert turned[3] > 0 and np.linalg.norm(turned[:3]) == pytest.approx(
        np.linalg.norm(HeldCane.REST[:3])
    ), "swung about the pivot"


@pytest.mark.parametrize(
    ("push", "gone", "fouled"),
    [
        ((0.0, 0.0, 0.0), False, 0),
        ((0.0, -0.01, 0.0), False, 0),
        ((0.0, 0.0, 0.01), False, 0),
        ((0.0, 0.0, 0.0), True, 0),
        ((0.0, 0.0, 0.0), False, 20),
    ],
)
def test_the_sequence_stands_off_closes_in_and_cuts_every_cane(monkeypatch, push, gone, fouled):
    """With an arm that goes where it is told and a blade that turns as told,
    every reachable cane is cut at its planned point, the shear closing with
    the mouth moved on from it by the cut's push, and the tally says so -- but
    a cane whose bud to take is `gone` before its turn is left alone. A piece
    cut free that lies across the mouth for the first `fouled` looks is
    tipped out, the blades turned down until it is gone; and once the vine is
    pruned the arm folds home."""
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
    pieces = iter([True] * fouled)
    monkeypatch.setattr(pruner.Stroke, "fouled", lambda self, hand: next(pieces, False))
    cuts = pruner.plan(Vine("/vine", np.zeros(3), canes), shears, np.array([0.0, 1.0, 0.0]))
    for cut in cuts:
        cut.push = np.array(push)
    if gone:
        shears.removed.add(cuts[1].removed.prim)
    tally = Tally()
    q = np.array([0.0, *bumblebee.ARM_HOME])
    pruning = pruner.Pruning(machine, cuts, np.eye(4), q, shears, None, tally)
    assert tally.reachable == 2, "both are within reach"

    angle, ticks, stage, closed, down = machine.shear.opening, 0, None, 0, []
    while not pruning.done and ticks < 3000:
        hand = kinematics.frames(machine.chain, q)[-1]
        q, angle = pruning.control(q, hand, angle)
        mouth = kinematics.frames(machine.chain, q)[-1] @ machine.tool
        if pruning.stage_name == "close" and stage != "close":
            assert mouth[:3, 3] == pytest.approx(pruning.cut.pushed[:3, 3], abs=0.002)
            closed += 1
        if pruning.stage_name == "tip":
            down.append(-mouth[2, 2])
        stage = pruning.stage_name
        ticks += 1

    made = 1 if gone else 2
    assert pruning.done and ticks < 3000 and closed == made
    assert (max(down) > np.sin(pruner.TIP) - 0.05) if fouled else not down
    assert q == pytest.approx(pruning.home, abs=pruner.ARRIVED)
    assert (tally.planned, tally.reachable, tally.made, tally.correct) == (2, 2, made, made)
