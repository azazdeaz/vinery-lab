"""Tests for the robot.

`straddler` reaches Isaac Lab through `isaaclab.sim`, so this is skipped
entirely where that isn't installed. The generated URDF is the subject of most
of them: it is where the dimensions above end up.
"""

from __future__ import annotations

import dataclasses
import xml.etree.ElementTree as ElementTree

import pytest

straddler = pytest.importorskip("straddler", reason="Isaac Lab is not installed")

from vinerylab.isaaclab import ParcelCfg, VineyardCfg  # noqa: E402

MACHINE = straddler.Straddler()


@pytest.fixture
def built() -> ElementTree.Element:
    return ElementTree.fromstring(straddler.urdf(MACHINE))


def test_the_robot_is_sized_to_the_field_it_works():
    """A leg in each alley, and the frame over the trellis wire."""
    vineyard = VineyardCfg(parcel=ParcelCfg(row_spacing=2.75, trellis_height=1.4))

    machine = straddler.Straddler.for_vineyard(vineyard, clearance=0.3)

    assert machine.track == 1.375, "half a row spacing apart puts a leg in each alley"
    assert machine.clear_height == pytest.approx(1.7)


# The published opening of each machine in `PRESETS`, which is what the
# dimensions it is built from have to add back up to.
@pytest.mark.parametrize(
    "name, opening",
    [
        ("bakus_s", (0.60, 1.75)),
        ("bakus_l", (0.80, 2.20)),
        ("ted_180", (1.15, 1.77)),
        ("ted_235", (1.60, 2.35)),
    ],
)
def test_a_preset_opens_as_wide_as_its_maker_publishes(name, opening):
    machine = straddler.PRESETS[name]

    assert (machine.clear_width, machine.clear_height) == pytest.approx(opening)


@pytest.mark.parametrize(
    "impossible",
    [
        {"clear_height": 0.2},  # a frame below the wheel centres
        {"leg_thickness": 1.2},  # legs that meet in the middle
    ],
)
def test_a_machine_with_no_opening_is_refused(impossible):
    """It would otherwise be built inside out, with negative parts."""
    with pytest.raises(ValueError):
        dataclasses.replace(MACHINE, **impossible)


def test_the_wheels_straddle_the_row():
    """Two either side of the row line, and none on it."""
    offsets = MACHINE.corners

    assert sorted(y for _, y in offsets) == [-MACHINE.track / 2] * 2 + [MACHINE.track / 2] * 2
    assert (
        sorted(x for x, _ in offsets) == [-MACHINE.wheelbase / 2] * 2 + [MACHINE.wheelbase / 2] * 2
    )


def test_every_wheel_gets_a_steering_joint_and_a_drive_joint(built):
    """And nothing else moves: the rest of the robot is one rigid frame."""
    moving = {joint.get("name") for joint in built.iter("joint") if joint.get("type") != "fixed"}

    assert moving == set(straddler.STEER_JOINTS + straddler.DRIVE_JOINTS)


def test_every_part_has_a_positive_size(built):
    """A dimension that crosses one of the others shows up here first."""
    sizes = [float(value) for box in built.iter("box") for value in box.get("size").split()] + [
        float(cylinder.get(attribute))
        for cylinder in built.iter("cylinder")
        for attribute in ("radius", "length")
    ]

    assert sizes and all(size > 0 for size in sizes)


def test_the_opening_it_advertises_is_the_opening_it_builds(built):
    """Nothing the frame is made of hangs into the hole it carries over the row."""
    for part in built.find("./link[@name='base_link']").iter("collision"):
        _, y, z = (float(value) for value in part.find("origin").get("xyz").split())
        _, across, tall = (
            float(value) for value in part.find("./geometry/box").get("size").split()
        )

        assert (
            abs(y) - across / 2 >= MACHINE.clear_width / 2 - 1e-9
            or z - tall / 2 >= MACHINE.clear_height - 1e-9
        ), "a part is either outside the opening or above it"


def test_the_steering_axis_runs_through_the_wheel_centre(built):
    """What makes a module a swerve module rather than a castor."""
    for steer, drive, (x, y) in zip(
        straddler.STEER_JOINTS, straddler.DRIVE_JOINTS, MACHINE.corners
    ):
        steer = built.find(f"./joint[@name='{steer}']")
        drive = built.find(f"./joint[@name='{drive}']")
        assert steer.find("axis").get("xyz") == "0 0 1"
        assert steer.find("origin").get("xyz") == f"{x} {y} {MACHINE.wheel_radius}"
        # The drive hangs off the steering link with no offset, so the two
        # axes cross at the wheel centre and the wheel does not scrub.
        assert drive.find("origin") is None
        assert drive.find("axis").get("xyz") == "0 1 0"


def test_the_mass_sits_on_the_wheels(built):
    """Most of it in the drive modules, so the machine does not lean over."""
    masses = {
        link.get("name"): float(link.find("./inertial/mass").get("value"))
        for link in built.iter("link")
    }

    assert masses["base_link"] < sum(value for name, value in masses.items() if "steer_" in name)
    assert sum(masses.values()) == pytest.approx(MACHINE.mass)
