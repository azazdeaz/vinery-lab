"""Driving one quadruped along a route.

Two nested loops, both in `Driver.control`: a waypoint follower turns the
route into a velocity command, and a pre-trained locomotion policy turns that
command into joint targets.

Import only after the Isaac Sim app has been launched.
"""

from __future__ import annotations

import numpy as np
import torch

from isaaclab.assets import Articulation
from isaaclab.utils.assets import ISAACLAB_NUCLEUS_DIR, read_file
from isaaclab.utils.math import quat_apply_inverse

from route import STRIDE

# Blind flat-terrain policy for ANYmal-C, as shipped with Isaac Lab: 48
# observations in, 12 joint-position offsets out.
POLICY_PATH = f"{ISAACLAB_NUCLEUS_DIR}/Policies/ANYmal-C/Blind/policy.pt"
ACTION_SCALE = 0.5
# 200 Hz physics under a 50 Hz policy, the rates it was trained at. Change
# either and a joint-position target stops meaning what the policy learnt.
SIM_DT = 0.005
DECIMATION = 4

# -- Waypoint follower.
CRUISE_SPEED = 0.8  # m/s commanded along the alley
TURN_GAIN = 1.5  # rad/s of yaw command per rad of heading error
MAX_TURN = 1.0  # rad/s
LOOKAHEAD = 1.5 * STRIDE  # see `_advance` for why this is longer than a stride
STAND_HEIGHT = 0.6  # m the base is placed above the ground on a (re)spawn
FALLEN = -0.5  # gravity's z in the base frame is above this once tipped over


class Driver:
    """Walks `robot` along `route`, one waypoint at a time, forever."""

    def __init__(self, route: np.ndarray, robot: Articulation):
        self.route = route
        self.robot = robot
        self.waypoints = torch.tensor(route, dtype=torch.float32, device=robot.device)
        self.policy = torch.jit.load(read_file(POLICY_PATH)).to(robot.device).eval()
        self.index = 0
        self.action = torch.zeros(1, robot.num_joints, device=robot.device)
        self.command = torch.zeros(1, 3, device=robot.device)

    @property
    def target(self) -> torch.Tensor:
        """The waypoint currently being chased. Shape is (1, 3)."""
        return self.waypoints[self.index].unsqueeze(0)

    @torch.no_grad()  # the policy is inference-only, and warp rejects tensors that carry a grad
    def control(self):
        """Advance the route and drive one policy step. Call every `DECIMATION` sim steps."""
        self._advance()
        self._recover()
        self.command = self._steer()
        # The order the policy was trained with; changing it silently produces
        # a robot that twitches rather than an error.
        default_joint_pos = self.robot.data.default_joint_pos.torch
        observation = torch.cat(
            [
                self.robot.data.root_lin_vel_b.torch,
                self.robot.data.root_ang_vel_b.torch,
                self.robot.data.projected_gravity_b.torch,
                self.command,
                self.robot.data.joint_pos.torch - default_joint_pos,
                self.robot.data.joint_vel.torch,
                self.action,
            ],
            dim=-1,
        )
        self.action = self.policy(observation)
        self.robot.set_joint_position_target_index(target=default_joint_pos + ACTION_SCALE * self.action)

    def place(self):
        """Stand the robot on the current waypoint, facing the one after it."""
        here, ahead = self.route[self.index], self.route[(self.index + 1) % len(self.route)]
        yaw = np.arctan2(ahead[1] - here[1], ahead[0] - here[0])
        device = self.robot.device
        pose = torch.tensor(
            # Position, then orientation as a quaternion in (x, y, z, w).
            [[here[0], here[1], here[2] + STAND_HEIGHT, 0.0, 0.0, np.sin(yaw / 2), np.cos(yaw / 2)]],
            dtype=torch.float32,
            device=device,
        )
        self.robot.write_root_pose_to_sim_index(root_pose=pose)
        self.robot.write_root_velocity_to_sim_index(root_velocity=torch.zeros(1, 6, device=device))
        self.robot.write_joint_position_to_sim_index(position=self.robot.data.default_joint_pos.torch.clone())
        self.robot.write_joint_velocity_to_sim_index(velocity=self.robot.data.default_joint_vel.torch.clone())
        self.robot.reset()

    def _advance(self):
        """Move the target on once the robot is nearly on it.

        One waypoint per call, and `LOOKAHEAD` is longer than a stride, so the
        target settles a waypoint or two ahead and stays there: it is chased,
        never stood on. A target the robot is already at has no useful
        direction to steer by.
        """
        if torch.linalg.norm(self.target[0, :2] - self.robot.data.root_pos_w.torch[0, :2]) < LOOKAHEAD:
            self.index = (self.index + 1) % len(self.waypoints)

    def _recover(self):
        """Put the robot back on its feet if it tipped over.

        A blind flat-terrain policy trips on rough ground sooner or later, and
        a run that ends on its side is a run that stops reporting anything.
        """
        if self.robot.data.projected_gravity_b.torch[0, 2] > FALLEN:
            print(f"[INFO]: Robot fell over at waypoint {self.index}/{len(self.waypoints)}, resetting...")
            self.place()
            self.action.zero_()

    def _steer(self) -> torch.Tensor:
        """The `(v_x, v_y, omega_z)` command that walks the base onto the target.

        Turn-then-go: yaw at a rate proportional to the heading error and taper
        the forward speed off with it, so the robot pivots on the headland
        instead of arcing wide through the vines. No sideways command -- the
        policy can strafe, but measured against a straight alley it tracks no
        better for it, and a robot that walks the way it faces is easier to
        reason about.
        """
        offset = self.target - self.robot.data.root_pos_w.torch
        local = quat_apply_inverse(self.robot.data.root_quat_w.torch, offset)
        error = torch.atan2(local[:, 1], local[:, 0])
        return torch.stack(
            [
                CRUISE_SPEED * error.cos().clamp(min=0.0),
                torch.zeros_like(error),
                (TURN_GAIN * error).clamp(-MAX_TURN, MAX_TURN),
            ],
            dim=-1,
        )
