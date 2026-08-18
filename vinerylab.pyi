"""Type stubs for the `vinerylab` extension module.

Manually maintained — keep this in sync with the `#[pymethods] impl
SceneParams` block in `src/python.rs`. Maturin picks this file up
automatically (pure Rust project layout: no `python/` source dir) and
bundles it into the wheel along with an auto-generated `py.typed` marker.
"""

class SceneParams:
    rows: int
    cols: int
    spacing: float
    cube_size: float

    def __init__(
        self,
        rows: int = 10,
        cols: int = 10,
        spacing: float = 0.2,
        cube_size: float = 0.1,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def generate_usda(self) -> str:
        """Generates the scene and returns it as `usda` text."""
        ...
    def write_usd(self, path: str) -> None:
        """Generates the scene and writes it directly to `path` (format
        chosen by extension -- `.usda`, `.usdc`, `.usd`, `.usdz`)."""
        ...
