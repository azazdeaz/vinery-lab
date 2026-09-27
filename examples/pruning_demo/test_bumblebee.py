"""Tests for the robot.

`bumblebee` reaches Isaac Lab through `isaaclab.sim`, so this is skipped
entirely where that isn't installed. The generated URDF is the subject of
most of them: it is where the dimensions above end up.
"""

from __future__ import annotations

import dataclasses
import xml.etree.ElementTree as ElementTree

import numpy as np
import pytest

bumblebee = pytest.importorskip("bumblebee", reason="Isaac Lab is not installed")

import kinematics  # noqa: E402

MACHINE = bumblebee.Bumblebee()


@pytest.fixture
def built() -> ElementTree.Element:
    return ElementTree.fromstring(bumblebee.urdf(MACHINE))


def test_the_chain_is_the_urdfs_own_joints_in_order(built):
    """The solver poses the joints the importer builds, in the order it
    builds them, with the same origins, axes and limits."""
    written = {joint.get("name"): joint for joint in built.iter("joint")}
    moving = [name for name, joint in written.items() if joint.get("type") != "fixed"]

    assert moving == [
        *bumblebee.WHEEL_JOINTS,
        *(j.name for j in MACHINE.chain),
        bumblebee.SHEAR_JOINT,
    ]
    for joint in MACHINE.chain:
        element = written[joint.name]
        origin = element.find("origin")
        xyz = [float(v) for v in origin.get("xyz").split()]
        axis = [float(v) for v in element.find("axis").get("xyz").split()]
        limit = element.find("limit")
        # The arm's first joint is written off the carriage's top, which the
        # mount joint above it climbs.
        if joint.name == bumblebee.ARM_JOINTS[0]:
            xyz[2] += MACHINE.carriage
        assert xyz == pytest.approx(joint.xyz), joint.name
        assert axis == pytest.approx(joint.axis), joint.name
        assert (float(limit.get("lower")), float(limit.get("upper"))) == joint.limits, joint.name
        assert element.get("type") == ("prismatic" if joint.prismatic else "revolute")


def test_the_arm_stands_on_the_rail_and_folds_over_the_deck():
    """At home the mouth is over the deck's edge, not out over the row or in
    the ground, and the rail is where the slide joint says it is."""
    q = np.array([0.0, *bumblebee.ARM_HOME])
    mouth = (kinematics.frames(MACHINE.chain, q)[-1] @ MACHINE.tool)[:3, 3]

    assert MACHINE.deck < mouth[2] < MACHINE.deck + 1.0
    assert abs(mouth[0]) < MACHINE.length / 2 and abs(mouth[1]) < MACHINE.width / 2 + 0.05
    assert MACHINE.chain[0].xyz == pytest.approx(MACHINE.rail)


def test_the_mouth_lies_between_the_blades():
    """What `Shears.cut_through` is given: a rectangle in the hand's frame
    spanning the mouth across and the blades along, whose centre is the
    tool frame's origin."""
    corner, across, up = MACHINE.mouth
    centre = MACHINE.tool[:3, 3]
    shear = MACHINE.shear

    assert np.linalg.norm(across) == pytest.approx(shear.jaw)
    assert np.linalg.norm(up) == pytest.approx(shear.blade - 0.02)
    assert across @ up == pytest.approx(0.0)
    assert corner + across / 2 + up / 2 == pytest.approx(centre + (0.01 - 0.01) * up, abs=0.011)
    # And the tool frame's +X is the pivot, which a cane lies along.
    assert MACHINE.tool[:3, 0] == pytest.approx([1.0, 0.0, 0.0])


def test_the_blades_collide_with_nothing(built):
    """A blade that collided would push the cane it is there to cut aside."""
    for name in ("shear_link", "blade_link"):
        link = built.find(f"./link[@name='{name}']")
        assert link.find("collision") is None, name
        assert link.find("visual") is not None, name


def test_every_part_has_a_positive_size(built):
    sizes = [float(value) for box in built.iter("box") for value in box.get("size").split()] + [
        float(cylinder.get(attribute))
        for cylinder in built.iter("cylinder")
        for attribute in ("radius", "length")
    ]
    assert sizes and all(size > 0 for size in sizes)


def test_the_wheels_carry_the_chassis(built):
    """Four wheels at the corners, about the machine's Y, and the whole UGV
    mass between the chassis and them."""
    masses = {
        link.get("name"): float(link.find("./inertial/mass").get("value"))
        for link in built.iter("link")
    }
    wheels = [built.find(f"./joint[@name='{joint}']") for joint in bumblebee.WHEEL_JOINTS]

    for joint, (x, y) in zip(wheels, MACHINE.corners, strict=True):
        assert joint.find("axis").get("xyz") == "0 1 0"
        assert joint.find("origin").get("xyz") == f"{x} {y} {MACHINE.wheel_radius}"
    ugv = masses["base_link"] + sum(m for name, m in masses.items() if name.startswith("wheel_"))
    assert ugv == pytest.approx(MACHINE.mass)


@pytest.mark.parametrize("impossible", [{"travel": 3.0}, {"clearance": 0.0}, {"deck": 0.2}])
def test_a_machine_that_cannot_be_built_is_refused(impossible):
    with pytest.raises(ValueError):
        dataclasses.replace(MACHINE, **impossible)
