"""A Bumblebee-like pruning robot, generated from its published dimensions.

Bumblebee (Silwal et al., 2021) is a Clearpath Warthog driving the alley with
a UR5 on a 1.35 m linear slide across its deck and a bypass shear in the arm's
hand. This is that layout from primitives: a skid-steer chassis on four
wheels, a rail along the deck's row-side edge carrying a carriage on a
prismatic joint, a six-joint arm with the UR5's link offsets standing on the
carriage, and a shear head whose one blade swings on a revolute joint. Every
dimension is a field of `Bumblebee`; `urdf` writes the robot from them and
`Bumblebee.chain` is the same joints for `kinematics`, so the arm the solver
poses is the arm the simulation steps.

The shear touches what it prunes. Its head and both blades collide with the
canes, so a cane the shear comes in on is pushed aside or funnelled into the
mouth, and the moving blade carries it across the mouth onto the fixed one.
Held there, it is cut where the moving blade's edge reaches its axis: tick by
tick while the shear closes, `pruner.Stroke` hands `Bumblebee.blade` at the
blade's angle to `Shears.cut_through`, and keeps the moving blade's collider
standing at the cane while the blade itself closes on through it.

The robot drives with the row on its left: the rail is on the +Y edge of the
deck, and the arm reaches out that way.
"""

from __future__ import annotations

import dataclasses
import hashlib
import math
import pathlib
import tempfile

import numpy as np

import isaaclab.sim as sim_utils
from isaaclab.actuators import ImplicitActuatorCfg
from isaaclab.assets import ArticulationCfg

from kinematics import Joint, along, rpy_of, transform

CORNERS = ("front_left", "front_right", "rear_left", "rear_right")
WHEEL_JOINTS = [f"wheel_{corner}" for corner in CORNERS]
"""The drive joints, in `CORNERS` order. A skid steer has no steering joint:
the left pair and the right pair turn at different speeds."""

LEFT = [joint for joint in WHEEL_JOINTS if "left" in joint]
RIGHT = [joint for joint in WHEEL_JOINTS if "right" in joint]

SLIDE_JOINT = "slide"
SHEAR_JOINT = "shear"
"""The one joint of the cutter: 0 is closed, `Shear.opening` is open."""

ARM_BASE = "arm_base"
"""The link the arm stands on: the carriage, once the importer merges the
fixed joint between them."""

HAND = "wrist_3_link"
"""The body the shear is fixed to, and whose pose says where the mouth is."""

BLADE = "blade_link"
"""The body the moving blade is, swinging on `SHEAR_JOINT`."""

# -- The UR5, as its URDF places each joint in the frame of the link before
# it. Zero everywhere is the arm lying out along +X.
UR5 = (
    Joint("shoulder_pan_joint", (0.0, 0.0, 0.089159)),
    Joint("shoulder_lift_joint", (0.0, 0.13585, 0.0), (0.0, math.pi / 2, 0.0), (0.0, 1.0, 0.0)),
    Joint("elbow_joint", (0.0, -0.1197, 0.425), axis=(0.0, 1.0, 0.0), limits=(-math.pi, math.pi)),
    Joint(
        "wrist_1_joint", (0.0, 0.0, 0.39225), (0.0, math.pi / 2, 0.0), (0.0, 1.0, 0.0), effort=28.0
    ),
    Joint("wrist_2_joint", (0.0, 0.093, 0.0), effort=28.0),
    Joint("wrist_3_joint", (0.0, 0.0, 0.09465), axis=(0.0, 1.0, 0.0), effort=28.0),
)
ARM_JOINTS = [joint.name for joint in UR5]
SHOULDER_JOINTS, WRIST_JOINTS = ARM_JOINTS[:3], ARM_JOINTS[3:]

TOOL = transform((0.0, 0.0823, 0.0), (-math.pi / 2, 0.0, 0.0))
"""The flange frame off the last wrist: +Z out of the hand, where a tool
bolts on."""

# Link radii and masses, in the order of `UR5`, with the base's first. The
# masses are the UR5's own; the radii draw links of about its girth.
ARM_RADII = (0.06, 0.06, 0.055, 0.045, 0.04, 0.04, 0.035)
ARM_MASSES = (4.0, 3.7, 8.393, 2.275, 1.219, 1.219, 0.1879)

ARM_HOME = (0.0, -2.0, 2.2, -1.8, -1.57, 0.0)
"""Joint positions the arm is folded to for driving: elbow up, hand over the
deck. The slide and the shear start at zero, mid-rail and closed."""

