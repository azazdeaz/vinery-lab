# Straddling robot demo

A portal field robot driving every row of a generated vineyard: a leg in the
alley either side of the row, the trellis passing under its frame. The navigation 
path is derived from the USD. No sensor input is used in this demo. 

## Requirements
 - [uv](https://docs.astral.sh/uv/getting-started/installation/) installed
 - [Isaac Sim 6.1](https://docs.isaacsim.omniverse.nvidia.com/6.1.0/installation/requirements.html) compatible hardware
 - [Rust](https://www.rust-lang.org/tools/install) installed (this demo builds the scene generator from source)

## Running the demo

```bash
uv run main.py
```



https://github.com/user-attachments/assets/fd3fa3ef-3130-49ce-b59d-d5a1189af516



The first run generates the vineyard and converts the robot, and caches both;
later runs start straight up. `--help` lists the launcher's options —
`--row` picks the row it starts on, `--physics` the backend and `--viz` the
viewer.

The default backend is Newton coupled with VBD, the one solver that bends the
vineyard's flexible shoots as the frame pushes through them. Any other --
`--physics newton_mjwarp` for the robot alone at near real time, or a PhysX
backend -- spawns the stray shoots static, at the same lean.

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

The route closes on itself — down every row, then back the way it came — so
`--row` rotates where the drive begins rather than shortening it, and the
whole block is covered whichever row it starts on.

## Trimming

```bash
uv run main.py --trim
```

`--trim` hangs a hedger under the frame: an upright cutter bar either side of
the row, 0.4 m out from it, reaching from 0.3 m above the ground to the frame.
Every stray shoot that crosses a bar's plane is cut right where it crosses,
and the piece past the cut falls and lies where it lands. The vineyard it
drives has more strays than the plain demo, so there is something to trim on
every row.

That is how summer hedging is done: cutter bars on a frame over the row trim
the canopy's sides back to a plane, sickle bars at 3–4 km/h and rotary knives
at 5–6, about the pace this robot drives at. A real hedger cuts the whole
canopy face. Here the shoots the trellis holds are static meshes, so the bars
sit just outside them and cut only the flexible strays.

`Trimmer` in `straddler.py` holds the bars' reach, bottom, width and
thickness. The cut itself is
[`vinerylab.isaaclab.cutting`](../../python/vinerylab/isaaclab/cutting.py),
which any script can use on a running simulation:

```python
from vinerylab.isaaclab import Shears

shears = Shears()                           # after sim.reset()
shears.cut_through(corner, along, up)       # every rod crossing a rectangle
shears.cut(body, 0.4)                       # one rod body, 40% along it
shears.settle(ground.height)                # lay fallen pieces on the ground
```

A cut between two joints shortens the capsule it lands on and stretches the
next one back to meet it, so a shoot comes apart exactly at the knife, not at
the nearest joint. Only values in the model change, never its size, so the
cut runs under the CUDA graph that steps the simulation.

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
| `main.py` | the vineyard, the sky, the robot, the simulation loop, the trimming |
| `straddler.py` | the robot: its dimensions, the URDF built from them, the actuators, the trimmer |
| `route.py` | waypoints down every row, read off the trellis posts |
| `driver.py` | waypoint following and swerve kinematics |
| `newton_patches.py` | workarounds for Newton backend bugs |

`DEVELOPMENT.md` in `../isaaclab_demo` covers the pinned Isaac Lab revision,
which this example shares.
