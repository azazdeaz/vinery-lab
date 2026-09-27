"""Isaac Lab integration: spawn a generated vineyard as a scene asset.

Importing this subpackage requires Isaac Lab. It is deliberately *not*
imported by `vinerylab/__init__.py`, so `import vinerylab` on its own stays
usable without it.

Note the name: Python 3 imports are absolute, so `import isaaclab` from
inside `vinerylab.isaaclab` reaches the real top-level Isaac Lab package
rather than this one.
"""

try:
    import isaaclab  # noqa: F401
except ImportError as err:  # pragma: no cover - depends on the environment
    raise ImportError(
        "vinerylab.isaaclab requires Isaac Lab. Install it alongside vinerylab"
        " (see examples/isaaclab_demo/pyproject.toml), or use"
        " vinerylab.VineyardParams.write_usd() directly to generate a scene"
        " without it."
    ) from err

from .cutting import Shears
from .physics import CABLE, make_physics_cfg_newton, tune_shoots
from .vineyard import spawn_vineyard

# >>> generated: imports
from .vineyard_cfg import (
    CoverCfg,
    LeafCfg,
    ParcelCfg,
    PlantingCfg,
    PoleCfg,
    SceneCfg,
    ShootCfg,
    TerrainCfg,
    VineCfg,
    VineyardCfg,
    WeedCfg,
    WireCfg,
)

# <<< generated: imports

__all__ = [
    "CABLE",
    "Shears",
    "make_physics_cfg_newton",
    "spawn_vineyard",
    "tune_shoots",
    # >>> generated: all
    "CoverCfg",
    "LeafCfg",
    "ParcelCfg",
    "PlantingCfg",
    "PoleCfg",
    "SceneCfg",
    "ShootCfg",
    "TerrainCfg",
    "VineCfg",
    "VineyardCfg",
    "WeedCfg",
    "WireCfg",
    # <<< generated: all
]
