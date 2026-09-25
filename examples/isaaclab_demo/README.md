# Quadruped demo

A stock ANYmal-C walking every alley of a generated vineyard, down one and
back up the next. The navigation path is derived from the USD, no sensor input
is used in this demo. 

## Requirements
 - [uv](https://docs.astral.sh/uv/getting-started/installation/) installed
 - [Isaac Sim 6.1](https://docs.isaacsim.omniverse.nvidia.com/6.1.0/installation/requirements.html) compatible hardware
 - [Rust](https://www.rust-lang.org/tools/install) installed (this demo builds the scene generator from source)

## Running the demo

```bash
uv run main.py
```

The first run generates the vineyard and caches it; later runs start straight
up. `--help` lists the launcher's options — `--physics` picks the backend and
`--viz` the viewer.

The default backend is Newton coupled with VBD, the one solver that bends the
vineyard's flexible shoots. Here they lean out over the alleys a metre or so up,
above the quadruped, which walks under them without touching one; the
[straddler demo](../straddler_demo/) is the one that pushes through them. Any other —
`--physics newton_mjwarp`, or a PhysX backend — spawns the stray shoots
static, at the same lean.

## Layout

| file | what it owns |
| --- | --- |
| `main.py` | the vineyard, the sky, the robot, the simulation loop |
| `route.py` | waypoints down every alley, read off the trellis posts |
| `driver.py` | waypoint following, the locomotion policy, getting back up |
| `newton_patches.py` | workarounds for Newton backend bugs |

`DEVELOPMENT.md` covers the pinned Isaac Lab revision.
