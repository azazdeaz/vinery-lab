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
class SceneCfg:
    """Parameters that belong to no single element.

    `seed` is the one seed the whole scene is generated from. Every layer salts
    it with a constant of its own before drawing, so nudging one never re-rolls
    another -- change it and you get a different vineyard, not a different
    trunk on the same one.

    `season` is where in the growing season the scene is, from 0.0 at budbreak
    to 1.0 at harvest. Today only the weeds read it: which species are up, and
    whether a bolter has bolted.
    """

    seed: int = 0
    season: float = 0.5


@configclass
class TerrainCfg:
    """The ground surface the vineyard stands on.

    `length` runs along X -- the direction rows take at `ParcelCfg.orientation`
    0 -- and `width` along Y. The ground is noise sampled over that extent:
    `feature_size` is the distance in meters from one hill to the next and is
    anchored in world space, so a larger field shows more hills rather than
    larger ones. `max_inclination`, in degrees, caps how steep the *hills* get
    -- the elevation is solved from it and `feature_size`, so the same angle
    means the same steepness at any size.

    `roughness` is the height in meters of the bumps riding on those hills --
    the clods, ruts and tillage texture a machine drives over rather than
    climbs, and the reason a wheel or a foot sees anything but a plane. Set it
    to 0 for bare hills. `roughness_size` is the longest bump wavelength;
    shorter ones are added below it, each half the wavelength and half the
    height, which is the spectrum natural ground follows. The band does not
    count towards `max_inclination`, so adding bumps never flattens the grade.

    `detail` is grid samples per feature, which sets the mesh density, the
    collision height field's resolution, and how short a bump can get: the
    roughness band stops at the shortest wave the grid can carry, so bumps
    below about four grid steps need a higher `detail` to appear at all.
    """

    length: float = 80.0
    width: float = 50.0
    max_inclination: float = 20.0
    feature_size: float = 16.0
    roughness: float = 0.08
    roughness_size: float = 4.0
    detail: int = 32


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

    A slot that comes out young by `young_rate` is planted as a replant in its
    first season -- one green shoot out of the bare ground, not a shrunken
    mature vine -- and `young_scale` says how much of a full-grown shoot the
    youngest of them has put out.
    """

    miss_rate: float = 0.03
    young_rate: float = 0.08
    young_scale: float = 0.55


@configclass
class PoleCfg:
    """One trellis post: a plain grey cylinder.

    How tall a post is comes from `ParcelCfg.trellis_height` -- the posts are
    what hold the wires up there -- and where the posts stand comes from
    `ParcelCfg.post_spacing`, so neither is here.

    There is no `variations` either: a post is a manufactured
    object, and the only variety a row of them shows is in how each was
    driven, which is applied per placement.
    """

    radius: float = 0.04
    sides: int = 8


@configclass
class WireCfg:
    """The wires strung from post to post, and what the vines are trained onto.

    One *fruiting wire* on the post axis at `VineCfg.trunk_height` -- the head
    height the cordons are tied along -- and above it `catch_wires` levels of
    *pairs*, one wire either side of the post, that the season's shoots grow up
    between. The levels are spread evenly from the fruiting wire to just under
    `ParcelCfg.trellis_height`, so the top pair is the wire a hedger cuts to.

    Each wire spans one panel, post to post, so a run follows the ground the
    way the posts do. There is no `variations`: a trellis is strung from one
    reel of wire, and every span is the same mesh stretched to its own length.
    """

    catch_wires: int = 2
    radius: float = 0.0015


@configclass
class VineCfg:
    """The permanent woody framework of a grapevine.

    `arms` is 1 for a unilateral vine or 2 for a bilateral one; how far each
    cordon reaches is solved from `ParcelCfg.vine_spacing` and `cordon_gap`
    rather than set directly. Shape only -- where vines stand, and which ones
    are missing or young, is `PlantingCfg`.
    """

    variations: int = 4
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

    `stray` is the fraction of shoots the trellis failed to hold -- missed by
    shoot positioning, so grown out into the alley, or by hedging, so grown on
    past the top wire. A stray shoot leans out of the canopy, longer than the
    shoots beside it, and is what a machine passing over the row runs into. It
    is authored as a deformable curve instead of a mesh, and bends **only**
    under the coupled solver `make_coupled_physics_cfg` builds; every other
    backend imports it as an inert curve at its rest shape. Each one is a chain
    of rigid bodies, so this is the most expensive knob here: keep it low.
    """

    variations: int = 4
    length: float = 0.75
    radius: float = 0.006
    lean: float = 0.06
    sides: int = 6
    detail: int = 40
    internode: float = 0.07
    leaf_droop: float = 0.35
    stray: float = 0.0


@configclass
class LeafCfg:
    """One blade of the canopy.

    The blade shapes are drawn rather than generated -- one SVG outline each,
    embedded at build time -- so the first 5 of `variations` go on giving every
    drawing a mesh of its own, and the rest buys curls of them. Size is not a
    parameter: every blade is built at the same area, and a leaf's size comes
    from the scale it is placed at.

    `detail` is how many triangles the blade's interior is cut into, and the
    floor on how fine a curl those triangles can hold.

    `curl` is how far a blade bends out of the flat shape it was drawn as: a
    trough down the midrib, a droop along it and a ruffled margin, all scaled
    together. It is the middle of a spread -- every leaf draws its own share of
    it, and some curl the other way. Zero leaves the drawing flat.
    """

    variations: int = 40
    detail: int = 120
    curl: float = 1.0