# -- Actuation. Position drives on the arm, the slide and the shear, and
# velocity drives on the wheels.
SHOULDER_STIFFNESS = 2000.0
SHOULDER_DAMPING = 200.0
"""The three joints carrying the arm's weight: stiff, so that weight sags
the mouth a couple of centimetres off its pose at most. The mouth is a few
centimetres across, and a cane outside it when the blades meet is not cut."""
WRIST_STIFFNESS = 100.0
WRIST_DAMPING = 10.0
"""The three wrist joints, which carry little: gains a twentieth of the
shoulder's, so a wrist's 28 N m is not saturated by a few tenths of a radian
of error."""
ARMATURE = {"shoulder": 0.1, "wrist": 0.02, "shear": 0.002}
"""Reflected rotor inertia added to each joint, in kg m^2, the way a geared
servo carries its motor's. Without it a wrist -- a link of a few hundred
grams on a 28 N m drive -- turns a tenth of a radian in one physics substep
and chatters about its target instead of settling on it."""
SLIDE_STIFFNESS = 4000.0
SLIDE_DAMPING = 400.0
SHEAR_STIFFNESS = 20.0
SHEAR_DAMPING = 1.0
DRIVE_DAMPING = 1500.0  # with zero stiffness this is the velocity drive's strength
DRIVE_EFFORT_PER_KG = 0.5
"""N m per kilogram of machine each wheel may push with: what a wheel can
push is what is standing on it."""

# How the chassis mass is shared out.
WHEEL_SHARE = 0.2  # the four wheels together

MATERIALS = {
    "chassis": "0.93 0.72 0.10 1",
    "tire": "0.05 0.05 0.05 1",
    "steel": "0.55 0.57 0.60 1",
    "arm": "0.32 0.38 0.48 1",
}
"""Colours, as URDF rgba: a yellow chassis, rubber, galvanized steel and the
blue-grey of an industrial arm."""


@dataclasses.dataclass(frozen=True)
class Shear:
    """A bypass shear: a fixed blade and one that swings past it.

    Both blades are plates standing out of the hand along its +Z, in its YZ
    plane, with their cutting edges meeting on its XZ plane when closed. The
    moving one swings about the hand's X axis at the blades' base, so the
    mouth -- the wedge between the two edges -- opens in the hand's -Y, and
    closing it sweeps whatever lies across the blades.
    """

    blade: float = 0.08
    """Blade length, along the hand's +Z."""

    width: float = 0.02
    """Plate width, from the cutting edge back."""

    thickness: float = 0.004
    """Plate thickness, along the pivot."""

    opening: float = 0.9
    """How far the moving blade swings open, in radians: about fifty
    degrees, an electric pruner's. The mouth is then wide enough that a cane
    a couple of centimetres off the planned point still lies between the
    blades."""

    head: float = 0.05
    """The body the blades are set in: a cube of this edge, holding the drive
    a real shear has -- Bumblebee's takes about 320 N to part an 8 mm cane."""

    @property
    def bypass(self) -> float:
        """The moving plate's offset from the fixed one along the pivot: a
        plate's thickness of clearance between them, and half of one more."""
        return 1.5 * self.thickness

    @property
    def mouth(self) -> tuple[float, float]:
        """Where a cane is brought to, as (y, z) in the shear's own frame:
        the point of the open mouth furthest from both edges and from the
        blades' tips, the centre of the widest circle the mouth holds, so a
        cane that far off the planned point in any direction still lies
        between the blades -- 2.4 cm at the default size."""
        half = self.opening / 2
        d = self.blade / (1 + math.sin(half))
        return -d * math.sin(half), d * math.cos(half)

    @property
    def reach(self) -> float:
        """How far across, in -Y, the mouth opens: where the moving edge's
        plane is at full opening, a blade's length out from the pivot."""
        return self.blade * math.tan(self.opening)

    def pinch(self, y: float, z: float, radius: float) -> float:
        """The blade angle at which the moving edge's plane is `radius` from
        the point (y, z) of the shear's frame, on the mouth's side of it:
        where the moving blade touches a cane of that radius lying across
        the mouth there, and shutting further would push it."""
        return math.asin(min(1.0, radius / math.hypot(y, z))) - math.atan2(y, z)


