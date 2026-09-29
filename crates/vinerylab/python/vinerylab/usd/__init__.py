"""The vineyard's USD side: `misina_lab.usd`, plus the name of its root prim.

The builder and the `Ground` reader are the core's; `python -m misina_lab.usd
scene.json scene.usd` builds a document the viewer saved. Kept out of
`vinerylab/__init__.py` so that plain `import vinerylab` keeps working without
`usd-core` installed.
"""

from misina_lab.usd import FORMAT, GEOM, HEIGHT_FIELD, PARTS, Ground, build_stage, build_usd

ROOT = "/Vineyard"
"""The scene root, and the stage's default prim: the generator's name."""

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
