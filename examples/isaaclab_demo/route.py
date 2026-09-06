"""The route a robot drives through a generated vineyard.

The waypoints come out of the generated scene itself -- the trellis posts --
so the route re-solves whenever the vineyard parameters change, with nothing
to keep in sync by hand.

Import only after the Isaac Sim app has been launched.
"""

from __future__ import annotations

import numpy as np
from pxr import Usd, UsdGeom

from vinerylab.isaaclab import VineyardCfg
from vinerylab.isaaclab.vineyard import resolve_usd_path
from vinerylab.usd import ROOT

STRIDE = 1.0  # m between waypoints along an alley
RUNOUT = 3.0  # m an alley carries on past its rows' end posts, into the headland


def alley_route(cfg: VineyardCfg) -> np.ndarray:
    """Waypoints down every alley of `cfg`'s vineyard, in scene coordinates.

    Read off the trellis posts of the cached scene. Every row is parallel to
    every other by construction, so the first row's posts give an AB line the
    whole block is measured against: `along` metres up the line and `across`
    metres off it. A row is then an interval of `along` at a fixed `across`,
    and the alley beside it is the midline between it and the next row.

    Rows are clipped to the parcel one at a time, so their ends are staggered
    and an alley's two rows rarely start or end together. Each alley spans the
    union of the two, plus a run-out past both ends: a headland turn out of an
    alley clears the end post of whichever of its two rows reaches further,
    which is the row the turn goes around half the time.

    Alleys are walked in alternating directions, so the result is one
    continuous path with no drive back to the start between them.

    Waypoint heights come from the posts of the two rows the alley runs
    between rather than the terrain under the alley itself. Only the spawn
    height reads them, and the two are centimeters apart on any terrain this
    generator produces.
    """
    stage = Usd.Stage.Open(resolve_usd_path(cfg))
    rows = [
        np.array([
            UsdGeom.Xformable(post).ComputeLocalToWorldTransform(Usd.TimeCode.Default()).ExtractTranslation()
            for post in row.GetChildren()
            if post.GetName().startswith("Pole_")
        ])
        for row in stage.GetPrimAtPath(f"{ROOT}/Planting").GetChildren()
    ]

    # The AB line: `a` its origin and `ab` its unit direction. Posts are
    # authored from one end of a row to the other, so the first row's own end
    # posts define it, and every pass below is an offset of it.
    a = rows[0][0, :2]
    ab = rows[0][-1, :2] - a
    ab /= np.linalg.norm(ab)

    route = []
    for index, (row, neighbour) in enumerate(zip(rows, rows[1:])):
        alley = _paved(row, neighbour, a, ab)
        route.append(alley if index % 2 == 0 else alley[::-1])
    return np.concatenate(route)


def _paved(row: np.ndarray, neighbour: np.ndarray, a: np.ndarray, ab: np.ndarray) -> np.ndarray:
    """The alley between `row` and `neighbour` as evenly spaced waypoints.

    Both rows are straight by construction, so sampling their midline loses
    nothing but the centimeter of wobble each post was driven with. Two things
    this buys the follower:

    * waypoints a stride apart rather than a post-spacing apart, so steering at
      the next one keeps the robot on the alley instead of cutting the corner
      to a point six metres away -- and an alley is only a row spacing wide.
    * a run-out into the headland past both rows, so the turn between two
      alleys happens clear of the vines rather than inside the last panel.
    """
    across = np.array([-ab[1], ab[0]])
    rows = (row, neighbour)
    # Where each row's posts sit along the AB line.
    along = [(r[:, :2] - a) @ ab for r in rows]
    # The midline of the two rows, averaged per row so that a row with more
    # posts than its neighbour does not pull the alley towards itself.
    offset = np.mean([((r[:, :2] - a) @ across).mean() for r in rows])
    # Both rows plus a run-out at each end. `arange` stops short of its
    # endpoint, hence the extra stride.
    span = np.arange(
        min(u.min() for u in along) - RUNOUT,
        max(u.max() for u in along) + RUNOUT + STRIDE,
        STRIDE,
    )
    # The two rows' heights averaged. Past a row's own end its `interp` holds
    # that row's outermost post, which is what a run-out wants.
    height = np.mean([np.interp(span, u, r[:, 2]) for u, r in zip(along, rows)], axis=0)
    return np.column_stack([a + offset * across + np.outer(span, ab), height])
