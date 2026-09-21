"""The route a row-straddling robot drives through a generated vineyard.

The waypoints come out of the generated scene itself -- the trellis posts for
the lines, the terrain mesh for the heights -- so the route re-solves whenever
the vineyard parameters change, with nothing to keep in sync by hand.

Unlike a robot that walks the alleys, a gantry drives *along* a row with a leg
on either side of it, so the waypoints sit on the row lines rather than
between them, and every change of row has to happen out in the headland.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np

from vinerylab.isaaclab import VineyardCfg
from vinerylab.isaaclab.vineyard import resolve_usd_path

if TYPE_CHECKING:  # `vinerylab.usd` pulls in `pxr`; see `row_route` for why not here
    from vinerylab.usd import Ground

STRIDE = 0.5  # m between waypoints
RUNOUT = 4.0  # m a pass carries on past the end posts, into the headland


def row_route(cfg: VineyardCfg) -> tuple[np.ndarray, float, Ground]:
    """Waypoints straddling every row of `cfg`'s vineyard, the row heading, and
    a height lookup for the ground they run over.

    Read off the trellis posts of the cached scene. Every row is parallel to
    every other by construction, so the first row's posts give an AB line the
    whole block is measured against: `along` metres up the line and `across`
    metres off it. A row is then an interval of `along` at a fixed `across`,
    and the robot drives that interval with the row between its legs.

    The result is a boustrophedon: down one row, out into the headland, across
    to the next, back down that one. Crossing from one row to the next takes
    each leg over a row line, so a pass reaches past the end posts of its
    neighbours as well as its own and the crossing stays clear of the vines.

    The returned heading is the row direction. A four-wheel-steer gantry holds
    it for the whole run -- it reverses down alternate rows and crabs sideways
    between them rather than turning around, which is what the machine is for
    and what keeps its legs out of the vines.

    Waypoint heights come from the terrain under each point rather than from
    the posts: a pass runs out well past its row's own last post -- far past
    it, where a row is clipped short and its neighbour is not -- and out there
    a post height is not the ground, it is the last one held flat. The legs
    joining one pass to the next are straight, so only their ends sit on the
    terrain exactly.
    """
    # Imported here, not at module level: both pull in `pxr`, and Kit's own
    # copy of `pxr` only wins the import if nothing loaded the pip one first.
    from pxr import Usd, UsdGeom

    from vinerylab.usd import ROOT, Ground

    stage = Usd.Stage.Open(resolve_usd_path(cfg))
    ground = Ground(stage)
    rows = [
        np.array(
            [
                UsdGeom.Xformable(post)
                .ComputeLocalToWorldTransform(Usd.TimeCode.Default())
                .ExtractTranslation()
                for post in row.GetChildren()
                if post.GetName().startswith("Pole_")
            ]
        )
        for row in stage.GetPrimAtPath(f"{ROOT}/Planting").GetChildren()
    ]

    # The AB line: `a` its origin and `ab` its unit direction. Posts are
    # authored from one end of a row to the other, so the first row's own end
    # posts define it, and every pass below is an offset of it.
    a = rows[0][0, :2]
    ab = rows[0][-1, :2] - a
    ab /= np.linalg.norm(ab)

    passes = _passes(rows, a, ab, ground)
    # Each pass joined to the next by a straight leg through the headland.
    route = [passes[0]]
    for leg in passes[1:]:
        route += [_between(route[-1][-1], leg[0]), leg]
    route = np.concatenate(route)
    # Driven back the way it came, so that the loop at the far side of the
    # block is another headland turn rather than a drive across every row.
    return np.concatenate([route, route[::-1]]), float(np.arctan2(ab[1], ab[0])), ground


def _passes(
    rows: list[np.ndarray], a: np.ndarray, ab: np.ndarray, ground: Ground
) -> list[np.ndarray]:
    """One run of waypoints per row, in alternating directions."""
    across = np.array([-ab[1], ab[0]])
    # Where each row's posts sit along and off the AB line. A row is straight
    # by construction, so the mean offset is the row line.
    along = [(row[:, :2] - a) @ ab for row in rows]
    offset = [((row[:, :2] - a) @ across).mean() for row in rows]

    passes = []
    for index in range(len(rows)):
        # Rows are clipped to the parcel one at a time, so their ends are
        # staggered. A pass covers its neighbours' ends too, which is what
        # puts the crossing between two rows past the last post of both.
        near = along[max(index - 1, 0) : index + 2]
        # `arange` stops short of its endpoint, hence the extra stride.
        span = np.arange(
            min(u.min() for u in near) - RUNOUT,
            max(u.max() for u in near) + RUNOUT + STRIDE,
            STRIDE,
        )
        line = a + offset[index] * across + np.outer(span, ab)
        waypoints = np.column_stack([line, [ground.height(x, y) for x, y in line]])
        passes.append(waypoints if index % 2 == 0 else waypoints[::-1])
    return passes


def _between(start: np.ndarray, end: np.ndarray) -> np.ndarray:
    """Waypoints a stride apart strictly between `start` and `end`.

    Both ends belong to the passes being joined, so neither is repeated here.
    """
    steps = max(round(float(np.linalg.norm(end - start)) / STRIDE), 1)
    return start + np.outer(np.arange(1, steps) / steps, end - start)
