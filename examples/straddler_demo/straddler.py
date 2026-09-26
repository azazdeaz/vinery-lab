"""A straddling field robot, generated at the size the vineyard asks for.

The robot is a portal: a flat roof carried on four legs with a swerve module
at the foot of each. It drives with a leg in the alley either side of a vine
row and the trellis passing under the frame, which leaves two dimensions
deciding whether it can work a block at all -- the track it stands on and the
opening it carries over the row. `Straddler.for_vineyard` takes both from the
vineyard to be worked; `dataclasses.replace` moves any of the rest.

The proportions are the commercial portals': 0.10-0.25 m of structure between
a wheel centre and the opening, tracks of 1.1-2.0 m, and openings 1.4-2.35 m
high. `PRESETS` holds four of those machines at their published figures. The
defaults are lighter and smaller-wheeled than any of them, because this robot
is only the frame. The one tool it can carry is a `Trimmer`.

Mass sits low on purpose: the drive modules carry most of it and the top frame
is light, which is both how these machines are built and the only way
something this tall stays upright on a slope.
"""

from __future__ import annotations

import dataclasses
import hashlib
import pathlib
import tempfile

import isaaclab.sim as sim_utils
from isaaclab.actuators import ImplicitActuatorCfg
from isaaclab.assets import ArticulationCfg

from vinerylab.isaaclab import VineyardCfg

CORNERS = ("front_left", "front_right", "rear_left", "rear_right")
"""Wheel modules, in the order `Straddler.corners` and the driver address them."""

STEER_JOINTS = [f"steer_{corner}" for corner in CORNERS]
DRIVE_JOINTS = [f"drive_{corner}" for corner in CORNERS]
"""The steering and drive joints, in `CORNERS` order. Nothing else moves."""

_SIGNS = {
    "front_left": (1, 1),
    "front_right": (1, -1),
    "rear_left": (-1, 1),
    "rear_right": (-1, -1),
}
"""(along, across) sign of each corner, as multiples of half the wheelbase/track."""

# -- How the mass is distributed, as fractions of the whole. A straddler's
# frame is a hollow shell and its drive modules are motors, gearboxes and the
# batteries between them, so the frame is the light end by a long way.
FRAME_SHARE = 0.2
WHEEL_SHARE = 0.15  # the four wheels together

# -- Actuation. The steering joints hold an angle, the drive joints hold a
# speed. Steering is continuous, as it is on the real machines: a module
# reaches any direction within a quarter turn (`driver.swerve`), so a stop
# would only ever be something for the crab at a headland to jam against.
STEER_STIFFNESS = 4000.0
STEER_DAMPING = 200.0
DRIVE_DAMPING = 2000.0  # with zero stiffness this is the velocity drive's strength
STEER_EFFORT_PER_KG = 0.9  # N m, enough to scrub a loaded wheel round on the spot
DRIVE_EFFORT_PER_KG = 0.5
"""Torque ceilings, per kilogram of machine: what a wheel can push is what is
standing on it. A drive this stiff with no ceiling explodes on the first step,
and a continuous joint carries no limit of its own to fall back on."""

MATERIALS = {"frame": "0.82 0.80 0.75 1", "tire": "0.05 0.05 0.05 1"}
"""Colours, as URDF rgba. Painted frame, and rubber where it touches the ground.

These are sRGB; the importer converts them to the linear values the renderer
works in, so they are picked the way a colour is picked rather than the way a
reflectance is. The frame is a broken white rather than a true one: paint that
returns nine tenths of the light falling on it does not exist, and under an
open sky a surface that bright renders as a hole with no shape in it.
"""

ROUGHNESS = {"frame": 0.55, "tire": 0.9}
"""How rough each material is. See `set_finish`.

Weathered machine enamel, and rubber -- which is rougher than almost anything
else on a vehicle.
"""


@dataclasses.dataclass(frozen=True)
class Trimmer:
    """A hedger: two upright cutter bars hung from the frame, one either side of
    the row, trimming the canopy's sides to a plane as the machine drives.

    Summer hedging is done this way, with sickle bars at 3-4 km/h or rotary
    knives at 5-6, which is the pace this robot cruises at. The bars are
    visuals only; what they cut is decided by `Straddler.bars` and
    `vinerylab.isaaclab.cutting.Shears`.
    """

    reach: float = 0.4
    """Row line to each bar: the half width the canopy is trimmed to.

    Clear of the shoots the trellis holds, which reach about 0.3 m out either
    side and are static, so only the strays -- the flexible ones -- cross it.
    """

    bottom: float = 0.3
    """The bars' lower ends, above the ground. Their tops are at the frame."""

    width: float = 0.08
    """Along the row. The knife sweeps this much of its plane at each check,
    so it has to be wider than the machine drives in a control step."""

    thickness: float = 0.03


