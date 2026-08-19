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

class VineyardParams:
    """The full parameter set, one attribute per element.

    Fragments are live objects, so mutating them in place takes effect:

        params = VineyardParams()
        params.grid.rows = 4
    """

    cube: CubeParams
    grid: GridParams

    def __init__(
        self,
        cube: CubeParams | None = None,
        grid: GridParams | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def generate_usda(self) -> str:
        """Generates the scene and returns it as `usda` text."""
        ...
    def write_usd(self, path: str) -> None:
        """Generates the scene and writes it directly to `path` (format
        chosen by extension -- `.usda`, `.usdc`, `.usd`, `.usdz`)."""
        ...
