"""Workarounds for Newton backend bugs. Each should go when upstream fixes it."""

import sys

import numpy as np


def fix_heightfield_offsets() -> None:
    """Lift every height field collider back to the terrain it was built from.

    Newton places a height field geom at the terrain's lowest point (the
    normalized elevation grid measures up from there), then its runtime
    property sync rewrites ``geom_pos`` from the Newton shape transform, which
    carries no such offset -- ``update_geom_properties_kernel`` reapplies the
    recentering of a mesh geom but knows nothing about a height field. The
    collider ends up that far below its own surface and everything standing on
    it sinks by the same amount. Only the MuJoCo CPU model keeps the offset, so
    copy it back from there.

    Unfiled upstream as of newton 1.6.0.dev0; the nearest report,
    newton-physics/newton#3897, is a different path (SolverXPBD, fixed).

    Call once the solver exists, i.e. after `SimulationContext.reset()`.
    """
    physics = sys.modules.get("isaaclab_newton.physics")  # unimported unless Newton is in play
    solver = physics.NewtonManager._solver if physics is not None else None
    if (model := getattr(solver, "mjw_model", None)) is None:
        return

    import mujoco

    fields = np.flatnonzero(solver.mj_model.geom_type == mujoco.mjtGeom.mjGEOM_HFIELD)
    if fields.size == 0:
        return
    # The derived pose too: mujoco_warp leaves world-body geoms out of forward
    # kinematics, so nothing recomputes it from the position we just wrote.
    for array in (model.geom_pos, solver.mjw_data.geom_xpos):
        values = array.numpy()
        values[:, fields] = solver.mj_model.geom_pos[fields]
        array.assign(values)
