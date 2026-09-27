"""Tests for the chain kinematics. Pure numpy: they run without Isaac Lab."""

from __future__ import annotations

import math

import numpy as np
import pytest

import kinematics
from kinematics import Joint

# The UR5's joints as its URDF places them. Duplicated from `bumblebee` so
# this file needs no Isaac Lab; `test_bumblebee` checks the two agree.
UR5 = (
    Joint("shoulder_pan_joint", (0.0, 0.0, 0.089159)),
    Joint("shoulder_lift_joint", (0.0, 0.13585, 0.0), (0.0, math.pi / 2, 0.0), (0.0, 1.0, 0.0)),
    Joint("elbow_joint", (0.0, -0.1197, 0.425), axis=(0.0, 1.0, 0.0)),
    Joint("wrist_1_joint", (0.0, 0.0, 0.39225), (0.0, math.pi / 2, 0.0), (0.0, 1.0, 0.0)),
    Joint("wrist_2_joint", (0.0, 0.093, 0.0)),
    Joint("wrist_3_joint", (0.0, 0.0, 0.09465), axis=(0.0, 1.0, 0.0)),
)
FLANGE = kinematics.transform((0.0, 0.0823, 0.0), (-math.pi / 2, 0.0, 0.0))


def test_the_ur5_at_zero_lies_out_along_x_as_published():
    """The flange of the ur_description model at zero, to the millimetre:
    what checks both the joint table and the chain arithmetic at once."""
    flange = kinematics.frames(UR5, np.zeros(6))[-1] @ FLANGE

    assert flange[:3, 3] == pytest.approx([0.81725, 0.19145, -0.00549], abs=1e-5)
    assert flange[:3, 2] == pytest.approx([0.0, 1.0, 0.0], abs=1e-9), "tool axis out along +Y"


def test_an_origin_round_trips_through_roll_pitch_yaw():
    for rpy in [(0.0, 0.0, 0.0), (0.3, -1.2, 2.5), (-math.pi / 2, 0.0, 0.0), (1.0, 1.4, -2.0)]:
        frame = kinematics.transform((1.0, 2.0, 3.0), rpy)
        back = kinematics.transform((1.0, 2.0, 3.0), kinematics.rpy_of(frame[:3, :3]))
        assert back == pytest.approx(frame, abs=1e-9), rpy


def test_along_turns_z_onto_any_direction():
    for direction in [
        (1.0, 0.0, 0.0),
        (0.0, -1.0, 0.0),
        (0.0, 0.0, 1.0),
        (0.0, 0.0, -1.0),
        (1.0, 2.0, 3.0),
    ]:
        unit = np.asarray(direction) / np.linalg.norm(direction)
        assert kinematics.along(direction) @ [0.0, 0.0, 1.0] == pytest.approx(unit, abs=1e-9)


def test_the_solver_reaches_a_pose_and_respects_the_limits():
    """A pose inside the workspace is reached to a fraction of a millimetre
    and a milliradian; one outside is approached as far as the limits allow,
    and the limits hold."""
    chain = (
        Joint("slide", (0.0, 0.0, 0.5), axis=(1.0, 0.0, 0.0), prismatic=True, limits=(-0.5, 0.5)),
        *UR5,
    )
    home = np.array([0.0, 0.0, -2.0, 2.2, -1.8, -1.57, 0.0])
    target = np.eye(4)
    target[:3, :3] = kinematics.rotation((0.0, 1.0, 0.0), -math.pi / 2) @ kinematics.rotation(
        (1.0, 0.0, 0.0), 0.3
    )
    target[:3, 3] = [0.4, 0.5, 1.0]

    q, meters, radians = kinematics.solve(chain, FLANGE, target, home)

    assert meters < 1e-4 and radians < 1e-3, (meters, radians)
    assert (kinematics.frames(chain, q)[-1] @ FLANGE)[:3, 3] == pytest.approx(
        target[:3, 3], abs=1e-4
    )
    assert abs(q[0]) <= 0.5, "the slide stays on its rail"

    beyond = target.copy()
    beyond[:3, 3] = [3.0, 0.0, 1.0]
    q, meters, _ = kinematics.solve(chain, FLANGE, beyond, home)
    assert meters > 1.0, "three metres out is out of reach"
    assert -0.5 <= q[0] <= 0.5 and (np.abs(q[1:]) <= 2 * math.pi).all()