@dataclasses.dataclass(frozen=True)
class Bumblebee:
    """One machine, in meters and kilograms.

    The chassis is a Clearpath Warthog at its published size; the slide and
    the arm's offsets are the paper's and the UR5's. Everything is free to
    move with `dataclasses.replace`.
    """

    length: float = 1.52
    width: float = 1.38
    """The chassis, over the wheels."""

    deck: float = 0.8
    """The chassis top, above the ground."""

    clearance: float = 0.3
    """The chassis underside, above the ground."""

    wheelbase: float = 1.1
    wheel_radius: float = 0.3
    wheel_width: float = 0.2
    mass: float = 280.0
    """The UGV alone, without the gantry and the arm."""

    travel: float = 1.35
    """The slide's stroke, along the row, centered on the chassis."""

    rail_height: float = 0.2
    """The rail top above the deck; the carriage rides on it."""

    carriage: float = 0.08
    """The carriage's height, which is what the arm stands on."""

    shear: Shear = Shear()
    max_speed: float = 1.0
    """m/s on the ground, flat out. A Warthog does five; an alley is not the
    place for it."""

    def __post_init__(self):
        if self.travel >= self.length + 2 * self.wheel_radius:
            raise ValueError(f"{self} slides past the ends of its chassis")
        if self.clearance <= 0 or self.deck <= self.clearance:
            raise ValueError(f"{self} has no chassis between the ground and its deck")

    @property
    def track(self) -> float:
        """Wheel centre to wheel centre, across the machine."""
        return self.width - self.wheel_width

    @property
    def corners(self) -> list[tuple[float, float]]:
        """Each wheel's `(x, y)` in the base frame, in `CORNERS` order."""
        return [
            (x * self.wheelbase / 2, y * self.track / 2)
            for x, y in ((1, 1), (1, -1), (-1, 1), (-1, -1))
        ]

    @property
    def max_wheel_speed(self) -> float:
        """rad/s at `max_speed`."""
        return self.max_speed / self.wheel_radius

    @property
    def drive_effort(self) -> float:
        """N m a wheel may exert. See `DRIVE_EFFORT_PER_KG`."""
        return DRIVE_EFFORT_PER_KG * self.mass

    @property
    def rail(self) -> tuple[float, float, float]:
        """Where the rail's top surface is, at mid-stroke, in the base frame:
        on the deck's +Y edge."""
        return (0.0, self.width / 2 - 0.1, self.deck + self.rail_height)

    @property
    def chain(self) -> tuple[Joint, ...]:
        """The slide and the arm, as `kinematics` reads them: seven joints from
        the base frame to the last wrist, `TOOL` still to come."""
        x, y, z = self.rail
        first = dataclasses.replace(UR5[0], xyz=(0.0, 0.0, self.carriage + UR5[0].xyz[2]))
        slide = Joint(
            SLIDE_JOINT,
            (x, y, z),
            axis=(1.0, 0.0, 0.0),
            prismatic=True,
            limits=(-self.travel / 2, self.travel / 2),
            effort=1000.0,
            velocity=0.5,
        )
        return (slide, first, *UR5[1:])

    def blade(self, angle: float) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        """The moving blade's plate with the shear at `angle`, in the `HAND`
        frame, as the rectangle `Shears.cut_through` takes: a corner and its
        two edges, across the plate to its cutting edge and along it. A cane
        whose axis crosses it is cut where it does."""
        s = self.shear
        plate = TOOL @ transform(rpy=(angle, 0.0, 0.0))
        corner = plate @ np.array([s.bypass, -s.width, 0.0, 1.0])
        across = plate[:3, :3] @ np.array([0.0, s.width, 0.0])
        up = plate[:3, :3] @ np.array([0.0, 0.0, s.blade])
        return corner[:3], across, up

    def wedge(self) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        """The mouth at full opening, in the `HAND` frame, as the rectangle
        `Shears.crossing` takes: in the moving plate's plane, `Shear.reach`
        across to the fixed edge and the blades' length out. A cane crossing
        it at (a, b) lies at y = reach (a - 1), z = blade b of the shear's
        frame."""
        s = self.shear
        corner = TOOL @ np.array([s.bypass, -s.reach, 0.0, 1.0])
        across = TOOL[:3, :3] @ np.array([0.0, s.reach, 0.0])
        up = TOOL[:3, :3] @ np.array([0.0, 0.0, s.blade])
        return corner[:3], across, up

    @property
    def tool(self) -> np.ndarray:
        """The frame off the last wrist a target is posed for: the mouth, +Z
        along the blades and +X along the pivot -- which a cane has to lie
        along to be cut."""
        y, z = self.shear.mouth
        return TOOL @ transform((0.0, y, z))


