"""Tests for the swerve kinematics the gantry is driven with.

`driver` reaches Isaac Lab through `isaaclab.utils.math`, so this is skipped
entirely where that isn't installed.
"""

from __future__ import annotations

import pytest
import torch

driver = pytest.importorskip("driver", reason="Isaac Lab is not installed")

from straddler import Straddler  # noqa: E402

MACHINE = Straddler()
CORNERS = torch.tensor(MACHINE.corners)
STRAIGHT = torch.zeros(4)


def swerve(vx: float, vy: float, wz: float, current: torch.Tensor = STRAIGHT):
    return driver.swerve(torch.tensor([vx, vy, wz]), CORNERS, current, MACHINE)


# Forward, backward, and the sideways crab that takes the gantry to the next
# row: every wheel points the same way and turns at the same speed.
@pytest.mark.parametrize(
    "twist, angle, ground",
    [
        ((1.0, 0.0, 0.0), 0.0, 1.0),
        # Reversing points the wheels straight ahead and spins them backwards,
        # rather than swinging all four through half a turn.
        ((-1.0, 0.0, 0.0), 0.0, -1.0),
        ((0.0, 0.8, 0.0), torch.pi / 2, 0.8),
        ((0.0, -0.8, 0.0), -torch.pi / 2, 0.8),
    ],
)
def test_translation_drives_every_module_alike(twist, angle, ground):
    steer, speed = swerve(*twist)
    assert torch.allclose(steer, torch.full((4,), angle), atol=1e-6)
    assert torch.allclose(speed, torch.full((4,), ground / MACHINE.wheel_radius), atol=1e-6)


def test_spinning_on_the_spot_points_every_module_along_its_own_circle():
    """A pure yaw rate turns each wheel tangent to the circle it sits on."""
    steer, speed = swerve(0.0, 0.0, 1.0)
    for (x, y), angle, wheel in zip(CORNERS, steer, speed):
        # The commanded direction, allowing for the module that chose to point
        # the other way and drive backwards.
        heading = angle + (torch.pi if wheel < 0 else 0.0)
        assert torch.allclose(
            torch.tensor([torch.cos(heading), torch.sin(heading)]),
            torch.tensor([-y, x]) / torch.linalg.norm(torch.tensor([x, y])),
            atol=1e-6,
        )
        assert (
            abs(abs(wheel) * MACHINE.wheel_radius - torch.linalg.norm(torch.tensor([x, y]))) < 1e-6
        )


# Where the wheels already are when the crab is commanded. The commanded
# direction is exactly the half-turn boundary, so a rule that did not look at
# the current angle would be free to flip across it every step.
@pytest.mark.parametrize(
    "current, expected", [(torch.pi / 2, torch.pi / 2), (-torch.pi / 2, -torch.pi / 2)]
)
def test_a_crabbing_module_is_not_flipped_out_of_the_angle_it_holds(current, expected):
    steer, speed = swerve(0.0, 1.0, 0.0, current=torch.full((4,), current))
    assert torch.allclose(steer, torch.full((4,), expected), atol=1e-6)
    # Whichever way it points, the wheel still drives the body sideways.
    assert torch.allclose(speed.sign(), torch.full((4,), 1.0 if expected > 0 else -1.0))


# A wheel already turned most of a half turn, asked for a direction just the
# other side of the wrap from where it sits. The swing is a few degrees, but
# the two angles are nearly a full turn apart as joint values, so a target
# wrapped into a fixed range would send the wheel all the way round.
@pytest.mark.parametrize("current", [3.0, -3.0])
def test_a_module_never_turns_the_long_way_round(current):
    held = torch.full((4,), current)

    # Just past half a turn from straight ahead, i.e. just across the wrap.
    steer, _ = swerve(-1.0, -0.1 * (1 if current > 0 else -1), 0.0, current=held)

    assert torch.allclose(steer, held + (0.242 if current > 0 else -0.242), atol=1e-3)
    assert (steer - held).abs().max() < torch.pi / 2


def test_a_stopped_module_holds_its_angle():
    """Nothing is commanded, so nothing should swing to `atan2`'s zero."""
    held = torch.full((4,), torch.pi / 2)
    steer, speed = swerve(0.0, 0.0, 0.0, current=held)
    assert torch.allclose(steer, held)
    assert not speed.any()


def test_wheel_speed_is_capped():
    _, speed = swerve(100.0, 0.0, 0.0)
    assert torch.allclose(speed, torch.full((4,), MACHINE.max_wheel_speed))