@dataclasses.dataclass(frozen=True)
class Straddler:
    """One machine, in metres and kilograms.

    The defaults are a light electric portal on a 1.2 m track. Every field is
    free; `clear_width`, `clear_height` and `track` are the three a grower
    would be quoted, and the rest follow the machine's construction.
    """

    track: float = 1.2
    """Wheel centre to wheel centre, across the row."""

    clear_height: float = 1.7
    """The underside of the top frame, and so the opening's height."""

    wheelbase: float = 1.6
    """Front axle to rear axle. Roughly half the headland the machine needs."""

    leg_thickness: float = 0.12
    """Structure between a wheel centre and the opening, per side.

    The leg is a square post of twice this, standing on the wheel centre line,
    so it is also what each side costs the opening: 0.10 m on a Naio Ted and
    0.25 m on a VitiBot Bakus.
    """

    wheel_radius: float = 0.2
    wheel_width: float = 0.15
    roof_thickness: float = 0.12
    """The slab across the top. Its corners sit on the legs', so nothing overhangs."""

    mass: float = 160.0
    """The whole machine, and what the actuators are sized from.

    A bare frame on wheels is this light; the fleet's working machines, tanks
    and tools and all, run 500 kg to 2 400 kg.
    """

    max_speed: float = 2.4
    """m/s on the ground, flat out."""

    trimmer: Trimmer | None = None
    """What hangs under the frame, if anything."""

    def __post_init__(self):
        if self.clear_width <= 0 or self.clear_height <= self.wheel_radius:
            raise ValueError(f"{self} leaves no opening over the row")
        if self.trimmer and self.trimmer.reach + self.trimmer.thickness / 2 >= self.clear_width / 2:
            raise ValueError(f"{self.trimmer} does not fit between the legs")

    @classmethod
    def for_vineyard(cls, vineyard: VineyardCfg, clearance: float = 0.2) -> Straddler:
        """The machine that straddles `vineyard`'s rows.

        Half a row spacing of track puts a leg in each alley, halfway between
        the vines it straddles and the next row's, and the frame clears the top
        wire by `clearance` -- 0.2 to 0.5 m across the fleet.
        """
        return cls(
            track=vineyard.parcel.row_spacing / 2,
            clear_height=vineyard.parcel.trellis_height + clearance,
        )

    @property
    def clear_width(self) -> float:
        """The opening over the row, between the inner faces of the two legs."""
        return self.track - 2 * self.leg_thickness

    @property
    def corners(self) -> list[tuple[float, float]]:
        """Each wheel's `(x, y)` in the base frame, in `CORNERS` order."""
        return [
            (along * self.wheelbase / 2, across * self.track / 2)
            for along, across in map(_SIGNS.get, CORNERS)
        ]

    @property
    def max_wheel_speed(self) -> float:
        """rad/s at `max_speed`."""
        return self.max_speed / self.wheel_radius

    @property
    def frame_mass(self) -> float:
        return FRAME_SHARE * self.mass

    @property
    def wheel_mass(self) -> float:
        return WHEEL_SHARE * self.mass / 4

    @property
    def steer_effort(self) -> float:
        """N m a steering joint may exert. See `STEER_EFFORT_PER_KG`."""
        return STEER_EFFORT_PER_KG * self.mass

    @property
    def drive_effort(self) -> float:
        """N m a drive joint may exert. See `DRIVE_EFFORT_PER_KG`."""
        return DRIVE_EFFORT_PER_KG * self.mass

    @property
    def module_mass(self) -> float:
        """One drive module: the motor, its gearbox and its share of the batteries."""
        return (1 - FRAME_SHARE - WHEEL_SHARE) * self.mass / 4

    @property
    def bars(self) -> list[tuple[tuple[float, ...], ...]]:
        """The trimmer's cutter bars, in the base frame, as the plane each
        knife sweeps: a corner and the two edges from it, along the row and up.
        Empty without a trimmer.

        Midway along the wheelbase, so the bars lead neither way: the machine
        drives alternate rows in reverse.
        """
        if self.trimmer is None:
            return []
        t = self.trimmer
        return [
            (
                (-t.width / 2, side * t.reach, t.bottom),
                (t.width, 0.0, 0.0),
                (0.0, 0.0, self.clear_height - t.bottom),
            )
            for side in (1, -1)
        ]


def _commercial(**published) -> Straddler:
    """A working machine: the figures its maker gives, on the 0.86 m wheel
    (a 320/65 R16) the fleet has standardised on."""
    return Straddler(wheel_radius=0.43, wheel_width=0.32, **published)


