"""Driving the skid steer down the alley, one stop at a time.

The base holds the row heading and drives forward to each stop, leaning its
heading over to take out any cross-track error. There is no steering joint:
a turn is the two sides of the machine driven at different speeds, which
scrubs the wheels round, as it does on the real thing.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np
import torch

from isaaclab.assets import Articulation
from isaaclab.utils.math import wrap_to_pi, yaw_quat

from bumblebee import LEFT, RIGHT, Bumblebee

if TYPE_CHECKING:  # `vinerylab.usd` pulls in `pxr`, which has to wait for Kit
    from vinerylab.usd import Ground

# 200 Hz physics under a 50 Hz controller.
SIM_DT = 0.005
DECIMATION = 4

CRUISE_SPEED = 0.6  # m/s along the alley
SPEED_GAIN = 1.5  # m/s of speed per m still to drive, up to the cruise
ALIGN_GAIN = 1.0  # rad of heading per m off the alley's line
YAW_GAIN = 2.0  # rad/s of turn per rad of heading error
MAX_YAW_RATE = 0.5
STOP_TOLERANCE = 0.05  # m along the alley a stop is held to
STANDING = 0.02  # m/s below which the machine counts as stopped
SPAWN_CLEARANCE = 0.05  # m the lowest wheel is held off the ground on a spawn


class Driver:
    """Drives `robot` along `heading` to whichever stop it was last given."""

    def __init__(self, robot: Articulation, machine: Bumblebee, heading: float, ground: Ground):
        self.robot, self.machine, self.ground = robot, machine, ground
        self.heading = heading
        self.forward = np.array([np.cos(heading), np.sin(heading)])
        self.left_of = np.array([-self.forward[1], self.forward[0]])
        # Addressed by name: the backends import an articulation's joints in
        # their own order.
        self.left, _ = robot.find_joints(LEFT, preserve_order=True)
        self.right, _ = robot.find_joints(RIGHT, preserve_order=True)
        self.stop: np.ndarray | None = None

    def place(self, xy: np.ndarray) -> None:
        """Stand the robot at `xy`, facing along the alley, on the ground."""
        device = self.robot.device
        pose = torch.tensor(
            [
                [
                    *xy,
                    self._spawn_height(xy),
                    0.0,
                    0.0,
                    np.sin(self.heading / 2),
                    np.cos(self.heading / 2),
                ]
            ],
            dtype=torch.float32,
            device=device,
        )
        self.robot.write_root_pose_to_sim_index(root_pose=pose)
        self.robot.write_root_velocity_to_sim_index(root_velocity=torch.zeros(1, 6, device=device))
        self.robot.write_joint_position_to_sim_index(
            position=self.robot.data.default_joint_pos.torch.clone()
        )
        self.robot.write_joint_velocity_to_sim_index(
            velocity=self.robot.data.default_joint_vel.torch.clone()
        )
        self.robot.reset()

    def _spawn_height(self, xy: np.ndarray) -> float:
        """Where the base goes so no wheel starts inside the terrain: the
        highest ground under any of the four decides, since the machine goes
        down level."""
        cos, sin = self.forward
        corners = np.asarray(self.machine.corners)
        wheels = xy + np.column_stack(
            [cos * corners[:, 0] - sin * corners[:, 1], sin * corners[:, 0] + cos * corners[:, 1]]
        )
        return float(max(self.ground.height(x, y) for x, y in wheels)) + SPAWN_CLEARANCE

    @torch.no_grad()
    def control(self) -> bool:
        """Drive one tick toward the stop. Returns whether the robot is
        standing still on it; with no stop set, it stands still where it is."""
        here = self.robot.data.root_pos_w.torch[0, :2].cpu().numpy()
        moving = float(torch.linalg.norm(self.robot.data.root_lin_vel_w.torch[0, :2]))
        speed = yaw_rate = 0.0
        parked = self.stop is None
        if self.stop is not None:
            offset = self.stop - here
            along, across = float(offset @ self.forward), float(offset @ self.left_of)
            parked = abs(along) < STOP_TOLERANCE and moving < STANDING
            if not parked:
                speed = float(np.clip(SPEED_GAIN * along, -CRUISE_SPEED, CRUISE_SPEED))
                # Lean the heading toward the line, more the further off it,
                # and less when driving backwards onto an overshot stop.
                wanted = self.heading + np.sign(speed) * np.clip(ALIGN_GAIN * across, -0.4, 0.4)
                error = wrap_to_pi(torch.tensor(wanted - _yaw(self.robot.data.root_quat_w.torch)))
                yaw_rate = float(np.clip(YAW_GAIN * error.item(), -MAX_YAW_RATE, MAX_YAW_RATE))
        self._drive(speed, yaw_rate)
        return parked

    def _drive(self, speed: float, yaw_rate: float) -> None:
        """Wheel speeds for a body speed and a turn rate: the outer side of a
        turn runs faster by the track times the rate."""
        m = self.machine
        half = yaw_rate * m.track / 2
        for joints, wheel in ((self.left, speed - half), (self.right, speed + half)):
            target = torch.full(
                (1, len(joints)),
                float(np.clip(wheel / m.wheel_radius, -m.max_wheel_speed, m.max_wheel_speed)),
                device=self.robot.device,
            )
            self.robot.set_joint_velocity_target_index(target=target, joint_ids=joints)


def _yaw(quat: torch.Tensor) -> float:
    """The heading of a (1, 4) orientation quaternion: the direction the
    robot faces across the ground, whatever a slope tilts it by."""
    flat = yaw_quat(quat)[0]
    return float(2.0 * torch.atan2(flat[2], flat[3]))