def bumblebee_cfg(machine: Bumblebee, prim_path: str) -> ArticulationCfg:
    """An articulation spawning `machine`, converting its URDF on first use."""
    return ArticulationCfg(
        prim_path=prim_path,
        spawn=sim_utils.UrdfFileCfg(
            asset_path=_write_urdf(machine),
            usd_dir=str(_CACHE),
            fix_base=False,
            # Boxes and cylinders: a hull of each is the shape itself.
            collision_type="Convex Hull",
            joint_drive=None,  # the actuators below own the gains
            activate_contact_sensors=False,
        ),
        init_state=ArticulationCfg.InitialStateCfg(
            joint_pos=dict(zip(ARM_JOINTS, ARM_HOME, strict=True))
        ),
        actuators={
            "shoulder": ImplicitActuatorCfg(
                joint_names_expr=SHOULDER_JOINTS,
                stiffness=SHOULDER_STIFFNESS,
                damping=SHOULDER_DAMPING,
                effort_limit=UR5[0].effort,
                armature=ARMATURE["shoulder"],
            ),
            "wrist": ImplicitActuatorCfg(
                joint_names_expr=WRIST_JOINTS,
                stiffness=WRIST_STIFFNESS,
                damping=WRIST_DAMPING,
                effort_limit=UR5[-1].effort,
                armature=ARMATURE["wrist"],
            ),
            "slide": ImplicitActuatorCfg(
                joint_names_expr=[SLIDE_JOINT],
                stiffness=SLIDE_STIFFNESS,
                damping=SLIDE_DAMPING,
                effort_limit=1000.0,
            ),
            "shear": ImplicitActuatorCfg(
                joint_names_expr=[SHEAR_JOINT],
                stiffness=SHEAR_STIFFNESS,
                damping=SHEAR_DAMPING,
                effort_limit=10.0,
                armature=ARMATURE["shear"],
            ),
            # Zero stiffness makes these velocity drives: the target is a
            # speed and `damping` is how hard they hold it.
            "drive": ImplicitActuatorCfg(
                joint_names_expr=WHEEL_JOINTS,
                stiffness=0.0,
                damping=DRIVE_DAMPING,
                effort_limit=machine.drive_effort,
            ),
        },
    )


##
# URDF generation.
##

_CACHE = pathlib.Path(tempfile.gettempdir()) / "pruning_demo"
"""Where a generated robot is kept, named for its contents: two sizes are two
robots, and the URDF converter caches on the name too."""


def _write_urdf(machine: Bumblebee) -> str:
    """Build the URDF for one machine and return its path, writing it once."""
    text = urdf(machine)
    _CACHE.mkdir(parents=True, exist_ok=True)
    path = _CACHE / f"bumblebee_{hashlib.sha256(text.encode()).hexdigest()[:16]}.urdf"
    path.write_text(text)
    return str(path)


def urdf(machine: Bumblebee) -> str:
    """`machine` as a URDF document.

    `base_link` is the chassis with its origin on the ground under the
    chassis centre, the rail and its posts included, since nothing between
    them moves. The wheels hang off it on continuous joints, the carriage on
    the slide, and the arm stands on the carriage through a fixed joint the
    importer merges away.
    """
    m = machine
    x, y, z = m.rail
    rail_length = m.travel + 0.2
    chassis_height = m.deck - m.clearance
    chassis_mass = (1 - WHEEL_SHARE) * m.mass
    chassis = "\n".join(
        [
            _box(
                (0.0, 0.0, m.clearance + chassis_height / 2),
                (m.length, m.width - 2 * m.wheel_width, chassis_height),
                "chassis",
            ),
            _box((x, y, z - 0.03), (rail_length, 0.08, 0.06), "steel"),
            *(
                _box(
                    (post * m.travel / 2, y, (m.deck + z - 0.06) / 2),
                    (0.06, 0.06, z - 0.06 - m.deck),
                    "steel",
                )
                for post in (-1, 1)
            ),
        ]
    )
    wheels = "\n".join(
        _wheel(m, joint, corner, x, y)
        for joint, corner, (x, y) in zip(WHEEL_JOINTS, CORNERS, m.corners, strict=True)
    )
    slide, *arm = m.chain
    return f"""<?xml version="1.0"?>
<!-- Generated by bumblebee.py; edit that, not this. -->
<robot name="bumblebee">
  <link name="base_link">
{chassis}
    <inertial>
      <origin xyz="0 0 {m.clearance + chassis_height / 2}"/>
      <mass value="{chassis_mass}"/>
{_box_inertia(chassis_mass, m.length, m.width, chassis_height)}
    </inertial>
  </link>
{wheels}
{_joint(slide, "base_link", "carriage")}
  <link name="carriage">
{_box((0.0, 0.0, m.carriage / 2), (0.15, 0.15, m.carriage), "steel")}
    <inertial>
      <origin xyz="0 0 {m.carriage / 2}"/>
      <mass value="5.0"/>
{_box_inertia(5.0, 0.15, 0.15, m.carriage)}
    </inertial>
  </link>
{_arm(m, arm)}
{_shear(m)}
</robot>
"""


