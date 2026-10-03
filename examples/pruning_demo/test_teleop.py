"""Tests for the keyboard control: keys held by hand, on a stand-in driver
and shears and an arm that goes where it is told."""

from __future__ import annotations

import numpy as np
import pytest

bumblebee = pytest.importorskip("bumblebee", reason="Isaac Lab is not installed")

import kinematics  # noqa: E402
import teleop  # noqa: E402
from driver import CRUISE_SPEED, MAX_YAW_RATE  # noqa: E402
from pruner import BLADE_STEP  # noqa: E402

MACHINE = bumblebee.Bumblebee()


class FakeDriver:
    def __init__(self):
        self.driven: list[tuple[float, float]] = []

    def drive(self, speed: float, yaw_rate: float) -> None:
        self.driven.append((speed, yaw_rate))


class FakeShears:
    """Every sweep of the blade cuts one cane; none is ever held."""

    labels = ["/robot/blade_link"]

    def __init__(self):
        self.sweeps = 0

    def cut_through(self, corner, u, v) -> int:
        self.sweeps += 1
        return 1

    def crossing(self, corner, u, v):
        return np.zeros(0, dtype=int), np.zeros(0), np.zeros((0, 2))

    def collider(self, body: int):
        return 0, np.array([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])

    def place(self, shape: int, pose: np.ndarray) -> None:
        pass


def mouth(q: np.ndarray) -> np.ndarray:
    return kinematics.frames(MACHINE.chain, q)[-1] @ MACHINE.tool


def test_the_keys_drive_the_base_jog_the_mouth_and_work_the_shear():
    driver, shears = FakeDriver(), FakeShears()
    tele = teleop.Teleop(MACHINE, driver, shears)
    q, angle = np.array([0.0, *bumblebee.ARM_HOME]), MACHINE.shear.opening

    joints: list[np.ndarray] = []

    def hold(*keys: str, ticks: int = 1) -> None:
        """The arm and the blade follow their targets within the tick."""
        nonlocal q, angle
        tele.held = set(keys)
        for _ in range(ticks):
            q, angle = tele.control(q, kinematics.frames(MACHINE.chain, q)[-1], angle)
            joints.append(kinematics.frames(MACHINE.chain, q)[1:, :3, 3])

    hold(ticks=200)
    ready = tele.mouth
    assert mouth(q)[:3, 3] == pytest.approx(ready, abs=0.01), "reaches out to the ready point"
    assert MACHINE.rail[1] + 0.2 < ready[1] < 1.2 and 1.0 < ready[2] < 1.4, (
        "over the rail, spur high"
    )
    swept = np.concatenate(joints)
    assert swept[:, 1].max() < ready[1] + 0.01 and swept[:, 2].min() > MACHINE.deck, (
        "the arm unfolds over the deck, no further out than the ready point"
    )
    assert driver.driven[-1] == (0.0, 0.0)

    hold("UP", "PAGE_UP", ticks=10)
    moved = mouth(q)
    assert moved[:3, 3] - ready == pytest.approx(
        10 * teleop.JOG_STEP * np.array([1.0, 0.0, 1.0]) / np.sqrt(2), abs=0.01
    )
    assert moved[:3, 0] == pytest.approx(teleop.UP, abs=0.02), "the pivot stays upright"
    assert moved[:3, 2] == pytest.approx(teleop.OUT, abs=0.02), "the blades point at the row"

    hold("LEFT", ticks=400)
    assert tele.mouth[1] < 2.0 and mouth(q)[:3, 3] == pytest.approx(tele.mouth, abs=0.02), (
        "a jog out of reach is refused"
    )

    hold("W", "D")
    assert driver.driven[-1] == (CRUISE_SPEED, -MAX_YAW_RATE)

    shut = round(MACHINE.shear.opening / BLADE_STEP)
    hold("ENTER")
    mouth_at_cut = tele.mouth
    hold("ENTER", "W", "UP", ticks=4)
    assert driver.driven[-1] == (0.0, 0.0) and tele.mouth is mouth_at_cut, "still while it shuts"
    hold("ENTER", ticks=shut - 4)
    assert shears.sweeps == shut and tele.stroke.jaw < MACHINE.shear.opening, "shut, and opening"
    hold(ticks=shut + 10)
    assert tele.stroke.jaw == MACHINE.shear.opening and shears.sweeps == shut
