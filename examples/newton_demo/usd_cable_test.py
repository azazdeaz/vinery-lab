"""Vineyard offshoots as Newton VBD cables.

`main.py` loads the vineyard; this authors a handful of its shoots as
deformable curves on top, and Newton's USD importer turns each curve into a
cable: one capsule body per segment, joined by `JointType.CABLE` joints and
stepped by `SolverVBD`.

    uv run usd_cable_test.py                              # interactive viewer
    uv run usd_cable_test.py --viewer null --num-frames 100

Only the vineyard's colliders are imported -- the ground mesh, and a capsule
per post and trunk. The canopy is visual geometry and is skipped, so what the
viewer draws is what the solver sees.

Cables are why this directory needs Newton 1.5 or newer. Cable *physics* has
been in Newton since 1.2, but parsing a `BasisCurves` prim into one arrived
with the deformable importer in 1.5; Isaac Lab's own cable support (PR #6688)
is on `develop`, against a newer Newton than the 1.2.1 its released wheels pin.
"""

from __future__ import annotations

import numpy as np
import warp as wp
from pxr import Sdf, Usd, UsdGeom, UsdPhysics, UsdShade, Vt

import newton
import newton.examples
import vinerylab
from main import vineyard_usd

# A small parcel on purpose: every offshoot is simulated, and the point here is
# the cables rather than the vineyard around them. The defaults leave no room
# for a row this size, hence the shorter headland and minimum row.
PARAMS = vinerylab.VineyardParams(
    terrain=vinerylab.TerrainParams(width=20.0, height=14.0, max_elevation=0.6),
    parcel=vinerylab.ParcelParams(headland=3.0, min_row_length=6.0),
)

OFFSHOOTS = 12
"""How many of the scene's shoots become cables, spread evenly over the rows."""

SEGMENTS = 8
"""Capsule bodies per cable. `SEGMENTS + 1` control points."""

LENGTH = 0.75
"""Cable length [m]. Matches `ShootParams.length`, whose default this leaves."""

THICKNESS = 0.012
"""Cable diameter [m]: twice `ShootParams.radius`, which is a radius."""

# Green-wood moduli [Pa]. Newton derives the per-joint stiffnesses from these
# and the segment geometry -- stretch as `E*A/L`, bend as `E*I/L` -- so both
# scale with thickness to the second and fourth power. Drop `bendStiffness` to
# make an offshoot droop.
MATERIAL = {
    "thickness": THICKNESS,
    "density": 700.0,
    "stretchStiffness": 1.0e9,
    "bendStiffness": 1.0e9,
}


