"""Type stubs for the `vinerylab._core` extension module.

Manually maintained — keep this in sync with the `#[pymethods]` blocks in
`src/python.rs`. Sits next to the compiled `_core` extension in the mixed
maturin layout (`python-source = "python"`), alongside the `py.typed` marker
that makes the whole package's annotations visible to type checkers.

`vinerylab/__init__.py` re-exports everything declared here, so consumers
write `from vinerylab import VineyardParams` rather than reaching into
`_core` themselves.
"""

__version__: str

class SceneParams:
    """Parameters that belong to no single element.

    `seed` is the one seed the whole scene is generated from. Every layer salts
    it with a constant of its own before drawing, so nudging one never re-rolls
    another -- change it and you get a different vineyard, not a different
    trunk on the same one.
    """

    seed: int

    def __init__(
        self,
        seed: int = 0,
    ) -> None: ...
    def __repr__(self) -> str: ...

class TerrainParams:
    width: float
    height: float
    max_elevation: float
    variations: int
    detail: int

    def __init__(
        self,
        width: float = 80.0,
        height: float = 50.0,
        max_elevation: float = 3.0,
        detail: int = 6,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ParcelParams:
    """How vineyard rows are laid out across the terrain.

    Solves the positions `PlantingParams` puts a vine at; `vine_spacing` also
    sizes the cordons `VineParams` builds.
    """

    orientation: float
    headland: float
    row_spacing: float
    vine_spacing: float
    post_spacing: float
    min_row_length: float
    trellis_height: float

    def __init__(
        self,
        orientation: float = 0.0,
        headland: float = 6.0,
        row_spacing: float = 2.4,
        vine_spacing: float = 1.2,
        post_spacing: float = 6.0,
        min_row_length: float = 10.0,
        trellis_height: float = 1.8,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PoleParams:
    """One trellis post: a plain grey cylinder.

    How tall a post is comes from `ParcelCfg.trellis_height` -- the posts are
    what hold the wires up there -- and where the posts stand comes from
    `ParcelParams.post_spacing`, so neither is here.

    There is no `variations` either. A post is a manufactured object, and the
    only variety a row of them shows is in how each was driven: a centimeter
    off line, a degree off plumb, a few centimeters deeper. That is applied per
    placement and needs no geometry of its own.
    """

    radius: float
    sides: int

    def __init__(
        self,
        radius: float = 0.04,
        sides: int = 8,
    ) -> None: ...
    def __repr__(self) -> str: ...

class VineParams:
    """The permanent woody framework of a grapevine.

    A trunk rising to the head, one or two cordons running along the fruiting
    wire from there, and the spurs pruned back onto them. `arms` is 1 for a
    unilateral vine or 2 for a bilateral one; how far each cordon reaches is
    solved from `ParcelParams.vine_spacing` and `cordon_gap` rather than set
    directly.

    Shape only -- where vines stand, and which ones are missing or young, is
    `PlantingParams`.
    """

    variations: int
    trunk_height: float
    trunk_radius: float
    trunk_wobble: float
    arms: int
    cordon_gap: float
    cordon_radius: float
    spur_spacing: float
    spur_length: float
    shoots_per_spur: float
    roughness: float
    sides: int
    detail: int

    def __init__(
        self,
        variations: int = 4,
        trunk_height: float = 0.9,
        trunk_radius: float = 0.035,
        trunk_wobble: float = 0.02,
        arms: int = 2,
        cordon_gap: float = 0.15,
        cordon_radius: float = 0.022,
        spur_spacing: float = 0.12,
        spur_length: float = 0.05,
        shoots_per_spur: float = 1.8,
        roughness: float = 0.14,
        sides: int = 8,
        detail: int = 20,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ShootParams:
    """One season's green growth off a spur.

    Shoots are shaped here and placed by `VineParams` -- how many a spur
    pushes is `VineParams.shoots_per_spur`, since that is a fact about the
    vine's pruning rather than about a shoot.

    `length` is bud to tip; whoever places one varies it slightly per shoot.
    `detail` is high compared to the rest of the scene because a shoot turns a
    quarter circle within a few centimeters of its bud, and it is cheap
    because a shoot is a shared prototype.

    A shoot also carries the canopy, so the two leaf knobs live here rather
    than on `LeafParams` -- how many leaves a shoot holds and how they hang is
    a fact about the shoot, the way `VineParams.shoots_per_spur` is a fact
    about the vine. `internode` is the spacing between leaf nodes up the
    shoot; setting it to 0 leaves the shoot bare. Leaf *size* is not set
    anywhere: it comes out of each leaf's age, as a scale on a prototype of
    fixed area.
    """

    variations: int
    length: float
    radius: float
    lean: float
    sides: int
    detail: int
    internode: float
    leaf_droop: float

    def __init__(
        self,
        variations: int = 4,
        length: float = 0.75,
        radius: float = 0.006,
        lean: float = 0.06,
        sides: int = 6,
        detail: int = 40,
        internode: float = 0.07,
        leaf_droop: float = 0.35,
    ) -> None: ...
    def __repr__(self) -> str: ...

class LeafParams:
    """One blade of the canopy.

    The five blade shapes are *drawn*, not generated -- one SVG outline each
    under `assets/leaves/`, embedded at build time -- so `variations` has a
    natural ceiling here: at 5 every drawing gets a mesh, and above that it
    buys nothing.

    Size is not a parameter: every prototype is built at exactly the same
    area, one full-grown leaf of about 150 cm^2. That is what lets a leaf's
    size be set purely by the scale it is placed at -- a younger or shadier
    leaf is the same prototype scaled down, and the scale means the same thing
    whichever of the five shapes came up.

    `detail` is how many triangles the blade's interior is cut into; the drawn
    margin costs about 180 on its own whatever it is set to.

    Where leaves hang, and how big each one ends up, is `ShootParams`.
    """

    detail: int

    def __init__(
        self,
        detail: int = 120,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PlantingParams:
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

    miss_rate: float
    young_rate: float
    young_scale: float

    def __init__(
        self,
        miss_rate: float = 0.03,
        young_rate: float = 0.08,
        young_scale: float = 0.55,
    ) -> None: ...
    def __repr__(self) -> str: ...

class VineyardParams:
    """The full parameter set, one attribute per element.

    Fragments are live objects, so mutating them in place takes effect:

        params = VineyardParams()
        params.terrain.detail = 8
    """

    terrain: TerrainParams
    parcel: ParcelParams
    planting: PlantingParams
    pole: PoleParams
    vine: VineParams
    shoot: ShootParams
    leaf: LeafParams

    def __init__(
        self,
        scene: SceneParams | None = None,
        terrain: TerrainParams | None = None,
        parcel: ParcelParams | None = None,
        planting: PlantingParams | None = None,
        pole: PoleParams | None = None,
        vine: VineParams | None = None,
        shoot: ShootParams | None = None,
        leaf: LeafParams | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def generate_scene_json(self) -> str:
        """Generates the scene and returns it as a JSON document.

        The whole contract with the USD builder. Public so a caller can cache
        the bytes, diff two scenes, or build the stage on another machine.
        """
        ...
    def write_usd(self, path: str) -> None:
        """Generates the scene and writes it to `path` as USD.

        The extension decides the format: `.usd`/`.usdc` for the binary crate
        form (about a third the bytes and roughly 4x faster for USD to parse),
        `.usda` for text. The file must not already exist.

        Needs `usd-core`, which is a dependency of this package: the scene is
        built in Rust and the USD is authored in Python, by
        `vinerylab.usd.build_usd`.
        """
        ...
