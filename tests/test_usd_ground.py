"""Reading a scene's ground back: `vinerylab.usd.Ground`.

Built on the same `tiny_scene.json` as the builder's own tests. Its terrain is
a single triangle, which is exactly what a mesh that is *not* a height field
looks like, so the grid cases below give it points of their own.
"""

from __future__ import annotations

import json
import pathlib

import pytest
from pxr import Usd

from vinerylab.usd import Ground, build_stage

FIXTURE = pathlib.Path(__file__).parent / "fixtures" / "tiny_scene.json"

# A plane, z = 2x + y, over a grid whose columns are deliberately not evenly
# spaced: an interpolation that is right reproduces it exactly, anywhere, and
# one that assumes a constant spacing does not.
PLANE = [(x, y, 2 * x + y) for x in (0.0, 1.0, 3.0) for y in (0.0, 2.0)]


@pytest.fixture
def doc() -> dict:
    return json.loads(FIXTURE.read_text())


def staged(doc: dict, tmp_path: pathlib.Path, points: list | None = None) -> Usd.Stage:
    """The fixture scene, optionally with `points` for its ground.

    `Ground` reads the vertex grid, so what the faces over it are is beside
    the point.
    """
    for part in doc["parts"]:
        if points is not None and "heightfield_resolution" in part:
            part["points"] = [list(point) for point in points]
    return build_stage(doc, str(tmp_path / "scene.usda"))


@pytest.mark.parametrize(
    "x, y, z",
    [
        (0.0, 0.0, 0.0),  # on a grid point
        (3.0, 2.0, 8.0),
        (0.5, 0.5, 1.5),  # between them, in the narrow cell
        (2.0, 1.0, 5.0),  # and in the wide one
        (-5.0, 1.0, 1.0),  # off the grid entirely: the nearest edge, not zero
        (10.0, 9.0, 8.0),
    ],
)
def test_the_height_interpolates_the_grid_and_clamps_outside_it(doc, tmp_path, x, y, z):
    ground = Ground(staged(doc, tmp_path, PLANE))

    assert ground.height(x, y) == pytest.approx(z)


def test_a_ground_that_is_not_a_grid_is_refused(doc, tmp_path):
    """Filling a grid from it would leave holes, and a hole reads as a height
    of zero -- ground a robot would be spawned under."""
    with pytest.raises(ValueError, match="not a height field"):
        Ground(staged(doc, tmp_path))


def test_a_stage_with_no_ground_is_refused(doc, tmp_path):
    for part in doc["parts"]:
        part.pop("heightfield_resolution", None)

    with pytest.raises(ValueError, match="one ground"):
        Ground(staged(doc, tmp_path))
