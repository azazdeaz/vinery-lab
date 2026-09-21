"""Turning a vinerylab scene document into a USD stage.

The Rust generator builds the scene in Bevy and hands it over as JSON; this
subpackage is the only place in the project that knows what USD is. See
`build.py`'s module docstring for the conventions it authors and why each one
matters.

Kept out of `vinerylab/__init__.py` so that plain `import vinerylab` keeps
working without `usd-core` installed.
"""

from .build import FORMAT, GEOM, HEIGHT_FIELD, PARTS, ROOT, build_stage, build_usd
from .ground import Ground

__all__ = [
    "FORMAT",
    "GEOM",
    "HEIGHT_FIELD",
    "PARTS",
    "ROOT",
    "Ground",
    "build_stage",
    "build_usd",
]
