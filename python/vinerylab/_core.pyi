"""Type stubs for the `vinerylab._core` extension module.

The fragment classes between the ``generated`` markers are rendered from the
Rust params structs by ``cargo test regen_params -- --ignored``: edit the
structs, not this file -- `docs/editing-parameters.md` says how. The stub sits
next to the compiled `_core` extension in the mixed maturin layout
(`python-source = "python"`), alongside the `py.typed` marker that makes the
whole package's annotations visible to type checkers.

`vinerylab/__init__.py` re-exports everything declared here, so consumers
write `from vinerylab import VineyardParams` rather than reaching into
`_core` themselves.
"""

from typing import Literal

__version__: str

# >>> generated: fragments
CoverKind = Literal["none", "spontaneous", "sown", "cereal"]
"""What `CoverParams.kind` may be set to."""

WeedStrip = Literal["herbicide", "tilled", "mown", "untouched"]
"""What `WeedParams.strip` may be set to."""

class SceneParams:
    """Scene-wide parameters, owned by no element: one seed and one date for everything
    on the ground.
    """

    seed: int
    """The one seed the whole scene is generated from. Every layer salts it with a
    constant of its own, so nudging one layer's knobs never re-rolls another: a new
    seed is a different vineyard, not a different trunk on the same one.
    """
    season: float
    """Where in the growing season the scene is: `0.0` at budbreak, `1.0` at harvest.
    Today only the weeds read it, for which species are up and whether a bolter has
    bolted; the canopy does not yet.
    """

    def __init__(
        self,
        *,
        seed: int = 0,
        season: float = 0.5,
    ) -> None: ...
    def __repr__(self) -> str: ...

