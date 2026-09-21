"""Driving the gantry along a route.

Two steps, both in `Driver.control`: a waypoint follower turns the route into
a body twist, and swerve kinematics turn that twist into a steering angle and
a wheel speed per module.

The robot never turns around. It holds the row heading for the whole run, so
it drives alternate rows in reverse and crabs sideways between them -- the
legs then stay in the alleys they were put in, which a gantry straddling a row
has no alternative to.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np
import torch

from isaaclab.assets import Articulation
from isaaclab.utils.math import wrap_to_pi, yaw_quat

from straddler import DRIVE_JOINTS, STEER_JOINTS, Straddler
from route import STRIDE

if TYPE_CHECKING:  # `vinerylab.usd` pulls in `pxr`; see `route.row_route` for why that waits
    from vinerylab.usd import Ground

# 200 Hz physics under a 50 Hz controller. The gantry is stiff and slow; the
# rate matters only to the steering joints, which have to swing 90 degrees at
# a headland without the wheels scrubbing their way round.
SIM_DT = 0.005
DECIMATION = 4

CRUISE_SPEED = 1.2  # m/s commanded towards the target
LOOKAHEAD = 1.5 * STRIDE  # see `_advance` for why this is longer than a stride
YAW_GAIN = 2.0  # rad/s of yaw command per rad of heading error
MAX_YAW_RATE = 0.4  # rad/s
SPAWN_CLEARANCE = 0.05  # m the lowest wheel is held off the ground on a (re)spawn
CREEPING = 1e-3  # m/s below which a wheel is not asked to point anywhere


class Driver:
    """Drives `robot` along `route`, holding `heading`, one waypoint at a time."""

    def __init__(
        self,
        route: np.ndarray,
        heading: float,
        machine: Straddler,
        robot: Articulation,
        ground: Ground,
    ):
        self.route = route
        self.heading = heading
        self.machine = machine
        self.robot = robot
        self.ground = ground
        self.waypoints = torch.tensor(route, dtype=torch.float32, device=robot.device)
        # Where each wheel sits in the base frame, for the kinematics below.
        self.corners = torch.tensor(machine.corners, dtype=torch.float32, device=robot.device)
        # Addressed by name: the backends import an articulation's joints in
        # their own order, and both sets have to line up with `corners`.
        self.steer, _ = robot.find_joints(STEER_JOINTS, preserve_order=True)
        self.drive, _ = robot.find_joints(DRIVE_JOINTS, preserve_order=True)
        self.index = 0
        self.command = torch.zeros(3, device=robot.device)

    @property
    def target(self) -> torch.Tensor:
        """The waypoint currently being chased. Shape is (1, 3)."""
        return self.waypoints[self.index].unsqueeze(0)

    @torch.no_grad()  # warp rejects tensors that carry a grad
    def control(self):
        """Advance the route and drive one step. Call every `DECIMATION` sim steps."""
        self._advance()
        self.command = self._steer()
        angle, speed = swerve(
            self.command,
            self.corners,
            self.robot.data.joint_pos.torch[0, self.steer],
            self.machine,
        )
        self.robot.set_joint_position_target_index(target=angle.unsqueeze(0), joint_ids=self.steer)
        self.robot.set_joint_velocity_target_index(target=speed.unsqueeze(0), joint_ids=self.drive)

    def place(self):
        """Stand the robot over the current waypoint, facing along the rows."""
        here = self.route[self.index]
        device = self.robot.device
        pose = torch.tensor(
            # Position, then orientation as a quaternion in (x, y, z, w).
            [
                [
                    *here[:2],
                    spawn_height(self.ground, here[:2], self.heading, self.machine),
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

    def _advance(self):
        """Move the target on once the robot is nearly on it.

        One waypoint per call, and `LOOKAHEAD` is longer than a stride, so the
        target settles a waypoint or two ahead and stays there: it is chased,
        never stood on. A target the robot is already at has no useful
        direction to steer by.
        """
        if (
            torch.linalg.norm(self.target[0, :2] - self.robot.data.root_pos_w.torch[0, :2])
            < LOOKAHEAD
        ):
            self.index = (self.index + 1) % len(self.waypoints)

    def _steer(self) -> torch.Tensor:
        """The `(v_x, v_y, omega_z)` body twist that drives onto the target.

        Straight at it, whichever way that is relative to the robot: a swerve
        gantry has no preferred direction of travel, and picking one would
        make it turn round at every headland. Yaw is a correction only, back
        onto the row heading.
        """
        offset = self.target[0, :2] - self.robot.data.root_pos_w.torch[0, :2]
        velocity = CRUISE_SPEED * offset / torch.linalg.norm(offset).clamp(min=1e-6)
        yaw = _yaw(self.robot.data.root_quat_w.torch)
        cos, sin = torch.cos(yaw), torch.sin(yaw)
        error = wrap_to_pi(torch.tensor(self.heading, device=yaw.device) - yaw)
        return torch.stack(
            [
                cos * velocity[0] + sin * velocity[1],
                -sin * velocity[0] + cos * velocity[1],
                (YAW_GAIN * error).clamp(-MAX_YAW_RATE, MAX_YAW_RATE),
            ]
        )


def spawn_height(ground: Ground, xy: np.ndarray, heading: float, machine: Straddler) -> float:
    """Where to put the root so no wheel starts inside the terrain.

    The machine goes down level, and its wheels reach `track` across and
    `wheelbase` along, so the ground under all four decides the height and the
    highest of them sets it: `base_link`'s origin is the wheel bottoms' own
    height. A wheel spawned below the surface is pushed back out as an impulse,
    and on ground that slopes under the machine it is one corner's wheel rather
    than four -- which is a moment on something this tall, not a shove.

    The route's own height is the ground on the row line, between the wheels;
    it is what the camera looks at, not what the machine stands on.
    """
    cos, sin = np.cos(heading), np.sin(heading)
    corners = np.asarray(machine.corners)
    wheels = xy + np.column_stack(
        [
            cos * corners[:, 0] - sin * corners[:, 1],
            sin * corners[:, 0] + cos * corners[:, 1],
        ]
    )
    return float(max(ground.height(x, y) for x, y in wheels)) + SPAWN_CLEARANCE


def swerve(
    twist: torch.Tensor, corners: torch.Tensor, current: torch.Tensor, machine: Straddler
) -> tuple[torch.Tensor, torch.Tensor]:
    """Steering angles and wheel speeds realising a body twist.

    Args:
        twist: The body-frame `(v_x, v_y, omega_z)` to realise.
        corners: `machine.corners` on the device the robot is stepped on.
        current: Where each steering joint is now, in radians. Shape is (N,).
        machine: The robot being driven, for its wheel and its top speed.

    Returns:
        Steering angles in radians and wheel speeds in radians per second,
        both shape (N,) and both in `corners` order.

    Each wheel's contact point moves at the body velocity plus the yaw rate
    crossed with where the wheel is, so the module points along that and spins
    at its magnitude.

    A wheel serves a direction just as well pointing the other way and driving
    backwards, so a module takes whichever of the two is the shorter swing
    from where it is now -- never more than a quarter turn, and never a flip
    back and forth during the crab at a headland, where the commanded
    direction sits exactly on the boundary between the two.

    The swing and the target are both measured from `current` and never
    wrapped into a fixed range. Two directions either side of the wrap are a
    few degrees apart but nearly a full turn apart as joint values, so a
    wrapped target would send the wheel all the way round to reach one from
    the other.
    """
    velocity = torch.stack(
        [twist[0] - twist[2] * corners[:, 1], twist[1] + twist[2] * corners[:, 0]], dim=-1
    )
    speed = torch.linalg.norm(velocity, dim=-1)
    swing = wrap_to_pi(torch.atan2(velocity[:, 1], velocity[:, 0]) - current)
    backwards = swing.abs() > torch.pi / 2
    swing = torch.where(backwards, wrap_to_pi(swing + torch.pi), swing)
    return (
        # A wheel asked for no speed at all holds the angle it has; `atan2` of
        # nothing is zero, which would swing it straight.
        current + torch.where(speed < CREEPING, torch.zeros_like(swing), swing),
        (torch.where(backwards, -speed, speed) / machine.wheel_radius).clamp(
            -machine.max_wheel_speed, machine.max_wheel_speed
        ),
    )


def _yaw(quat: torch.Tensor) -> torch.Tensor:
    """The heading of a (1, 4) orientation quaternion, as a scalar tensor.

    `yaw_quat` drops roll and pitch, so this is the direction the robot faces
    across the ground rather than an Euler angle that a slope would tilt.
    """
    flat = yaw_quat(quat)[0]
    return 2.0 * torch.atan2(flat[2], flat[3])