def author_cables(stage: Usd.Stage) -> int:
    """Author `OFFSHOOTS` of the scene's shoots as deformable curves, and count them.

    Newton reads a cable as an open, linear, nonperiodic `UsdGeom.BasisCurves`
    carrying `PhysicsCurvesDeformableSimAPI` and bound to a material with the
    curve moduli in the `physics:` namespace. Nothing else is required --
    topology comes from the curve's own `points` and `curveVertexCounts`.

    A shoot's local +Z is its growth axis, so the curve runs straight up it and
    the shoot's own frame supplies the bearing and lean the generator drew.
    """
    material = _author_material(stage)
    points = Vt.Vec3fArray([(0.0, 0.0, z) for z in np.linspace(0.0, LENGTH, SEGMENTS + 1)])

    shoots = [prim for prim in stage.Traverse() if prim.GetName().startswith("Shoot_")]
    if not shoots:
        raise RuntimeError("the generated stage has no shoot prims to hang cables on")
    chosen = shoots[:: max(1, len(shoots) // OFFSHOOTS)][:OFFSHOOTS]
    for shoot in chosen:
        curve = UsdGeom.BasisCurves.Define(stage, shoot.GetPath().AppendChild("Cable"))
        curve.CreateTypeAttr(UsdGeom.Tokens.linear)
        curve.CreateWrapAttr(UsdGeom.Tokens.nonperiodic)
        curve.CreatePointsAttr(points)
        curve.CreateCurveVertexCountsAttr([len(points)])
        curve.CreateWidthsAttr([THICKNESS])
        curve.SetWidthsInterpolation(UsdGeom.Tokens.constant)

        prim = curve.GetPrim()
        prim.AddAppliedSchema("PhysicsCurvesDeformableSimAPI")
        # Opt in to collision. The importer filters adjacent segments only, so a
        # cable still collides with the ground, the trunks and other cables.
        UsdPhysics.CollisionAPI.Apply(prim)
        UsdShade.MaterialBindingAPI.Apply(prim).Bind(material, materialPurpose="physics")
    return len(chosen)


def _author_material(stage: Usd.Stage) -> UsdShade.Material:
    """The one deformable-curve material every cable binds."""
    material = UsdShade.Material.Define(stage, Sdf.Path("/Vineyard/CableMaterial"))
    prim = material.GetPrim()
    prim.AddAppliedSchema("PhysicsCurvesDeformableMaterialAPI")
    for name, value in MATERIAL.items():
        prim.CreateAttribute(f"physics:{name}", Sdf.ValueTypeNames.Float).Set(value)
    return material


def pin_cable_roots(builder: newton.ModelBuilder, cable_map: dict) -> None:
    """Fix each cable's first segment to the vine it grew out of.

    An imported cable is free-floating. Zeroing the root segment's mass makes it
    kinematic, which is enough to hang the offshoot where it was authored -- and
    cheaper than the `PhysicsAttachment` route, which lowers to a ball joint and
    so would let the base pivot anyway.
    """
    for bodies, _joints in cable_map.values():
        root = bodies[0]
        builder.body_mass[root] = 0.0
        builder.body_inv_mass[root] = 0.0
        builder.body_inertia[root] = wp.mat33(0.0)
        builder.body_inv_inertia[root] = wp.mat33(0.0)


class Example:
    def __init__(self, viewer, args):
        self.viewer = viewer
        self.fps = 60
        self.frame_dt = 1.0 / self.fps
        self.sim_substeps = 8
        self.sim_dt = self.frame_dt / self.sim_substeps
        self.sim_time = 0.0

        stage = Usd.Stage.Open(str(vineyard_usd(PARAMS)))
        authored = author_cables(stage)

        builder = newton.ModelBuilder()
        info = builder.add_usd(
            stage,
            # The canopy is visual only, and at full parcel size it is ~200k
            # prims Newton would otherwise carry through the whole model.
            load_visual_shapes=False,
            load_static_visual_shapes=False,
            return_deformable_results=True,
        )
        self.cables = info["path_cable_map"]
        # A curve the importer does not recognise as a cable is skipped with a
        # warning, leaving a scene that loads and simulates with no cables in
        # it. Fail here instead: this example is the authoring contract's test.
        if len(self.cables) != authored:
            raise RuntimeError(f"authored {authored} cable curves, the importer recognised {len(self.cables)}")
        pin_cable_roots(builder, self.cables)
        builder.color()  # VBD steps a coloured graph.

        self.model = builder.finalize()
        self.collision_pipeline = newton.CollisionPipeline(self.model)
        # A cable joint this stiff needs the iterations: VBD softens what it
        # cannot converge, so too few reads as a limp shoot rather than as an
        # error. At 5 the tip of an offshoot sags ~0.4 m, at 20 about 0.02 m.
        self.solver = newton.solvers.SolverVBD(self.model, iterations=20)
        self.state_0 = self.model.state()
        self.state_1 = self.model.state()
        self.control = self.model.control()
        self.contacts = self.collision_pipeline.contacts()
        body_q = self.state_0.body_q.numpy()
        self.spawn_roots = {path: body_q[bodies[0], :3].copy() for path, (bodies, _) in self.cables.items()}

        self.viewer.set_model(self.model)

    def step(self):
        for _ in range(self.sim_substeps):
            self.state_0.clear_forces()
            self.collision_pipeline.collide(self.state_0, self.contacts)
            self.solver.step(self.state_0, self.state_1, self.control, self.contacts, self.sim_dt)
            self.state_0, self.state_1 = self.state_1, self.state_0
        self.sim_time += self.frame_dt

    def render(self):
        self.viewer.begin_frame(self.sim_time)
        self.viewer.log_state(self.state_0)
        self.viewer.end_frame()

    def test_final(self):
        """`--test` asserts the cables still hang the way they were authored.

            uv run main.py --viewer null --test --num-frames 120
        """
        body_q = self.state_0.body_q.numpy()
        assert np.isfinite(body_q).all(), "non-finite body poses"

        rest = LENGTH / SEGMENTS
        for path, (bodies, _joints) in self.cables.items():
            positions = body_q[bodies, :3]
            spacing = np.linalg.norm(np.diff(positions, axis=0), axis=1)
            assert np.allclose(spacing, rest, rtol=0.1), f"{path} came apart: {spacing}"
            # The root segment is kinematic, so the offshoot stays on its vine.
            assert np.linalg.norm(positions[0] - self.spawn_roots[path]) < 1.0e-6, f"{path} root moved"


if __name__ == "__main__":
    viewer, args = newton.examples.init()
    newton.examples.run(Example(viewer, args), args)
