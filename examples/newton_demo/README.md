# Newton demo

The generated vineyard in [Newton](https://github.com/newton-physics/newton),
with no Isaac Lab and no Isaac Sim. The counterpart of `isaaclab_demo`.

```bash
uv run main.py
```

`main.py` generates the vineyard USD and loads it into a `newton.ModelBuilder`.
Nothing moves: there is no robot and no cable in that scene, so it constructs no
solver. It is the environment the rest of the directory builds on.

Generated stages are cached next to the scripts as `vineyard_<digest>.usd`,
keyed on the parameters that made them — edit `PARAMS` and the next run
generates rather than reusing a stale stage.

## Cost of the canopy

Newton flattens the vineyard's instanced canopy to one shape per leaf, so
import time scales with the canopy rather than with the physics:

| parcel | prims | shapes | import |
| --- | --- | --- | --- |
| 40 × 30 m (`main.py`) | 21k | 21k | ~15 s |
| 82 × 82 m (`isaaclab_demo`) | 205k | 186k | ~145 s |

Setting `VISUALS = False` loads only what can be collided with — the ground
mesh and a capsule per post and trunk — which is under a second at any size.

## `usd_cable_test.py`

```bash
uv run usd_cable_test.py
```

```bash
uv run usd_cable_test.py --viewer null --test --num-frames 120
```

The same vineyard with twelve of its shoots authored as deformable curves.
Newton's USD importer turns each into a cable and `SolverVBD` steps it.

### The authoring contract

A cable is a `UsdGeom.BasisCurves` that

- is open, `linear` and `nonperiodic`,
- carries the `PhysicsCurvesDeformableSimAPI` applied schema,
- binds a material with `PhysicsCurvesDeformableMaterialAPI` supplying
  `physics:thickness`, `physics:density`, `physics:stretchStiffness` and
  `physics:bendStiffness`, and
- carries `PhysicsCollisionAPI` if it should collide.

Topology comes from the curve's own `points` and `curveVertexCounts`: `N`
control points give `N-1` capsule bodies joined by `N-2` cable joints. A curve
missing any of the above still loads — as inert geometry, or as a cable with
Newton's 2.5 mm default radius — so the script asserts that the importer
recognised every curve it authored.

Isaac Lab authors the same contract through `CableCfg` / `CableMaterialCfg`
(PR [#6688](https://github.com/isaac-sim/IsaacLab/pull/6688)), so a stage that
satisfies it works on both paths.

### Things to know before scaling this up

- **VBD only.** No other Newton solver steps `JointType.CABLE`.
- **Iterations matter more than they look.** VBD softens what it cannot
  converge, so an under-iterated stiff cable reads as a limp one rather than as
  an error. These offshoots sag ~0.4 m at 5 iterations and ~0.02 m at 20.
- **One stiffness pair per cable**, derived from the *mean* segment length.
  Space the control points evenly or the outlier segments come out mistuned.
- **A cable segment is a rigid body.** A full-size vineyard has ~19k shoots; at
  8 segments each that is ~150k bodies. Cables are for a chosen few.
