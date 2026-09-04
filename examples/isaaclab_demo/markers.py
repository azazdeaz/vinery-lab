"""Debug markers showing what the route follower is doing.

Yellow spheres are the route, a red one is the waypoint currently being
chased, a green arrow is the velocity being commanded and a blue one the
velocity actually achieved.

Green and blue apart means the policy is not tracking the command; the robot
away from the yellow line means the follower is at fault.

Import only after the Isaac Sim app has been launched.
"""

from __future__ import annotations

import numpy as np
import torch

from isaaclab.assets import Articulation
from isaaclab.markers import VisualizationMarkers
from isaaclab.markers.config import BLUE_ARROW_X_MARKER_CFG, GREEN_ARROW_X_MARKER_CFG, SPHERE_MARKER_CFG
from isaaclab.utils.math import quat_from_euler_xyz, quat_mul

ARROW_BASE = (0.3, 0.3, 0.3)  # scale of a zero-length velocity arrow
ARROW_PER_MS = 1.5  # extra length per m/s of the velocity it draws
ARROW_HEIGHT = 0.5  # m the arrows float above the base, to clear the body


class DebugMarkers:
    """The route drawn once, plus per-step target and velocity markers."""

    def __init__(self, route: np.ndarray):
        # The route never moves, so it is drawn here and not touched again --
        # the markers are a USD point instancer, and its transforms stay where
        # they were written.
        _spheres("Route", radius=0.04, color=(0.9, 0.8, 0.1)).visualize(translations=route)
        self.target = _spheres("Target", radius=0.12, color=(0.9, 0.1, 0.1))
        self.commanded = _arrows("CommandedVelocity", GREEN_ARROW_X_MARKER_CFG)
        self.achieved = _arrows("ActualVelocity", BLUE_ARROW_X_MARKER_CFG)

    def show(self, robot: Articulation, target: torch.Tensor, command: torch.Tensor):
        """Redraw the target and the two velocity arrows."""
        above = robot.data.root_pos_w.torch.clone()
        above[:, 2] += ARROW_HEIGHT
        quat = robot.data.root_quat_w.torch

        self.target.visualize(translations=target)
        scale, orientation = _along(command, quat)
        self.commanded.visualize(translations=above, orientations=orientation, scales=scale)
        scale, orientation = _along(robot.data.root_lin_vel_b.torch, quat)
        self.achieved.visualize(translations=above, orientations=orientation, scales=scale)


def _spheres(name: str, radius: float, color: tuple[float, float, float]) -> VisualizationMarkers:
    """A set of coloured sphere markers under `/Visuals/{name}`."""
    cfg = SPHERE_MARKER_CFG.copy()
    cfg.prim_path = f"/Visuals/{name}"
    cfg.markers["sphere"].radius = radius
    cfg.markers["sphere"].visual_material.diffuse_color = color
    return VisualizationMarkers(cfg)


def _arrows(name: str, marker_cfg) -> VisualizationMarkers:
    """Arrow markers under `/Visuals/{name}`, drawn along the prototype's +x."""
    cfg = marker_cfg.copy()
    cfg.prim_path = f"/Visuals/{name}"
    return VisualizationMarkers(cfg)


def _along(velocity_b: torch.Tensor, quat_w: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
    """Scale and world orientation drawing an arrow along a base-frame velocity.

    Length carries the speed, so a stalled robot shows a stub rather than an
    arrow pointing somewhere meaningless.
    """
    scale = torch.tensor(ARROW_BASE, device=velocity_b.device).repeat(velocity_b.shape[0], 1)
    scale[:, 0] += velocity_b[:, :2].norm(dim=1) * ARROW_PER_MS
    heading = torch.atan2(velocity_b[:, 1], velocity_b[:, 0])
    zeros = torch.zeros_like(heading)
    return scale, quat_mul(quat_w, quat_from_euler_xyz(zeros, zeros, heading))
