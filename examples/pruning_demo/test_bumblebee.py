"""Tests for the robot.

`bumblebee` reaches Isaac Lab through `isaaclab.sim`, so this is skipped
entirely where that isn't installed. The generated URDF is the subject of
most of them: it is where the dimensions above end up.
"""

from __future__ import annotations

import dataclasses
import math
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


def test_the_mouth_is_what_the_closing_blade_sweeps():
    """The tool frame's origin, where a cane is brought to, lies in the wedge
    between the edges: clear of the open blade's plate, and crossed by the
    plate on its way shut. The frame's +X is the pivot, which a cane lies
    along, and the plate's rectangle faces it."""
    shear = MACHINE.shear
    centre = MACHINE.tool[:3, 3]

    def reached(angle: float) -> bool:
        corner, across, up = MACHINE.blade(angle)
        offset = centre - corner
        a, b = offset @ across / (across @ across), offset @ up / (up @ up)
        return 0 <= a <= 1 and 0 <= b <= 1

    assert not reached(shear.opening)
    assert any(reached(angle) for angle in np.linspace(0.0, shear.opening, 100))
    y, z = shear.mouth
    assert -shear.blade * math.tan(shear.opening) < y < 0 and 0 < z < shear.blade
    assert MACHINE.tool[:3, 0] == pytest.approx([1.0, 0.0, 0.0])
    corner, across, up = MACHINE.blade(0.0)
    normal = np.cross(across, up)
    assert abs(normal @ MACHINE.tool[:3, 0]) == pytest.approx(np.linalg.norm(normal))


def test_the_blades_collide_whole(built):
    """Each plate is one box, drawn and colliding alike, with its cutting
    edge on the link's XZ plane: what pushes a cane about is the plate the
    stroke stands at the cane."""
    shear = MACHINE.shear
    assert any(
        width == pytest.approx(shear.head)
        for _, width in _boxes(built.find("./link[@name='shear_link']"), "collision")
    ), "the head collides"
    for name, side in (("shear_link", 1), (bumblebee.BLADE, -1)):
        link = built.find(f"./link[@name='{name}']")
        for tag in ("visual", "collision"):
            [(y, width)] = [
                box for box in _boxes(link, tag) if box[1] == pytest.approx(shear.width)
            ]
            assert side * y - width / 2 == pytest.approx(0.0), f"{name} {tag}: edge on the XZ plane"


def test_a_cane_in_the_mouth_is_pinched_where_the_edges_are_a_cane_apart():
    """`pinch` is the angle at which the moving edge's plane is a radius
    from the cane's axis, and `wedge` maps a crossing back to where in the
    shear's frame the cane is."""
    shear = MACHINE.shear
    y, z, radius = -0.005, 0.06, 0.005
    angle = shear.pinch(y, z, radius)
    assert 0 < angle < shear.opening
    assert y * math.cos(angle) + z * math.sin(angle) == pytest.approx(radius)
    assert shear.pinch(y, z, 0.0) == pytest.approx(math.atan(-y / z)), "no radius: the axis itself"
    corner, across, up = MACHINE.wedge()
    point = (bumblebee.TOOL @ [shear.bypass, *shear.mouth, 1.0])[:3]
    a, b = (point - corner) @ across / (across @ across), (point - corner) @ up / (up @ up)
    assert np.cross(across, up) @ (point - corner) == pytest.approx(0.0), "in the plate's plane"
    assert (shear.reach * (a - 1), shear.blade * b) == pytest.approx(shear.mouth)


def _boxes(link: ElementTree.Element, tag: str) -> list[tuple[float, float]]:
    """Each box under `link`'s `tag` elements, as its Y centre and Y size."""
    boxes = []
    for element in link.findall(tag):
        box = element.find("geometry/box")
        if box is not None:
            y = float(element.find("origin").get("xyz").split()[1])
            boxes.append((y, float(box.get("size").split()[1])))
    return boxes


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
