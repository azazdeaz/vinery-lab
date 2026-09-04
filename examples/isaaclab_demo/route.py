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
RUNOUT = 3.0  # m an alley carries on past its end posts, into the headland


def alley_route(cfg: VineyardCfg) -> np.ndarray:
    """Waypoints down every alley of `cfg`'s vineyard, in scene coordinates.

    Read off the trellis posts of the cached scene: a row's posts trace that
    row's centerline, and the alley beside it is that line shifted half the
    row spacing toward the next row. Alleys are walked in alternating
    directions, so the result is one continuous path with no drive back to the
    start between them.

    Waypoint heights come from the posts of the row the alley was derived from
    rather than the terrain under the alley itself. Only the spawn height reads
    them, and the two are centimeters apart on any terrain this generator
    produces.
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

    route = []
    for index, (row, neighbour) in enumerate(zip(rows, rows[1:])):
        # Neighbouring rows can hold different numbers of posts -- they are
        # clipped to the parcel separately -- so the shift is the perpendicular
        # distance between the two centerlines, not the gap between endpoints.
        along = row[-1, :2] - row[0, :2]
        normal = np.array([-along[1], along[0]]) / np.linalg.norm(along)
        alley = _paved(row + np.append(normal * (normal @ (neighbour[0, :2] - row[0, :2])) / 2, 0.0))
        route.append(alley if index % 2 == 0 else alley[::-1])
    return np.concatenate(route)


def _paved(posts: np.ndarray) -> np.ndarray:
    """`posts` as evenly spaced waypoints, carried on past both ends.

    A row is a straight segment by construction, so sampling the line the posts
    lie on loses nothing but the centimeter of wobble each post was driven
    with. Two things this buys the follower:

    * waypoints a stride apart rather than a post-spacing apart, so steering at
      the next one keeps the robot on the alley instead of cutting the corner
      to a point six metres away -- and an alley is only a row spacing wide.
    * a run-out into the headland at each end, so the turn between two alleys
      happens clear of the vines rather than inside the last panel.
    """
    direction = posts[-1, :2] - posts[0, :2]
    direction /= np.linalg.norm(direction)
    # How far along the line each post sits, then the sample points spanning it.
    posted = (posts[:, :2] - posts[0, :2]) @ direction
    span = np.arange(-RUNOUT, posted[-1] + RUNOUT, STRIDE)
    # Heights interpolate between the posts; past either end `interp` holds the
    # last post's height, which is what the run-out wants.
    return np.column_stack([posts[0, :2] + np.outer(span, direction), np.interp(span, posted, posts[:, 2])])
