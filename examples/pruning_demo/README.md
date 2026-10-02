# Dormant pruning demo

A Bumblebee-like robot pruning a row of a generated winter vineyard: a
skid-steer UGV drives the alley, stops at each vine, and an arm on a linear
slide cuts every cane back to two buds with a bypass shear. The cuts are
planned from the scene's own ground truth -- which cane, where each bud is --
rather than from a sensor.

## Requirements
 - [uv](https://docs.astral.sh/uv/getting-started/installation/) installed
 - [Isaac Sim 6.1](https://docs.isaacsim.omniverse.nvidia.com/6.1.0/installation/requirements.html) compatible hardware
 - [Rust](https://www.rust-lang.org/tools/install) installed (this demo builds the scene generator from source)

## Running the demo

```bash
uv run main.py
```

The first run generates the vineyard and converts the robot, and caches both;
later runs start straight up. `--help` lists the launcher's options --
`--row` picks the row pruned, `--physics` the backend and `--viz` the viewer.

Only the default backend, Newton coupled with VBD, bends a cane and so can cut
one. Under any other the canes spawn static and the robot drives the row with
nothing to do.

## Teleoperation

```bash
uv run main.py --teleop
```

The planner stands down and the keyboard has the robot, with the Kit window
focused: the left hand drives, the right hand cuts.

| key | what it does |
| --- | --- |
| `W` / `S` | drive forward / back |
| `A` / `D` | turn left / right |
| `↑` / `↓` | jog the mouth forward / back along the robot |
| `←` / `→` | jog it toward the row / away from it |
| `Page Up` / `Page Down` | jog it up / down |
| `Enter` | close the shear, cutting what the blade sweeps through, and open it again; the robot holds still while it shuts |

The mouth is a gantry head rather than a wrist: it stays squared up to the
row -- blades out over the rail, pivot upright, so an upright cane lies across
it -- and its position is held in the robot's own frame, so it rides along
when the base drives. It starts out over the rail at spur height, a stand-off
short of the row, and a jog the arm cannot reach is refused.

## What it shows

The layout is [Bumblebee's](https://arxiv.org/abs/2112.00291) (Silwal et al.,
2021): a Clearpath Warthog in the alley, a UR5 on a 1.35 m slide across its
deck, and a bypass shear in its hand. So is the pipeline, minus the cameras:

1. **The rule.** Keep two buds on every cane and cut midway between the second
   and the third. The scene is generated dormant, so every cane is a rod a
   solver bends, with a `Bud_NN` prim at every node; the planner reads the buds
   off the stage and their live positions off the rod bodies carrying them.
2. **The approach.** Each cut is a pose for the shear's mouth, squared up to
   the cane: the pivot along it, the blades along the approach, aimed afresh
   at the buds when the cut's turn comes. The arm plans to a point 15 cm out,
   then closes in on a straight line and settles there. The shear's head and
   blades collide with the canes, so a cane the mouth comes in on is
   funnelled between the edges or pushed aside, bending as it goes.
3. **The order.** The cuts on a vine are taken nearest neighbour first.
4. **The cut.** Once the arm has settled, the shear closes, and the moving
   blade carries the cane across the mouth onto the fixed one. Held there, the
   blade's collider stands still while the blade itself closes on through:
   every control tick its plate, at the angle the blade has reached, goes to
   `Shears.cut_through`, and the cane is cut where the edge reaches its axis.
   Nothing is crushed between the blades, nothing the cane leans on is pulled
   from under it, and the collider rides the blade again once it has opened
   back past where it stood. A cane the blade never reaches, pushed out of
   the mouth on the way in or never in it, is a miss.
5. **The fall.** The piece meets its own vine -- the stub, the other canes,
   the cordon -- and the ground, and passes through everything else: it
   catches on the stub for a moment, tips, and slides down beside it.

The score is printed after every vine, and against the paper's at the end:
cuts reachable, cuts made, cuts made at the right place -- the kept bud still
on the vine and the next one gone -- and seconds per vine. Bumblebee reports
87% of cuts made at the right place, 213 s per vine and 68% of canes reachable
from one side. On the default scene the demo reaches all 72 cuts of its row
and makes every one at the right place, at 15 s of simulated time per vine.
Because the simulation knows the true plant, the score keeps "chose the right
cut" apart from "made the cut", which a field trial cannot.

## The robot

`Bumblebee` in `bumblebee.py` is a dataclass of dimensions, and the URDF is
generated from it. The chassis is a Warthog at its published size and mass, the
slide is the paper's, and the arm has the UR5's link offsets, so its reach is a
UR5's. `Bumblebee.chain` is the same joints for `kinematics.py`, which poses
the arm with damped least squares; the solver and the simulation come from one
description.

The geometry is primitives, so there is no manufacturer's description in here
to be bound by. Isaac Sim's asset library carries a UR5e and a Ridgeback with a
UR5 on it, but no Warthog, and an arm asset cannot be bolted onto a generated
base without merging two articulations into one; a generated arm was the
shorter road.

## Layout

| file | what it owns |
| --- | --- |
| `main.py` | the vineyard, the sky, the robot, the simulation loop |
| `bumblebee.py` | the robot: its dimensions, the URDF built from them, the actuators, the shear |
| `kinematics.py` | forward and inverse kinematics of the generated chain |
| `pruner.py` | the ground truth off the stage, the rule, the cut sequence, the score |
| `driver.py` | driving the skid steer from stop to stop |
| `teleop.py` | the keyboard driving the base, jogging the mouth and working the shear |

`DEVELOPMENT.md` in `../isaaclab_demo` covers the pinned Isaac Lab revision,
which this example shares.
