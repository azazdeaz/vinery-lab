"""Reading a generated scene's ground back off its stage.

The one thing a robot in the scene has to agree with is where the surface is:
spawn a machine below it and the solver throws it back out. The generator
knows -- `Ground` in `src/elements/terrain.rs` is what every element is placed
against -- but that lives and dies inside one build, while what a consumer
gets is a `.usd` file. This is the same lookup over the same grid, read back
from the stage: `build.py` authors it, this reads it.

It is deliberately a plain Python class over plain lists. `vinerylab` depends
on `usd-core` and nothing else, and a height query is not worth an array
library.
"""

from __future__ import annotations

import bisect

from pxr import Usd, UsdGeom

from .build import HEIGHT_FIELD


class Ground:
    """The ground of a generated scene, as a height lookup.

        ground = Ground(Usd.Stage.Open(path))
        z = ground.height(x, y)

    The mesh is a regular grid of vertices in xy, which is also the grid a
    backend rasterizes the height field from, so a height between vertices is
    a bilinear blend of the four around it and at one it is exact -- the same
    answer the generator's own `Ground` gives, and the same surface the wheels
    will stand on.
    """

    def __init__(self, stage: Usd.Stage):
        prim = _terrain(stage)
        # In world space: the ground is a child of the scene root like anything
        # else, and nothing says it has to sit at the origin.
        to_world = UsdGeom.Xformable(prim).ComputeLocalToWorldTransform(Usd.TimeCode.Default())
        points = [to_world.Transform(point) for point in UsdGeom.Mesh(prim).GetPointsAttr().Get()]

        self._xs = sorted({point[0] for point in points})
        self._ys = sorted({point[1] for point in points})
        column = {x: index for index, x in enumerate(self._xs)}
        row = {y: index for index, y in enumerate(self._ys)}
        heights = {(column[x], row[y]): z for x, y, z in points}
        if len(self._xs) < 2 or len(self._ys) < 2 or len(heights) != len(self._xs) * len(self._ys):
            raise ValueError(
                f"{prim.GetPath()} is not a height field: {len(points)} points make"
                f" {len(heights)} of the {len(self._xs)}x{len(self._ys)} grid they span"
            )
        # Indexed `ix * len(ys) + iy`, as the generator holds them.
        self._heights = [
            heights[(ix, iy)] for ix in range(len(self._xs)) for iy in range(len(self._ys))
        ]

    def height(self, x: float, y: float) -> float:
        """Height at `(x, y)`, bilinear between the four grid points around it.

        Clamped to the grid at the edges, so a point off the terrain reads the
        ground nearest it rather than nothing at all.
        """
        ix, tx = _segment(self._xs, x)
        iy, ty = _segment(self._ys, y)
        rows = len(self._ys)
        # The two heights either side of `y`, on each of the columns either
        # side of `x`.
        near = self._heights[ix * rows + iy : ix * rows + iy + 2]
        far = self._heights[(ix + 1) * rows + iy : (ix + 1) * rows + iy + 2]
        return (
            near[0] * (1 - tx) * (1 - ty)
            + far[0] * tx * (1 - ty)
            + near[1] * (1 - tx) * ty
            + far[1] * tx * ty
        )


def _terrain(stage: Usd.Stage) -> Usd.Prim:
    """The ground mesh: the one prim on the stage authored as a height field.

    Found by what it is rather than by what it is called. The attribute is on
    the part, so it composes through to wherever the scene places it.
    """
    found = [prim for prim in stage.Traverse() if prim.HasAttribute(HEIGHT_FIELD)]
    if len(found) != 1:
        raise ValueError(
            f"a scene has one ground, carrying `{HEIGHT_FIELD}`;"
            f" this stage has {len(found)}: {[str(prim.GetPath()) for prim in found]}"
        )
    return found[0]


def _segment(axis: list[float], v: float) -> tuple[int, float]:
    """Where `v` falls on a strictly increasing `axis`: the cell below it, and
    how far across that cell it lies. `v` is clamped to the axis's own extent.

    The axes are not evenly spaced -- each rounds itself to whole spans of the
    terrain -- so this is a search, not a division.
    """
    v = min(max(v, axis[0]), axis[-1])
    index = min(bisect.bisect_right(axis, v) - 1, len(axis) - 2)
    return index, (v - axis[index]) / (axis[index + 1] - axis[index])
