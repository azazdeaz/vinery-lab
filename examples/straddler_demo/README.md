# Straddling robot demo

A portal field robot driving every row of a generated vineyard: a leg in the
alley either side of the row, the trellis passing under its frame.

```bash
uv run main.py
```

The first run generates the vineyard and converts the robot, and caches both;
later runs start straight up. `--help` lists the launcher's options —
`--physics` picks the backend and `--viz` the viewer.

The default backend is `newton_flexible`, the coupled solver that bends the
vineyard's flexible shoots, and it is the only one that runs this scene as
configured: the others build a flexible shoot as a chain of rigid bodies, and
`newton_mjwarp` fails to assemble a model with those in it. Set
`ShootCfg(stray=0.0)` in `main.py` before reaching for another backend.

## What it shows

The robot has four swerve modules — a steering joint about the vertical and a
drive joint through the wheel centre — which means it never has to turn
around. It holds the row heading for the whole run, drives alternate rows in
reverse, and crabs sideways across the headland to line up on the next row.
That is what the machine is for: a gantry that turned round would have to put
its legs somewhere, and either side of it is a vine.

The route is read off the generated scene's own trellis posts, so it re-solves
whenever the vineyard parameters change. So does the robot: `for_vineyard`
takes the track from the row spacing and the frame height from the trellis
height, and the robot is built at that size.

## The robot

`Straddler` is a dataclass of dimensions, and `straddler.py` generates a URDF
from it, so every one of them moves:

```python
Straddler(track=1.80, clear_height=2.35, leg_thickness=0.10, mass=1700.0)
Straddler.for_vineyard(VINEYARD_CFG, clearance=0.3)
dataclasses.replace(PRESETS["bakus_s"], mass=900.0)
```

The defaults are a bare frame on wheels — a 1.2 m track, 0.4 m wheels, 160 kg
— and the range each dimension is worth moving inside is the commercial
portals': 1.1–2.0 m of track, openings 1.4–2.35 m high, 0.10–0.25 m of leg
between a wheel centre and the opening, 0.86 m wheels, and 500–2400 kg with a
tank and tools aboard. `PRESETS` holds four of those machines at their
published figures — VitiBot Bakus S and L, Naïo Ted 180 and 235 — to drive,
and to check the generator against: an opening that stops coming out at the
published width means the frame below it is built wrong.

The geometry is primitives — boxes for the frame and the legs, cylinders for
the wheels — so there is no manufacturer's description in here to be bound by.

## Layout

| file | what it owns |
| —- | —- |
| `main.py` | the vineyard, the sky, the robot, the simulation loop |
| `straddler.py` | the robot: its dimensions, the URDF built from them, the actuators |
| `route.py` | waypoints down every row, read off the trellis posts |
| `driver.py` | waypoint following and swerve kinematics |
| `newton_patches.py` | workarounds for Newton backend bugs |

`DEVELOPMENT.md` in `../isaaclab_demo` covers the pinned Isaac Lab revision,
which this example shares.
