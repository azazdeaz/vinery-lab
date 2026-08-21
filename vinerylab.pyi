"""Type stubs for the `vinerylab` extension module.

Manually maintained — keep this in sync with the `#[pymethods]` blocks in
`src/python.rs`. Maturin picks this file up automatically (pure Rust project
layout: no `python/` source dir) and bundles it into the wheel along with an
auto-generated `py.typed` marker.
"""

class TerrainParams:
    width: float
    height: float
    max_elevation: float
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
    seed: int
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
        seed: int = 0,
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
    """

    variations: int
    seed: int
    length: float
    radius: float
    lean: float
    sides: int
    detail: int

    def __init__(
        self,
        variations: int = 4,
        seed: int = 0,
        length: float = 0.75,
        radius: float = 0.006,
        lean: float = 0.06,
        sides: int = 6,
        detail: int = 40,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PlantingParams:
    """What stands on the ground, and where.

    Each plant is authored as its own prim -- `/Vineyard/Planting/Row_000/
    Vine_007` -- so a simulator can address one: attach a semantic label, bind
    a rigid body, randomize it. A name refers to a planting *slot*, so a vine
    skipped by `miss_rate` leaves a gap in the numbering rather than shifting
    every name after it.
    """

    seed: int
    miss_rate: float
    young_rate: float
    young_scale: float

    def __init__(
        self,
        seed: int = 0,
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
    vine: VineParams
    shoot: ShootParams

    def __init__(
        self,
        terrain: TerrainParams | None = None,
        parcel: ParcelParams | None = None,
        planting: PlantingParams | None = None,
        vine: VineParams | None = None,
        shoot: ShootParams | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def generate_usda(self) -> str:
        """Generates the scene and returns it as `usda` text."""
        ...
    def write_usd(self, path: str) -> None:
        """Generates the scene and writes it directly to `path` (format
        chosen by extension -- `.usda`, `.usdc`, `.usd`, `.usdz`)."""
        ...
