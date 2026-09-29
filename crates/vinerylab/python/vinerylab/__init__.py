"""Procedural vineyard scene generation, authored to USD.

The scene generator itself is a compiled Rust extension, [`_core`]; this
module re-exports it so `vinerylab.VineyardParams` stays the import path it
has always been. The re-export list is generated from the Rust params structs
-- see `docs/editing-parameters.md`.

Isaac Lab integration lives in the `vinerylab.isaaclab` subpackage and is
*not* imported from here — it needs Isaac Lab installed, and plain
`import vinerylab` must keep working without it.
"""

# >>> generated: exports
from ._core import (
    CoverParams,
    LeafParams,
    ParcelParams,
    PlantingParams,
    PoleParams,
    SceneParams,
    ShootParams,
    TerrainParams,
    VineParams,
    VineyardParams,
    WeedParams,
    WireParams,
    __version__,
)

__all__ = [
    "CoverParams",
    "LeafParams",
    "ParcelParams",
    "PlantingParams",
    "PoleParams",
    "SceneParams",
    "ShootParams",
    "TerrainParams",
    "VineParams",
    "VineyardParams",
    "WeedParams",
    "WireParams",
    "__version__",
]
# <<< generated: exports
