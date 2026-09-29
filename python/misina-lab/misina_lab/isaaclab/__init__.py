"""Isaac Lab integration: spawn a generated scene as a scene asset.

Importing this subpackage requires Isaac Lab. It is deliberately *not*
imported by `misina_lab/__init__.py`, so `import misina_lab` on its own stays
usable without it.

Note the name: Python 3 imports are absolute, so `import isaaclab` from
inside `misina_lab.isaaclab` reaches the real top-level Isaac Lab package
rather than this one.
"""

try:
    import isaaclab  # noqa: F401
except ImportError as err:  # pragma: no cover - depends on the environment
    raise ImportError(
        "misina_lab.isaaclab requires Isaac Lab. Install it alongside the generator"
        " (see examples/isaaclab_demo/pyproject.toml), or call the params class's"
        " write_usd() directly to generate a scene without it."
    ) from err

from .cutting import Shears
from .rods import CABLE, PLANT_GROUPS, SCENE_GROUP, make_physics_cfg_newton, steps_rods, tune_rods
from .spawn import GeneratedSceneCfg, resolve_usd_path, spawn_generated, to_params

__all__ = [
    "CABLE",
    "PLANT_GROUPS",
    "SCENE_GROUP",
    "GeneratedSceneCfg",
    "Shears",
    "make_physics_cfg_newton",
    "resolve_usd_path",
    "spawn_generated",
    "steps_rods",
    "to_params",
    "tune_rods",
]