def _wheel(m: Bumblebee, joint: str, corner: str, x: float, y: float) -> str:
    """One wheel on a continuous joint about the machine's Y."""
    mass = WHEEL_SHARE * m.mass / 4
    return f"""
  <joint name="{joint}" type="continuous">
    <parent link="base_link"/>
    <child link="wheel_{corner}_link"/>
    <origin xyz="{x} {y} {m.wheel_radius}"/>
    <axis xyz="0 1 0"/>
    <limit effort="{m.drive_effort}" velocity="{m.max_wheel_speed}"/>
  </joint>
  <link name="wheel_{corner}_link">
{_cylinder((0.0, 0.0, 0.0), (math.pi / 2, 0.0, 0.0), m.wheel_radius, m.wheel_width, "tire")}
    <inertial>
      <origin rpy="{math.pi / 2} 0 0"/>
      <mass value="{mass}"/>
{_cylinder_inertia(mass, m.wheel_radius, m.wheel_width)}
    </inertial>
  </link>"""


def _arm(m: Bumblebee, joints: list[Joint]) -> str:
    """The arm on the carriage: a fixed base link, then one link per joint,
    each drawn as a cylinder reaching to where the next joint starts."""
    parts = [
        f"""
  <joint name="arm_mount" type="fixed">
    <parent link="carriage"/>
    <child link="{ARM_BASE}"/>
    <origin xyz="0 0 {m.carriage}"/>
  </joint>
  <link name="{ARM_BASE}">
{_link_shape(joints[0].xyz[2] - m.carriage, 0.0, ARM_RADII[0], "arm")}
    <inertial>
      <mass value="{ARM_MASSES[0]}"/>
{_cylinder_inertia(ARM_MASSES[0], ARM_RADII[0], 0.1)}
    </inertial>
  </link>"""
    ]
    # The arm's first joint is `m.chain`'s, offset by the carriage the mount
    # above already climbs.
    first = dataclasses.replace(joints[0], xyz=(0.0, 0.0, joints[0].xyz[2] - m.carriage))
    reaches = [*(joint.xyz for joint in joints[1:]), tuple(TOOL[:3, 3])]
    parent = ARM_BASE
    for joint, reach, radius, mass in zip(
        [first, *joints[1:]], reaches, ARM_RADII[1:], ARM_MASSES[1:], strict=True
    ):
        child = joint.name.replace("_joint", "_link")
        length = float(np.linalg.norm(reach))
        parts.append(_joint(joint, parent, child))
        parts.append(f"""
  <link name="{child}">
{_link_shape(length, radius, radius, "arm", reach)}
    <inertial>
      <origin xyz="{reach[0] / 2} {reach[1] / 2} {reach[2] / 2}"/>
      <mass value="{mass}"/>
{_cylinder_inertia(mass, radius, max(length, 0.05))}
    </inertial>
  </link>""")
        parent = child
    return "\n".join(parts)