class TerrainParams:
    """The ground surface the vineyard stands on: hills the field is laid over, with
    tillage bumps riding on them.

    `length` runs along X, the direction rows take at orientation 0, and `width`
    along Y. The hills are noise anchored in world space at `feature_size`, so a
    larger field shows more hills rather than larger ones, and their grade is capped
    by `max_inclination`. The bumps are a separate band, `roughness` tall and
    `roughness_size` long at the coarsest, that does not count towards the grade:
    adding them never flattens a hill.
    """

    length: float
    """Extent along X, in meters. Rows run along it at orientation 0."""
    width: float
    """Extent along Y, in meters."""
    max_inclination: float
    """Upper bound on the hills' slope, in degrees. The elevation amplitude is solved
    from this and `feature_size`, so the same value gives the same steepness whatever
    the field's extent or resolution. This is the grade a route has to climb;
    `roughness` rides on top of it and is not counted here, the way a clod does not
    make a field steep.
    """
    feature_size: float
    """Distance from one hill to the next, in meters. The noise field is anchored in
    world space at this size, so changing the extent uncovers more or less of the
    same landscape rather than rescaling it.
    """
    roughness: float
    """Height of the bumps riding on the hills, in meters — the clods, ruts and
    tillage texture a machine rides over rather than climbs. Zero leaves the ground
    as bare hills.
    """
    roughness_size: float
    """Longest wavelength in the bump band, in meters. Shorter octaves are added below
    it, down to the finest the grid resolves, so this is the coarsest bump rather
    than the only one.
    """
    detail: int
    """Grid samples per feature: how finely the mesh follows the noise. Also the
    collision height field's resolution, and how short a bump may get, since the
    roughness band stops at the shortest wave the grid carries.
    """

    def __init__(
        self,
        *,
        length: float = 80.0,
        width: float = 50.0,
        max_inclination: float = 20.0,
        feature_size: float = 16.0,
        roughness: float = 0.08,
        roughness_size: float = 4.0,
        detail: int = 32,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ParcelParams:
    """How vineyard rows are laid out across the terrain.

    Solves the positions the planting puts a vine and a post at. `vine_spacing` also
    sizes the cordons the vines build, and `trellis_height` is how tall the posts
    stand.
    """

    orientation: float
    """Row direction, in degrees counter-clockwise from +X."""
    headland: float
    """Inset from the terrain's edge left unplanted, in meters — the turning area
    machinery needs at each end of a row.
    """
    row_spacing: float
    """Distance between neighbouring row centerlines, in meters."""
    vine_spacing: float
    """Distance between neighbouring vines along a row, in meters."""
    post_spacing: float
    """Target distance between posts along a row, in meters. Panel count is solved from
    this and the row's actual length, so real post spacing comes out close to this
    value rather than exactly equal to it.
    """
    min_row_length: float
    """Rows shorter than this, after clipping to the headland-inset rectangle, are
    dropped rather than planted.
    """
    trellis_height: float
    """Height of the trellis above the ground, in meters: how tall the posts stand, and
    what the top catch wires hang just under.
    """

    def __init__(
        self,
        *,
        orientation: float = 0.0,
        headland: float = 6.0,
        row_spacing: float = 2.4,
        vine_spacing: float = 1.2,
        post_spacing: float = 6.0,
        min_row_length: float = 10.0,
        trellis_height: float = 1.8,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PlantingParams:
    """What stands on the ground, and where.

    Each plant is authored as its own prim, `/Vineyard/Planting/Row_000/Vine_007`, so
    a simulator can address one: attach a semantic label, bind a rigid body,
    randomize it. A name refers to a planting slot, so a vine skipped by `miss_rate`
    leaves a gap in the numbering rather than shifting every name after it.

    A slot that comes out young by `young_rate` is planted as a replant in its first
    season, one green shoot out of the bare ground rather than a shrunken mature
    vine, and `young_scale` says how much of a full-grown shoot the youngest of them
    has put out.
    """

    miss_rate: float
    """Fraction of planting positions left empty. Real vineyards have gaps, and a
    perception model trained without them learns that they can't happen.
    """
    young_rate: float
    """Fraction of vines that are recent replants rather than mature."""
    young_scale: float
    """How small the youngest replant is, relative to a full-grown shoot."""

    def __init__(
        self,
        *,
        miss_rate: float = 0.03,
        young_rate: float = 0.08,
        young_scale: float = 0.55,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PoleParams:
    """One trellis post: a plain grey cylinder.

    How tall a post is comes from the parcel's `trellis_height`, since the posts are
    what hold the wires up there, and where the posts stand from its `post_spacing`,
    so neither is here.

    There is no `variations` either. A post is a manufactured object, and the only
    variety a row of them shows is in how each was driven: a centimeter off line, a
    degree off plumb, a few centimeters deeper. That is applied per placement and
    needs no geometry of its own.
    """

    radius: float
    """Post radius, in meters. The default is the 8 cm round softwood post that is the
    commonest thing in a European vineyard; a steel profile post is nearer half as
    thick.
    """
    sides: int
    """Vertices around the post. The silhouette, and the only detail knob a straight
    tube has.
    """

    def __init__(
        self,
        *,
        radius: float = 0.04,
        sides: int = 8,
    ) -> None: ...
    def __repr__(self) -> str: ...

class WireParams:
    """The wires strung from post to post, and what the vines are trained onto.

    One fruiting wire on the post axis at the vines' `trunk_height`, the head height
    the cordons are tied along, and above it `catch_wires` levels of pairs, one wire
    either side of the post, that the season's shoots grow up between. The levels are
    spread evenly from the fruiting wire to just under the parcel's `trellis_height`,
    so the top pair is the wire a hedger cuts to.

    Each wire spans one panel, post to post, so a run follows the ground the way the
    posts do. There is no `variations`: a trellis is strung from one reel of wire,
    and every span is the same mesh stretched to its own length.
    """

    catch_wires: int
    """Levels of catch wires above the fruiting wire. Each level is a pair, one wire
    either side of the post, and the shoots grow up between them; two pairs is the
    usual vertical-shoot-positioned trellis.
    """
    radius: float
    """Wire radius, in meters. The default is the 3 mm high-tensile steel a trellis is
    strung with.
    """

    def __init__(
        self,
        *,
        catch_wires: int = 2,
        radius: float = 0.0015,
    ) -> None: ...
    def __repr__(self) -> str: ...

class VineParams:
    """The permanent woody framework of a grapevine.

    A trunk rising to the head, one or two cordons running along the fruiting wire
    from there, and the spurs pruned back onto them. `arms` is 1 for a unilateral
    vine or 2 for a bilateral one; how far each cordon reaches is solved from the
    parcel's `vine_spacing` and `cordon_gap` rather than set directly.

    Shape only: where vines stand, and which ones are missing or young, is the
    planting's.
    """

    trunk_height: float
    """Ground to head, in meters: the height of the fruiting wire. Not the trellis
    height, which is where the tops of the posts are.
    """
    trunk_radius: float
    """Trunk radius at the base, in meters."""
    trunk_wobble: float
    """How far the trunk's axis wanders off vertical, in meters."""
    arms: int
    """Cordons per vine: 1 for a unilateral vine, 2 for a bilateral one."""
    cordon_gap: float
    """Bare wire left between the cordon tips of neighbouring vines, in meters. Together
    with the parcel's vine spacing this is what sets how far a cordon reaches.
    """
    cordon_radius: float
    """Cordon radius at the head, in meters."""
    spur_spacing: float
    """Target distance between spurs along a cordon, in meters."""
    spur_length: float
    """How far a spur stands off its cordon, in meters."""
    shoots_per_spur: float
    """Shoots per spur, as a fractional count: the whole part is certain and the
    fraction is the odds of one more. A spur is pruned to two buds, so `1.8` — two
    shoots four times in five, one otherwise — is what a healthy spur-pruned vine
    looks like.
    """
    roughness: float
    """Depth of the bark ridges, as a fraction of the local radius."""
    sides: int
    """Vertices around each tube. The silhouette, visible on every instance."""
    detail: int
    """Rings per meter along each tube. Barely visible at row distance, so this is the
    cheaper of the two detail knobs to turn down.
    """
    variations: int
    """How many distinct vine meshes the scene may hold. A budget, not a count: the
    plants are clustered and this is how many representatives the clustering may
    keep. Lower it to trade variety for memory, raise it to spend memory on variety.
    """

    def __init__(
        self,
        *,
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
        variations: int = 4,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ShootParams:
    """One season's green growth off a spur.

    Shoots are shaped here and placed by the vine: how many a spur pushes is the
    vine's `shoots_per_spur`, since that is a fact about its pruning rather than
    about a shoot. `length` is bud to tip, and whoever places a shoot varies it a
    little.

    A shoot also carries the canopy, so the two leaf knobs live here rather than on
    the leaf: how many leaves a shoot holds and how they hang is a fact about the
    shoot. Leaf size is set nowhere; it comes out of each leaf's age, as a scale on a
    prototype of fixed area.

    `stray` is the share of shoots the trellis failed to hold. A stray shoot leans
    out of the canopy, longer than the shoots beside it, and is exported as a
    deformable curve a physics engine bends; `flexible` off exports it as an ordinary
    static mesh at the same rest shape instead.
    """

    length: float
    """Bud to tip, in meters — how tall a shoot stands above the spur it grew from.
    Whoever places one varies this a little per shoot.
    """
    radius: float
    """Radius at the bud, in meters."""
    lean: float
    """How far the tip wanders off vertical, in meters."""
    stray: float
    """The fraction of shoots the trellis failed to hold: missed by shoot positioning,
    so grown out into the alley, or by hedging, so grown on past the top wire. A
    stray shoot leans out of the canopy and is exported as a deformable curve rather
    than a mesh, which makes this the most expensive knob in the scene.
    """
    flexible: bool
    """Whether a stray shoot is exported as a deformable curve a solver bends. Off, it
    is a single static mesh at the same rest shape: the same lean out of the canopy,
    nothing for a solver to pick up. For a backend with no rods, or to keep a
    canopy's look without paying for the bodies.
    """
    internode: float
    """Distance between leaf nodes up the shoot, in meters — how many leaves it
    carries, said the way a viticulturist would. Zero leaves the shoot bare.
    """
    leaf_droop: float
    """How far a full-grown blade pitches below horizontal, in radians. The small blades
    at the tip stand nearly straight out.
    """
    sides: int
    """Vertices around the tube."""
    detail: int
    """Rings per meter along the tube. Higher than anywhere else in the scene and
    cheaper than it looks: a stem is a shared mesh, and the bend at the tip needs the
    density.
    """
    variations: int
    """How many distinct stem meshes the scene may hold. A budget, not a count: the
    shoots are clustered and this is how many representatives the clustering may
    keep, once for the shoots the trellis holds and again for the stray ones, which
    are clustered apart.
    """

    def __init__(
        self,
        *,
        length: float = 0.75,
        radius: float = 0.006,
        lean: float = 0.06,
        stray: float = 0.0,
        flexible: bool = True,
        internode: float = 0.07,
        leaf_droop: float = 0.35,
        sides: int = 6,
        detail: int = 40,
        variations: int = 4,
    ) -> None: ...
    def __repr__(self) -> str: ...

class LeafParams:
    """One blade of the canopy.

    The blade shapes are drawn rather than generated, one SVG outline each embedded
    at build time, so the first five of `variations` go on giving every drawing a
    mesh of its own and the rest buys curls of them. Size is not a parameter: every
    blade is built at the same area, one full-grown leaf of about 150 cm², and a
    leaf's size comes from the scale it is placed at. Where leaves hang, and how big
    each one ends up, is the shoot's.
    """

    variations: int
    """How many distinct blade meshes the scene may hold. A budget, not a count, with a
    floor under it: the five drawn shapes each get a mesh first, and what the rest
    buys is curls of them.
    """
    detail: int
    """How finely the inside of a blade is subdivided, as the number of triangles its
    area is cut into. The drawn margin costs about 180 on its own whatever this is
    set to.
    """
    curl: float
    """How far a blade bends out of the flat shape it was drawn as: a trough down the
    midrib, a droop along it and a ruffled margin, all scaled together. The middle of
    a spread, with every leaf drawing its own share of it, and some curling the other
    way. Zero leaves the drawing flat.
    """

    def __init__(
        self,
        *,
        variations: int = 40,
        detail: int = 120,
        curl: float = 1.0,
    ) -> None: ...
    def __repr__(self) -> str: ...

class CoverParams:
    """What grows in the alley between two rows.

    `kind` is the regime, by name, and the species are the generator's to pick from
    it. The sward is built as half-metre tiles of blades, instanced down each alley:
    `variations` is the budget of distinct tile meshes and `detail` the blades per
    square metre baked into one.
    """

    kind: CoverKind
    """What the alley grows, by name: `none` for a bare or tilled alley, `spontaneous`
    for a sward that came up on its own, ragged and gappy, `sown` for a drilled grass
    sward, one height and dense, or `cereal` for a winter rye in drill lines along
    the alley.
    """
    alternate: bool
    """Leave every second alley bare: the cover on half the alleys, the tractor's tyres
    on the other half. The commonest permanent arrangement in France.
    """
    width: float
    """How much of the alley's width the cover spans, as a fraction, centred on the
    alley. Three quarters keeps a band clear of the vines either side.
    """
    height: float
    """Standing height, in meters: a few centimetres just after mowing, half a metre
    when left to head, more for a cereal in spring.
    """
    cover: float
    """Fraction of the ground inside the band the cover actually covers."""
    dryness: float
    """`0.0` green, `1.0` straw — a Mediterranean alley in August."""
    variations: int
    """How many distinct tile meshes the scene may hold. A budget, not a count."""
    detail: int
    """Blades per square metre baked into a tile at full cover. The one knob that trades
    sward density for triangles.
    """

    def __init__(
        self,
        *,
        kind: CoverKind = "spontaneous",
        alternate: bool = False,
        width: float = 0.75,
        height: float = 0.15,
        cover: float = 0.7,
        dryness: float = 0.0,
        variations: int = 12,
        detail: int = 600,
    ) -> None: ...
    def __repr__(self) -> str: ...

class WeedParams:
    """The plants that come up where nothing was sown: in the under-vine strip, and as
    escapes in the alley.

    `strip` is what is done to the under-vine strip, and it picks the species; the
    scene's `season` tilts the mix between the spring and summer flushes and decides
    whether a bolter has bolted. Every plant is one mesh: `variations` is the budget
    of distinct ones and `detail` the triangles a leaf is cut into.
    """

    strip: WeedStrip
    """What is done to the under-vine strip, by name, which picks the species:
    `herbicide` leaves the annual grasses and tall bolters a spray does not kill,
    `tilled` the annuals that come back from seed, `mown` the rosettes and tufts that
    duck the blade, `untouched` everything.
    """
    strip_width: float
    """How far the under-vine strip reaches either side of the trunks, in meters."""
    pressure: float
    """Plants per square metre in the strip. Zero is a clean strip; the ceiling is one
    plant per slot, twenty-five.
    """
    alley_pressure: float
    """Plants per square metre in the alley, between the strips — the escapes."""
    tall: float
    """The share of plants that are tall — bolters and broadleaves — as against low
    tufts, mats and rosettes.
    """
    variations: int
    """How many distinct plant meshes the scene may hold. A budget, not a count."""
    detail: int
    """Triangles a leaf is cut into."""

    def __init__(
        self,
        *,
        strip: WeedStrip = "mown",
        strip_width: float = 0.3,
        pressure: float = 6.0,
        alley_pressure: float = 0.5,
        tall: float = 0.3,
        variations: int = 24,
        detail: int = 16,
    ) -> None: ...
    def __repr__(self) -> str: ...

# <<< generated: fragments

class VineyardParams:
    """The full parameter set, one attribute per element.

    Fragments are live objects, so mutating them in place takes effect:

        params = VineyardParams()
        params.terrain.detail = 8
    """

    # >>> generated: aggregate
    scene: SceneParams
    terrain: TerrainParams
    parcel: ParcelParams
    planting: PlantingParams
    pole: PoleParams
    wire: WireParams
    vine: VineParams
    shoot: ShootParams
    leaf: LeafParams
    cover: CoverParams
    weed: WeedParams

    def __init__(
        self,
        scene: SceneParams | None = None,
        terrain: TerrainParams | None = None,
        parcel: ParcelParams | None = None,
        planting: PlantingParams | None = None,
        pole: PoleParams | None = None,
        wire: WireParams | None = None,
        vine: VineParams | None = None,
        shoot: ShootParams | None = None,
        leaf: LeafParams | None = None,
        cover: CoverParams | None = None,
        weed: WeedParams | None = None,
    ) -> None: ...
    # <<< generated: aggregate
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