@configclass
class CoverCfg:
    """What grows in the alley between two rows.

    `kind` is the regime, by name: ``"none"`` for a bare or tilled alley,
    ``"spontaneous"`` for a sward that came up on its own -- ragged, gappy --,
    ``"sown"`` for a drilled grass sward, one height and dense, or
    ``"cereal"`` for a winter rye in drill lines along the alley. Any other
    name raises ``ValueError`` when the scene is generated. The species are
    the generator's to pick from the regime.

    `alternate` leaves every second alley bare, the commonest permanent
    arrangement in France. `width` is how much of the alley the cover spans,
    centred; `height` its standing height in meters, from a few centimetres
    just after mowing to half a metre left to head; `cover` the fraction of
    the ground inside the band that is actually covered; `dryness` runs the
    colour from green at 0 to straw at 1.

    The sward is built as half-metre tiles of blades, instanced down each
    alley: `variations` is the budget of distinct tile meshes and `detail` the
    blades per square metre baked into one.
    """

    kind: str = "spontaneous"
    alternate: bool = False
    width: float = 0.75
    height: float = 0.15
    cover: float = 0.7
    dryness: float = 0.0
    variations: int = 12
    detail: int = 600


@configclass
class WeedCfg:
    """The plants that come up where nothing was sown: in the under-vine
    strip, and as escapes in the alley.

    `strip` is what is done to the under-vine strip, by name, and it picks
    the species: ``"herbicide"`` leaves the annual grasses and tall bolters a
    spray does not kill, ``"tilled"`` the annuals that come back from seed,
    ``"mown"`` the rosettes and tufts that duck the blade, ``"untouched"``
    everything. Any other name raises ``ValueError`` when the scene is
    generated. `SceneCfg.season` tilts the mix between the spring and summer
    flushes and decides whether a bolter has bolted.

    `strip_width` is how far the strip reaches either side of the trunks, in
    meters. `pressure` and `alley_pressure` are plants per square metre in the
    strip and the alley; zero is clean. `tall` is the share of plants that
    are tall -- bolters and sprawling broadleaves -- against low tufts, mats
    and rosettes.

    Every plant is one mesh; `variations` is the budget of distinct ones and
    `detail` the triangles a leaf is cut into.
    """

    strip: str = "mown"
    strip_width: float = 0.3
    pressure: float = 6.0
    alley_pressure: float = 0.5
    tall: float = 0.3
    variations: int = 24
    detail: int = 16


@configclass
class VineyardCfg(FileCfg):
    """Spawn a procedurally generated vineyard.

    The scene is generated on first use and cached as a USD file keyed on the
    geometry parameters below, then spawned through Isaac Lab's ordinary
    USD-file path -- so everything `FileCfg` offers (`scale`, `semantic_tags`,
    `rigid_props`, `collision_props`, visual materials, contact sensors)
    applies here too, and the prim path may be an env regex.

    It comes with its own static colliders: the ground as its own mesh, each
    post and trunk as a capsule. `collision_props` tunes those it can reach --
    `apply_nested` skips instanced prims, and everything but the ground and the
    proxies is instanced.

    .. code-block:: python

        VINEYARD_CFG = VineyardCfg(
            parcel=ParcelCfg(row_spacing=2.8, vine_spacing=1.05),
            scene=SceneCfg(seed=42),
            vine=VineCfg(arms=1),
        )

        # plain script or a direct env's _setup_scene()
        VINEYARD_CFG.func("/World/Vineyard", VINEYARD_CFG)

        # manager-based scene cfg
        vineyard = AssetBaseCfg(prim_path="/World/Vineyard", spawn=VINEYARD_CFG)

    There is no `usd_path`: the fragments below are what identifies the asset,
    and the file backing it is an implementation detail of the cache.
    """

    func: Callable | str = "{DIR}.vineyard:spawn_vineyard"

    scene: SceneCfg = SceneCfg()
    terrain: TerrainCfg = TerrainCfg()
    parcel: ParcelCfg = ParcelCfg()
    planting: PlantingCfg = PlantingCfg()
    pole: PoleCfg = PoleCfg()
    wire: WireCfg = WireCfg()
    vine: VineCfg = VineCfg()
    shoot: ShootCfg = ShootCfg()
    leaf: LeafCfg = LeafCfg()
    cover: CoverCfg = CoverCfg()
    weed: WeedCfg = WeedCfg()

    cache_dir: str | None = None
    """Where generated scenes are cached. Defaults to ``$VINERYLAB_CACHE_DIR``,
    else ``$XDG_CACHE_HOME/vinerylab/scenes``, else ``~/.cache/vinerylab/scenes``."""

    force_regenerate: bool = False
    """Regenerate even on a cache hit. For iterating on the generator itself."""


FRAGMENTS: tuple[tuple[str, type], ...] = (
    ("scene", SceneCfg),
    ("terrain", TerrainCfg),
    ("parcel", ParcelCfg),
    ("planting", PlantingCfg),
    ("pole", PoleCfg),
    ("wire", WireCfg),
    ("vine", VineCfg),
    ("shoot", ShootCfg),
    ("leaf", LeafCfg),
    ("cover", CoverCfg),
    ("weed", WeedCfg),
)
"""The geometry fragments, in the order `VineyardParams` takes them.

The single list both the pyclass conversion and the cache key walk, so adding
an element means adding one line here. Everything on `VineyardCfg` that is
*not* in this list is applied to the spawned prim rather than baked into the
USD, and so must not take part in the cache key.
"""
