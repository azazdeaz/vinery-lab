"""Spawner configuration for a procedurally generated vineyard.

One `@configclass` fragment per element, mirroring the `*Params` pyclasses in
`vinerylab._core` field for field. The fragments are plain Python dataclasses
rather than the pyclasses themselves on purpose: a pyclass has no `__dict__`,
which is what `isaaclab.utils.dict.class_to_dict` dispatches on, and it cannot
be deep-copied — so holding one on a cfg would break `cfg.to_dict()`,
`cfg.replace()` and every YAML/hydra round-trip Isaac Lab does with a scene
config. `vineyard.py` converts these into pyclasses at spawn time instead.
"""

from __future__ import annotations

from collections.abc import Callable

from isaaclab.sim.spawners.from_files.from_files_cfg import FileCfg
from isaaclab.utils.configclass import configclass


@configclass
class TerrainCfg:
    """The ground surface the vineyard stands on."""

    width: float = 80.0
    height: float = 50.0
    max_elevation: float = 3.0
    detail: int = 6


@configclass
class ParcelCfg:
    """How vineyard rows are laid out across the terrain.

    Solves the positions `PlantingCfg` puts a vine at; `vine_spacing` also
    sizes the cordons `VineCfg` builds.
    """

    orientation: float = 0.0
    headland: float = 6.0
    row_spacing: float = 2.4
    vine_spacing: float = 1.2
    post_spacing: float = 6.0
    min_row_length: float = 10.0
    trellis_height: float = 1.8


@configclass
class PlantingCfg:
    """What stands on the ground, and where.

    Each plant is authored as its own prim -- `/Vineyard/Planting/Row_000/
    Vine_007` -- so a simulator can address one: attach a semantic label, bind
    a rigid body, randomize it. A name refers to a planting *slot*, so a vine
    skipped by `miss_rate` leaves a gap in the numbering rather than shifting
    every name after it.
    """

    seed: int = 0
    miss_rate: float = 0.03
    young_rate: float = 0.08
    young_scale: float = 0.55


@configclass
class PoleCfg:
    """One trellis post: a plain grey cylinder.

    How tall a post is comes from `ParcelCfg.trellis_height` -- the posts are
    what hold the wires up there -- and where the posts stand comes from
    `ParcelCfg.post_spacing`, so neither is here.

    There is no `variations` or `seed` either: a post is a manufactured
    object, and the only variety a row of them shows is in how each was
    driven, which is applied per placement.
    """

    radius: float = 0.04
    sides: int = 8


@configclass
class VineCfg:
    """The permanent woody framework of a grapevine.

    `arms` is 1 for a unilateral vine or 2 for a bilateral one; how far each
    cordon reaches is solved from `ParcelCfg.vine_spacing` and `cordon_gap`
    rather than set directly. Shape only -- where vines stand, and which ones
    are missing or young, is `PlantingCfg`.
    """

    variations: int = 4
    seed: int = 0
    trunk_height: float = 0.9
    trunk_radius: float = 0.035
    trunk_wobble: float = 0.02
    arms: int = 2
    cordon_gap: float = 0.15
    cordon_radius: float = 0.022
    spur_spacing: float = 0.12
    spur_length: float = 0.05
    shoots_per_spur: float = 1.8
    roughness: float = 0.14
    sides: int = 8
    detail: int = 20


@configclass
class ShootCfg:
    """One season's green growth off a spur.

    How many a spur pushes is `VineCfg.shoots_per_spur`, since that is a fact
    about the vine's pruning rather than about a shoot.

    A shoot also carries the canopy, so the two leaf knobs live here rather
    than on `LeafCfg`. `internode` is the spacing between leaf nodes up the
    shoot; setting it to 0 leaves the shoot bare.
    """

    variations: int = 4
    seed: int = 0
    length: float = 0.75
    radius: float = 0.006
    lean: float = 0.06
    sides: int = 6
    detail: int = 40
    internode: float = 0.07
    leaf_droop: float = 0.35


@configclass
class LeafCfg:
    """One blade of the canopy.

    The blade shapes are drawn rather than generated -- one SVG outline each,
    embedded at build time -- so there is no `variations` or `seed` here the
    way there is on `VineCfg`. Size is not a parameter either: every prototype
    is built at the same area, and a leaf's size comes from the scale it is
    placed at.

    `detail` is how many triangles the blade's interior is cut into.
    """

    detail: int = 120


@configclass
class VineyardCfg(FileCfg):
    """Spawn a procedurally generated vineyard.

    The scene is generated on first use and cached as a USD file keyed on the
    geometry parameters below, then spawned through Isaac Lab's ordinary
    USD-file path -- so everything `FileCfg` offers (`scale`, `semantic_tags`,
    `rigid_props`, `collision_props`, visual materials, contact sensors)
    applies here too, and the prim path may be an env regex.

    .. code-block:: python

        VINEYARD_CFG = VineyardCfg(
            parcel=ParcelCfg(row_spacing=2.8, vine_spacing=1.05),
            vine=VineCfg(arms=1, seed=42),
        )

        # plain script or a direct env's _setup_scene()
        VINEYARD_CFG.func("/World/Vineyard", VINEYARD_CFG)

        # manager-based scene cfg
        vineyard = AssetBaseCfg(prim_path="/World/Vineyard", spawn=VINEYARD_CFG)

    There is no `usd_path`: the fragments below are what identifies the asset,
    and the file backing it is an implementation detail of the cache.
    """

    func: Callable | str = "{DIR}.vineyard:spawn_vineyard"

    terrain: TerrainCfg = TerrainCfg()
    parcel: ParcelCfg = ParcelCfg()
    planting: PlantingCfg = PlantingCfg()
    pole: PoleCfg = PoleCfg()
    vine: VineCfg = VineCfg()
    shoot: ShootCfg = ShootCfg()
    leaf: LeafCfg = LeafCfg()

    cache_dir: str | None = None
    """Where generated scenes are cached. Defaults to ``$VINERYLAB_CACHE_DIR``,
    else ``$XDG_CACHE_HOME/vinerylab/scenes``, else ``~/.cache/vinerylab/scenes``."""

    force_regenerate: bool = False
    """Regenerate even on a cache hit. For iterating on the generator itself."""


FRAGMENTS: tuple[tuple[str, type], ...] = (
    ("terrain", TerrainCfg),
    ("parcel", ParcelCfg),
    ("planting", PlantingCfg),
    ("pole", PoleCfg),
    ("vine", VineCfg),
    ("shoot", ShootCfg),
    ("leaf", LeafCfg),
)
"""The geometry fragments, in the order `VineyardParams` takes them.

The single list both the pyclass conversion and the cache key walk, so adding
an element means adding one line here. Everything on `VineyardCfg` that is
*not* in this list is applied to the spawned prim rather than baked into the
USD, and so must not take part in the cache key.
"""
