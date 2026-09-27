"""Forward and inverse kinematics of a serial chain, in numpy.

A chain is the list of joints `bumblebee.py` writes its URDF from, so the
robot in the simulation and the solver that poses it come from one
description. Frames are 4x4 homogeneous transforms in the chain's base frame,
rotations follow URDF's fixed-axis roll, pitch, yaw, and a joint's motion is
about (or, prismatic, along) its own axis.

Nothing here knows about the simulation: it is what a planner calls to turn a
tool pose into joint positions, and what a test calls without Isaac Sim.
"""

from __future__ import annotations

import dataclasses
import math

import numpy as np


@dataclasses.dataclass(frozen=True)
class Joint:
    """One joint, as URDF describes it: the child frame sits at `xyz`, `rpy`
    in the parent's, and the joint turns about `axis` in that frame -- or
    slides along it, if `prismatic`. `limits` are the joint's range, `effort`
    and `velocity` the ceilings its actuator gets."""

    name: str
    xyz: tuple[float, float, float]
    rpy: tuple[float, float, float] = (0.0, 0.0, 0.0)
    axis: tuple[float, float, float] = (0.0, 0.0, 1.0)
    prismatic: bool = False
    limits: tuple[float, float] = (-2 * math.pi, 2 * math.pi)
    effort: float = 150.0
    velocity: float = 3.14


def rotation(axis, angle: float) -> np.ndarray:
    """The 3x3 rotation of `angle` radians about unit `axis` (Rodrigues)."""
    x, y, z = np.asarray(axis, dtype=float)
    k = np.array([[0.0, -z, y], [z, 0.0, -x], [-y, x, 0.0]])
    return np.eye(3) + math.sin(angle) * k + (1.0 - math.cos(angle)) * k @ k


def transform(xyz=(0.0, 0.0, 0.0), rpy=(0.0, 0.0, 0.0)) -> np.ndarray:
    """A URDF origin as a transform: `rpy` are fixed-axis angles, applied
    roll first."""
    roll, pitch, yaw = rpy
    frame = np.eye(4)
    frame[:3, :3] = (
        rotation((0, 0, 1), yaw) @ rotation((0, 1, 0), pitch) @ rotation((1, 0, 0), roll)
    )
    frame[:3, 3] = xyz
    return frame


def rpy_of(matrix: np.ndarray) -> tuple[float, float, float]:
    """The fixed-axis roll, pitch, yaw of a rotation: the inverse of
    `transform`, for writing a computed orientation into a URDF."""
    pitch = math.atan2(-matrix[2, 0], math.hypot(matrix[0, 0], matrix[1, 0]))
    return (
        math.atan2(matrix[2, 1], matrix[2, 2]),
        pitch,
        math.atan2(matrix[1, 0], matrix[0, 0]),
    )


def along(direction) -> np.ndarray:
    """The rotation taking +Z onto `direction`, for a shape built about its
    own Z axis that has to lie along a link."""
    z = np.array([0.0, 0.0, 1.0])
    d = np.asarray(direction, dtype=float)
    d = d / np.linalg.norm(d)
    axis = np.cross(z, d)
    if np.linalg.norm(axis) < 1e-9:
        return np.eye(3) if d[2] > 0 else rotation((1, 0, 0), math.pi)
    return rotation(axis / np.linalg.norm(axis), math.acos(float(np.clip(z @ d, -1.0, 1.0))))


def frames(chain: tuple[Joint, ...], q) -> np.ndarray:
    """Each joint's child frame at joint positions `q`, in the base frame.
    Shape is (n, 4, 4)."""
    out = np.empty((len(chain), 4, 4))
    frame = np.eye(4)
    for i, (joint, value) in enumerate(zip(chain, q, strict=True)):
        motion = np.eye(4)
        if joint.prismatic:
            motion[:3, 3] = np.asarray(joint.axis) * value
        else:
            motion[:3, :3] = rotation(joint.axis, value)
        frame = frame @ transform(joint.xyz, joint.rpy) @ motion
        out[i] = frame
    return out


def solve(
    chain: tuple[Joint, ...],
    tool: np.ndarray,
    target: np.ndarray,
    q,
    iterations: int = 200,
    damping: float = 0.05,
    step: float = 0.3,
) -> tuple[np.ndarray, float, float]:
    """Joint positions putting the chain's `tool` frame -- a transform off its
    last joint -- at `target`, starting from `q`. Returns the positions and
    how far the tool still is from the target, in meters and radians.

    Damped least squares on the geometric Jacobian, one step per iteration,
    clamped to `step` per joint and to the joints' limits. Nothing about the
    result is guaranteed: the caller reads the errors to decide whether the
    target was reached, and keeps the positions either way, since a pose just
    out of reach is still the closest the arm can come.
    """
    q = np.array(q, dtype=float)
    lower = np.array([joint.limits[0] for joint in chain])
    upper = np.array([joint.limits[1] for joint in chain])
    axes = np.array([joint.axis for joint in chain], dtype=float)
    prismatic = np.array([joint.prismatic for joint in chain])
    target_p, target_r = target[:3, 3], target[:3, :3]

    for _ in range(iterations):
        moved = frames(chain, q)
        end = moved[-1] @ tool
        p, r = end[:3, 3], end[:3, :3]
        # Position error, and the rotation error as the small-angle vector
        # between the two frames' axes.
        error = np.concatenate([target_p - p, 0.5 * np.cross(r, target_r, axis=0).sum(axis=1)])
        if np.linalg.norm(error[:3]) < 1e-4 and np.linalg.norm(error[3:]) < 1e-3:
            break
        # Each joint's axis and origin in the base frame; a joint's own motion
        # leaves its axis where it was, so the moved frame carries it.
        z = np.einsum("nij,nj->ni", moved[:, :3, :3], axes)
        o = moved[:, :3, 3]
        jacobian = np.where(
            prismatic[None, :],
            np.concatenate([z, np.zeros_like(z)], axis=1).T,
            np.concatenate([np.cross(z, p - o), z], axis=1).T,
        )
        gain = jacobian.T @ np.linalg.solve(jacobian @ jacobian.T + damping**2 * np.eye(6), error)
        q = np.clip(q + np.clip(gain, -step, step), lower, upper)

    end = frames(chain, q)[-1] @ tool
    return (
        q,
        float(np.linalg.norm(target_p - end[:3, 3])),
        float(np.linalg.norm(0.5 * np.cross(end[:3, :3], target_r, axis=0).sum(axis=1))),
    )