PRESETS = {
    # Track, opening and mass as published; the dimensions their makers do not
    # publish are the defaults above, with the wheel `_commercial` puts on.
    "bakus_s": _commercial(track=1.10, leg_thickness=0.25, clear_height=1.75, mass=2050.0),
    "bakus_l": _commercial(track=1.30, leg_thickness=0.25, clear_height=2.20, mass=2400.0),
    "ted_180": _commercial(track=1.35, leg_thickness=0.10, clear_height=1.77, mass=1700.0),
    "ted_235": _commercial(track=1.80, leg_thickness=0.10, clear_height=2.35, mass=1700.0),
}
"""Machines that exist, at the figures their makers quote.

They are here to be driven, and to be checked against: an opening that does
not come out at the published width and height means the frame below is built
wrong.
"""


def straddler_cfg(machine: Straddler, prim_path: str) -> ArticulationCfg:
    """An articulation spawning `machine`, converting its URDF on first use."""
    return ArticulationCfg(
        prim_path=prim_path,
        spawn=sim_utils.UrdfFileCfg(
            asset_path=_write_urdf(machine),
            usd_dir=str(_CACHE),
            fix_base=False,
            # The frame and legs are boxes and the wheels cylinders; a hull of
            # each is the shape itself, and a decomposition would only be slower.
            collision_type="Convex Hull",
            joint_drive=None,  # the actuators below own the gains
            activate_contact_sensors=False,
        ),
        actuators={
            "steering": ImplicitActuatorCfg(
                joint_names_expr=STEER_JOINTS,
                stiffness=STEER_STIFFNESS,
                damping=STEER_DAMPING,
                effort_limit=machine.steer_effort,
            ),
            # Zero stiffness makes these velocity drives: the target is a speed
            # and `damping` is how hard they hold it.
            "drive": ImplicitActuatorCfg(
                joint_names_expr=DRIVE_JOINTS,
                stiffness=0.0,
                damping=DRIVE_DAMPING,
                effort_limit=machine.drive_effort,
            ),
        },
    )


def set_finish(prim_path: str) -> None:
    """Give a spawned machine's materials their surface response.

    A URDF material carries a colour and nothing else, so every material the
    importer builds from one comes out at its default roughness of 0.5 -- a
    half gloss that reads as wet plastic on paint and as polished rubber on a
    tyre. The importer writes each as a `UsdPreviewSurface` with its roughness
    exposed on the material prim, which is where this sets it.

    Call once the articulation is built, since that is what spawns the asset.
    """
    stage = sim_utils.get_current_stage()
    for name, roughness in ROUGHNESS.items():
        material = stage.GetPrimAtPath(f"{prim_path}/Materials/{name}")
        if not material.IsValid():
            raise RuntimeError(
                f"{prim_path} has no material {name!r} to finish:"
                " the URDF importer no longer lays materials out where this expects them"
            )
        material.GetAttribute("inputs:roughness").Set(roughness)


##
# URDF generation.
##

_CACHE = pathlib.Path(tempfile.gettempdir()) / "straddler_demo"
"""Where a generated robot is kept.

Named for its contents, since two sizes are two robots. The URDF converter
caches on that name too, so a machine it has already imported spawns without
re-running the importer -- which needs Kit, and so would tie a kitless run to
whether the robot had been built before.
"""


def _write_urdf(machine: Straddler) -> str:
    """Build the URDF for one machine and return its path, writing it once."""
    text = urdf(machine)
    _CACHE.mkdir(parents=True, exist_ok=True)
    path = _CACHE / f"straddler_{hashlib.sha256(text.encode()).hexdigest()[:16]}.urdf"
    path.write_text(text)
    return str(path)


