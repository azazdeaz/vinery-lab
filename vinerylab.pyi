"""Type stubs for the `vinerylab` extension module.

Manually maintained — keep this in sync with the `#[pymethods]` blocks in
`src/python.rs`. Maturin picks this file up automatically (pure Rust project
layout: no `python/` source dir) and bundles it into the wheel along with an
auto-generated `py.typed` marker.
"""

class CubeParams:
    size: float
    variations: int

    def __init__(self, size: float = 0.1, variations: int = 3) -> None: ...
    def __repr__(self) -> str: ...

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

class GridParams:
    rows: int
    cols: int
    spacing: float
    seed: int

    def __init__(
        self,
        rows: int = 10,
        cols: int = 10,
        spacing: float = 0.2,
        seed: int = 0,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ParcelParams:
    """How vineyard rows are laid out across the terrain.

    Has no effect on the generated USD yet -- there is no element consuming
    `VineyardLayout` for geometry yet -- but is included in the aggregate for
    the geometry slice that follows.
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

class VineyardParams:
    """The full parameter set, one attribute per element.

    Fragments are live objects, so mutating them in place takes effect:

        params = VineyardParams()
        params.grid.rows = 4
    """

    cube: CubeParams
    terrain: TerrainParams
    grid: GridParams
    parcel: ParcelParams

    def __init__(
        self,
        cube: CubeParams | None = None,
        terrain: TerrainParams | None = None,
        grid: GridParams | None = None,
        parcel: ParcelParams | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def generate_usda(self) -> str:
        """Generates the scene and returns it as `usda` text."""
        ...
    def write_usd(self, path: str) -> None:
        """Generates the scene and writes it directly to `path` (format
        chosen by extension -- `.usda`, `.usdc`, `.usd`, `.usdz`)."""
        ...
