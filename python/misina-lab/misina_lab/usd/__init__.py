"""Turning a scene document into a USD stage.

A generator builds the scene in Bevy and hands it over as JSON; this
subpackage is the only place that knows what USD is. See `build.py`'s module
docstring for the conventions it authors and why each one matters, and
`python -m misina_lab.usd` to build a saved document from the command line.
"""

from .build import FORMAT, GEOM, HEIGHT_FIELD, PARTS, build_stage, build_usd
from .ground import Ground

__all__ = [
    "FORMAT",
    "GEOM",
    "HEIGHT_FIELD",
    "PARTS",
    "Ground",
    "build_stage",
    "build_usd",
]