def urdf(machine: Straddler) -> str:
    """`machine` as a URDF document.

    `base_link` is the whole frame -- the roof and the legs both, since nothing
    between them moves -- with its origin on the ground at the centre of the
    wheelbase. Each wheel then hangs off it through a steering joint about the
    vertical and a drive joint about the wheel's own axis, the two crossing at
    the wheel centre as they do on a swerve module.
    """
    leg = 2 * machine.leg_thickness
    leg_height = machine.clear_height - machine.wheel_radius
    # One slab over the four legs, flush with their outer faces.
    length, width = machine.wheelbase + leg, machine.track + leg
    frame = "\n".join(
        [
            _box(
                f"0 0 {machine.clear_height + machine.roof_thickness / 2}",
                f"{length} {width} {machine.roof_thickness}",
            ),
            *(
                _box(
                    f"{x} {y} {machine.wheel_radius + leg_height / 2}",
                    f"{leg} {leg} {leg_height}",
                )
                for x, y in machine.corners
            ),
            # A cutter bar over the plane its knife sweeps. Drawn only: a
            # collider would push the shoots aside before they could be cut.
            *(
                _box(
                    f"{x + along[0] / 2} {y} {z + up[2] / 2}",
                    f"{along[0]} {machine.trimmer.thickness} {up[2]}",
                    collide=False,
                )
                for (x, y, z), along, up in machine.bars
            ),
        ]
    )
    modules = "\n".join(
        _module(machine, steer, drive, corner, x, y)
        for corner, steer, drive, (x, y) in zip(
            CORNERS, STEER_JOINTS, DRIVE_JOINTS, machine.corners
        )
    )
    # The frame's inertia is a box the size of its bounding volume: it is a
    # hollow shell, so anything finer would be guesswork at the same order.
    return f"""<?xml version="1.0"?>
<!-- Generated by straddler.py; edit that, not this. -->
<robot name="straddler">
  <link name="base_link">
{frame}
    <inertial>
      <origin xyz="0 0 {machine.clear_height}"/>
      <mass value="{machine.frame_mass}"/>
{_box_inertia(machine.frame_mass, length, width, machine.roof_thickness)}
    </inertial>
  </link>
{modules}
</robot>
"""


def _module(machine: Straddler, steer: str, drive: str, corner: str, x: float, y: float) -> str:
    """One swerve module: a steering joint, a stub link, and the wheel on it."""
    stub = machine.wheel_width
    return f"""
  <joint name="{steer}" type="continuous">
    <parent link="base_link"/>
    <child link="steer_{corner}_link"/>
    <origin xyz="{x} {y} {machine.wheel_radius}"/>
    <axis xyz="0 0 1"/>
    <limit effort="{machine.steer_effort}" velocity="6"/>
  </joint>
  <link name="steer_{corner}_link">
{_box("0 0 0", f"{stub} {stub} {stub}", collide=False)}
    <inertial>
      <mass value="{machine.module_mass}"/>
{_box_inertia(machine.module_mass, stub, stub, stub)}
    </inertial>
  </link>

  <joint name="{drive}" type="continuous">
    <parent link="steer_{corner}_link"/>
    <child link="wheel_{corner}"/>
    <axis xyz="0 1 0"/>
    <limit effort="{machine.drive_effort}" velocity="{machine.max_wheel_speed}"/>
  </joint>
  <link name="wheel_{corner}">
{_cylinder(machine)}
    <inertial>
      <origin rpy="1.5707963 0 0"/>
      <mass value="{machine.wheel_mass}"/>
{_cylinder_inertia(machine.wheel_mass, machine.wheel_radius, machine.wheel_width)}
    </inertial>
  </link>"""


def _box(xyz: str, size: str, collide: bool = True) -> str:
    """A box as both a visual and, unless told otherwise, a collider."""
    shape = f'      <origin xyz="{xyz}"/>\n      <geometry><box size="{size}"/></geometry>'
    tags = ("visual", "collision") if collide else ("visual",)
    return _shaped(shape, tags, "frame")


def _cylinder(machine: Straddler) -> str:
    """The wheel, laid on its side so it spins about the joint's +y."""
    shape = (
        '      <origin rpy="1.5707963 0 0"/>\n'
        f'      <geometry><cylinder radius="{machine.wheel_radius}" '
        f'length="{machine.wheel_width}"/></geometry>'
    )
    return _shaped(shape, ("visual", "collision"), "tire")


def _shaped(shape: str, tags: tuple[str, ...], material: str) -> str:
    """One geometry repeated as each of `tags`, coloured where it is a visual."""
    colour = f'\n      <material name="{material}"><color rgba="{MATERIALS[material]}"/></material>'
    return "\n".join(
        f"    <{tag}>\n{shape}{colour if tag == 'visual' else ''}\n    </{tag}>" for tag in tags
    )


def _box_inertia(mass: float, x: float, y: float, z: float) -> str:
    """The inertia tensor of a solid box, as a URDF `<inertia>` element."""
    return _inertia(
        mass * (y * y + z * z) / 12, mass * (x * x + z * z) / 12, mass * (x * x + y * y) / 12
    )


def _cylinder_inertia(mass: float, radius: float, length: float) -> str:
    """The inertia tensor of a solid cylinder about its own axes (`z` is the axis)."""
    across = mass * (3 * radius * radius + length * length) / 12
    return _inertia(across, across, mass * radius * radius / 2)


def _inertia(ixx: float, iyy: float, izz: float) -> str:
    return (
        f'      <inertia ixx="{ixx:.6f}" iyy="{iyy:.6f}" izz="{izz:.6f}" ixy="0" ixz="0" iyz="0"/>'
    )
