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

## What it shows

The layout is [Bumblebee's](https://arxiv.org/abs/2112.00291) (Silwal et al.,
2021): a Clearpath Warthog in the alley, a UR5 on a 1.35 m slide across its
deck, and a bypass shear in its hand. So is the pipeline, minus the cameras:

1. **The rule.** Keep two buds on every cane and cut midway between the second
   and the third. The scene is generated dormant, so every cane is a rod a
   solver bends, with a `Bud_NN` prim at every node; the planner reads the buds
   off the stage and their live positions off the rod bodies carrying them.
2. **The approach.** Each cut is a pose for the shear's mouth, squared up to
   the cane: the pivot along it, the blades along the approach. The arm plans
   to a point 15 cm out, then closes in on a straight line. The shear's head
   and blades collide with the canes, so a cane the mouth comes in on is
   funnelled between the edges or pushed aside, bending as it goes.
3. **The order.** The cuts on a vine are taken nearest neighbour first.
4. **The cut.** The shear closes over half a second, and the moving blade
   cuts what it sweeps through: every control tick its plate, at the angle the
   blade has reached, goes to `Shears.cut_through`, and a cane is cut where the
   edge reaches it. Each plate's last centimetre before the edge -- the part
   that is in the wood -- collides with nothing, so the closing blades push the
   two pieces apart like a wedge rather than crushing them. A cane the blade
   never reaches, pushed out of the mouth on the way in or never in it, is a
   miss.

The score is printed after every vine, and against the paper's at the end:
cuts reachable, cuts made, cuts made at the right place -- the kept bud still
on the vine and the next one gone -- and seconds per vine. Bumblebee reports
87% of cuts made at the right place, 213 s per vine and 68% of canes reachable
from one side. On the default scene the demo reaches all 72 cuts of its row
and makes 94% of them at the right place, at 12 s of simulated time per vine;
a miss is a cane the shear pushed out of the mouth on the way in. Because the
simulation knows the true plant, the score keeps "chose the right cut" apart
from "made the cut", which a field trial cannot.

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

`DEVELOPMENT.md` in `../isaaclab_demo` covers the pinned Isaac Lab revision,
which this example shares.
