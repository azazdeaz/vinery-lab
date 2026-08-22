"""Procedural vineyard scene generation, authored to USD.

The scene generator itself is a compiled Rust extension, [`_core`]; this
module re-exports it so `vinerylab.VineyardParams` stays the import path it
has always been.

Isaac Lab integration lives in the `vinerylab.isaaclab` subpackage and is
*not* imported from here — it needs Isaac Lab installed, and plain
`import vinerylab` must keep working without it.
"""

from ._core import (
    LeafParams,
    ParcelParams,
    PlantingParams,
    PoleParams,
    ShootParams,
    TerrainParams,
    VineParams,
    VineyardParams,
    __version__,
)

__all__ = [
    "LeafParams",
    "ParcelParams",
    "PlantingParams",
    "PoleParams",
    "ShootParams",
    "TerrainParams",
    "VineParams",
    "VineyardParams",
    "__version__",
]