def _shear(m: Bumblebee) -> str:
    """The shear head on the flange, and its one moving blade. The fixed
    plate lies back from the hand's XZ plane along +Y, the moving one along
    -Y in its own frame, so their edges meet on that plane at zero."""
    s = m.shear
    head = f"""
  <joint name="tool" type="fixed">
    <parent link="{HAND}"/>
    <child link="shear_link"/>
    <origin xyz="{TOOL[0, 3]} {TOOL[1, 3]} {TOOL[2, 3]}" rpy="{" ".join(str(a) for a in rpy_of(TOOL[:3, :3]))}"/>
  </joint>
  <link name="shear_link">
{_box((0.0, 0.0, -s.head / 2), (s.head, s.head, s.head), "steel")}
{_plate(s, 0.0, 1)}
    <inertial>
      <origin xyz="0 0 {-s.head / 2}"/>
      <mass value="0.5"/>
{_box_inertia(0.5, s.head, s.head, s.head)}
    </inertial>
  </link>
  <joint name="{SHEAR_JOINT}" type="revolute">
    <parent link="shear_link"/>
    <child link="{BLADE}"/>
    <axis xyz="1 0 0"/>
    <limit lower="0" upper="{s.opening}" effort="10" velocity="6"/>
  </joint>
  <link name="{BLADE}">
{_plate(s, s.bypass, -1)}
    <inertial>
      <origin xyz="0 0 {s.blade / 2}"/>
      <mass value="0.05"/>
{_box_inertia(0.05, s.thickness, s.width, s.blade)}
    </inertial>
  </link>"""
    return head


def _plate(s: Shear, x: float, side: int) -> str:
    """One blade: a plate `x` along the pivot with its cutting edge on the
    link's XZ plane, lying back from it towards `side` (+1 or -1) in Y."""
    return _box((x, side * s.width / 2, s.blade / 2), (s.thickness, s.width, s.blade), "steel")


def _joint(joint: Joint, parent: str, child: str) -> str:
    kind = "prismatic" if joint.prismatic else "revolute"
    return f"""
  <joint name="{joint.name}" type="{kind}">
    <parent link="{parent}"/>
    <child link="{child}"/>
    <origin xyz="{" ".join(str(v) for v in joint.xyz)}" rpy="{" ".join(str(v) for v in joint.rpy)}"/>
    <axis xyz="{" ".join(str(v) for v in joint.axis)}"/>
    <limit lower="{joint.limits[0]}" upper="{joint.limits[1]}" effort="{joint.effort}" velocity="{joint.velocity}"/>
  </joint>"""


def _link_shape(
    length: float, base: float, radius: float, material: str, reach=(0.0, 0.0, 1.0)
) -> str:
    """A link's cylinder: from its origin along `reach`, a stub where the next
    joint sits on top of this one."""
    reach = np.asarray(reach, dtype=float)
    length = max(length, 0.06)
    direction = reach if np.linalg.norm(reach) > 1e-9 else np.array([0.0, 0.0, 1.0])
    axis = along(direction)
    centre = tuple(axis @ np.array([0.0, 0.0, length / 2]))
    return _cylinder(centre, rpy_of(axis), max(base, radius), length, material)


def _box(xyz, size, material: str) -> str:
    shape = (
        f'      <origin xyz="{xyz[0]} {xyz[1]} {xyz[2]}"/>\n'
        f'      <geometry><box size="{size[0]} {size[1]} {size[2]}"/></geometry>'
    )
    return _shaped(shape, ("visual", "collision"), material)


def _cylinder(xyz, rpy, radius: float, length: float, material: str) -> str:
    shape = (
        f'      <origin xyz="{xyz[0]} {xyz[1]} {xyz[2]}" rpy="{rpy[0]} {rpy[1]} {rpy[2]}"/>\n'
        f'      <geometry><cylinder radius="{radius}" length="{length}"/></geometry>'
    )
    return _shaped(shape, ("visual", "collision"), material)


def _shaped(shape: str, tags: tuple[str, ...], material: str) -> str:
    """One geometry repeated as each of `tags`, coloured where it is a visual."""
    colour = f'\n      <material name="{material}"><color rgba="{MATERIALS[material]}"/></material>'
    return "\n".join(
        f"    <{tag}>\n{shape}{colour if tag == 'visual' else ''}\n    </{tag}>" for tag in tags
    )


def _box_inertia(mass: float, x: float, y: float, z: float) -> str:
    return _inertia(
        mass * (y * y + z * z) / 12, mass * (x * x + z * z) / 12, mass * (x * x + y * y) / 12
    )


def _cylinder_inertia(mass: float, radius: float, length: float) -> str:
    across = mass * (3 * radius * radius + length * length) / 12
    return _inertia(across, across, mass * radius * radius / 2)


def _inertia(ixx: float, iyy: float, izz: float) -> str:
    return (
        f'      <inertia ixx="{ixx:.6f}" iyy="{iyy:.6f}" izz="{izz:.6f}" ixy="0" ixz="0" iyz="0"/>'
    )
