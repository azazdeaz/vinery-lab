"""Building a USD stage from a vinerylab scene document.

This module is where *all* of the project's USD knowledge lives. The Rust
generator owns the scene -- what geometry exists, where it goes, what
references what -- and hands it over as the plain JSON document described in
`src/scene/doc.rs`. Everything below is USD's side of that line: prim types,
schemas, composition arcs and stage metadata.

Every rule below fails *silently* if dropped.

Stage metadata
    ``upAxis`` and ``metersPerUnit`` are root-layer-only metadata and do not
    compose through references or payloads, so a consumer cannot correct for a
    stage that declares the wrong one. USD also defaults to Y-up when ``upAxis``
    is unauthored, which means leaving it unset is not neutral -- it is wrong,
    for a scene whose only export target is robotics simulation (Isaac Lab and
    ROS, both REP-103 right-handed Z-up). The document carries the convention
    and this module authors what it is told.

The parts library lives *inside* the default prim
    ``/Vineyard/parts``, not ``/parts``. Everything the scene is made of hangs
    off one prim, so a consumer referencing the layer gets a complete asset.
    It also keeps the door open for relationships: relationship targets --
    material bindings, physics joint bodies, a ``PointInstancer``'s
    ``prototypes`` -- are namespace-mapped through composition arcs, and a
    target outside the referenced subtree cannot be mapped and is dropped
    outright. Composition arcs may leave the default prim; targets may not.

...and it is a ``class``
    Prototypes are otherwise ordinary defined prims, and a renderer would draw
    the library as a pile of geometry stacked at the origin. ``class`` makes the
    subtree abstract, which traversals skip, while references still resolve
    against it by path. It is preferred over a ``visibility = "invisible"``
    opinion, which would ride along into every consumer that references the
    layer.

``subdivisionScheme = "none"``
    USD's default is ``catmullClark``, which declares every mesh here a
    subdivision *cage* rather than the surface it is. Anything that honours it
    -- Isaac, usdview, anything on Hydra -- rounds the drawn teeth off a leaf
    margin and sands away the bark ridges the strand kernel exists to produce.

``extent``
    What a consumer frustum-culls against. Unauthored, it has to walk every
    point of every prim to find one, and we already hold the points.

``displayColor`` at ``constant`` interpolation
    The one channel every consumer reads. Nothing here binds a material, and
    that is deliberate: ``UsdPreviewSurface``'s ``diffuseColor`` defaults to
    grey and ``displayColor`` is consulted *only* for prims with no bound
    material, so binding one without also wiring the colour through a primvar
    reader turns the scene grey. When materials do arrive they belong inside
    ``/Vineyard/parts/<name>`` -- inside the subtree that gets referenced --
    for the namespace-mapping reason above.

A part is an ``Xform`` *wrapping* its mesh, not a bare ``Mesh``
    This is what makes ``instanceable`` do anything at all. A USD instance
    shares its *descendants* through a prototype; the instance prim's own
    attributes stay authored on the instance. Referencing a bare ``Mesh`` and
    marking it instanceable therefore yields an empty prototype and N copies of
    the points -- perfectly valid, drawn correctly, and with none of the
    sharing that was the point. Wrapping the mesh one level down puts the
    geometry inside the prototype, where thousands of instances share it.

A referencing prim is defined *typeless*
    The referenced part supplies the type. Defining the prim as an ``Xform``
    first would leave a local type opinion competing with the referenced one.

``instanceable`` on referencing prims
    What makes tens of thousands of individually addressable leaf prims
    affordable: the paths stay real while the renderer draws one prototype per
    part. Safe because a referencing prim authors no children of its own -- an
    instanceable prim's *authored* descendants would be unreachable, while the
    one composed in through the reference is exactly what lands in the
    prototype. The exporter enforces the no-authored-children half on the Rust
    side.

``xformOp:orient`` rather than ``xformOp:rotateXYZ``
    USD's ``rotateXYZ`` and Bevy's Euler conventions disagree about intrinsic
    versus extrinsic composition, and a mismatch produces a scene that is
    plausibly wrong rather than obviously wrong. The document carries
    quaternions; there is no convention left to disagree about.

The prim tree is authored through ``Sdf``, not the ``Usd`` stage API
    ``Sdf.CreatePrimInLayer`` and ``Sdf.AttributeSpec`` write layer specs
    directly, inside one ``Sdf.ChangeBlock``; ``Usd.Stage.DefinePrim`` and the
    ``UsdGeom.Xformable`` op helpers recompose and notify per call, which at
    this scene's prim count is around five times slower for the same output.
    Two consequences to respect:

    * Nothing inside the block may read *composed* state -- that is what
      ``Usd.Stage.DefinePrim`` does, and calling it there throws. The parts
      library is authored through the ``Usd`` API before the block opens,
      where its fourteen prims cost nothing.
    * ``Sdf`` enforces no schema, so authoring an op stack onto a prim type
      that cannot carry one now fails silently rather than raising. See
      `NON_TRANSFORMABLE`.
"""

from __future__ import annotations

import functools
from typing import Any, Iterable, Mapping, Sequence

from pxr import Gf, Sdf, Usd, UsdGeom, Vt

FORMAT = 1
"""Document version this builder understands. See `src/scene/doc.rs`."""

ROOT = "/Vineyard"
"""The scene root, and the stage's default prim."""

PARTS = f"{ROOT}/parts"
"""Root of the mesh library. Referenced by every geometry prim."""

GEOM = "Geom"
"""Name of the `Mesh` inside a part. See the module docstring for why a part
wraps its mesh rather than being one."""

NON_TRANSFORMABLE = frozenset({"Scope"})
"""Prim types the generator emits that cannot carry an xform op stack.

Authoring goes through `Sdf`, which checks no schema, so this stands in for
the `UsdGeom.Xformable(prim)` test the `Usd` API used to make for free. It
lists the types the document can actually name, not every such type in USD --
extend it alongside the generator.
"""

_XFORM_OP_ORDER = Vt.TokenArray(["xformOp:translate", "xformOp:orient", "xformOp:scale"])

_UP_AXIS_TOKENS = {"X": UsdGeom.Tokens.x, "Y": UsdGeom.Tokens.y, "Z": UsdGeom.Tokens.z}


def build_usd(doc: Mapping[str, Any], path: str) -> None:
    """Author `doc` as a USD stage at `path`.

    The extension decides the format: `.usd`/`.usdc` for the binary crate form
    (about a third the bytes and roughly 4x faster for USD to parse), `.usda`
    for text.

    Args:
        doc: A scene document, as produced by the Rust generator.
        path: Where to write the stage. Must not already exist.

    Raises:
        ValueError: If the document's format version is not understood.
    """
    stage = build_stage(doc, path)
    stage.GetRootLayer().Save()


def build_stage(doc: Mapping[str, Any], path: str) -> Usd.Stage:
    """The stage `build_usd` writes, before it is saved.

    Split out so tests can inspect a stage without touching the filesystem
    twice, and so a caller composing something larger can keep authoring.
    """
    format_version = doc.get("format")
    if format_version != FORMAT:
        raise ValueError(
            f"scene document is format {format_version}, this builder speaks {FORMAT}"
        )

    stage = Usd.Stage.CreateNew(path)
    _author_stage_metadata(stage, doc)
    _author_parts(stage, doc.get("parts", ()))

    with Sdf.ChangeBlock():
        _author_node(stage.GetRootLayer(), ROOT, doc["root"])
    stage.SetDefaultPrim(stage.GetPrimAtPath(ROOT))
    return stage


# --- stage ----------------------------------------------------------


def _author_stage_metadata(stage: Usd.Stage, doc: Mapping[str, Any]) -> None:
    up_axis = doc.get("up_axis", "Z")
    if up_axis not in _UP_AXIS_TOKENS:
        raise ValueError(f"unknown up axis {up_axis!r}")
    UsdGeom.SetStageUpAxis(stage, _UP_AXIS_TOKENS[up_axis])
    UsdGeom.SetStageMetersPerUnit(stage, float(doc.get("meters_per_unit", 1.0)))


# --- the parts library ----------------------------------------------


def _author_parts(stage: Usd.Stage, parts: Iterable[Mapping[str, Any]]) -> None:
    library = stage.CreateClassPrim(PARTS)
    library.SetTypeName("Scope")
    for part in parts:
        _author_part(stage, part)


def _author_part(stage: Usd.Stage, part: Mapping[str, Any]) -> UsdGeom.Mesh:
    points = [tuple(p) for p in part["points"]]
    indices = list(part["indices"])

    # An Xform wrapping the mesh, so that referencing it and marking the
    # reference instanceable puts the geometry in the prototype rather than
    # leaving a copy on every instance. See the module docstring.
    root = f"{PARTS}/{part['name']}"
    UsdGeom.Xform.Define(stage, root)
    mesh = UsdGeom.Mesh.Define(stage, f"{root}/{GEOM}")
    mesh.CreatePointsAttr(Vt.Vec3fArray(points))
    mesh.CreateFaceVertexIndicesAttr(Vt.IntArray(indices))
    # Every face is a triangle, so the counts are implied by the index count
    # and are not transmitted.
    mesh.CreateFaceVertexCountsAttr(Vt.IntArray([3] * (len(indices) // 3)))
    mesh.CreateSubdivisionSchemeAttr(UsdGeom.Tokens.none)

    extent = _extent(points)
    if extent is not None:
        mesh.CreateExtentAttr(Vt.Vec3fArray(list(extent)))

    if part.get("double_sided"):
        mesh.CreateDoubleSidedAttr(True)

    if normals := part.get("normals"):
        mesh.CreateNormalsAttr(Vt.Vec3fArray([tuple(n) for n in normals]))
        mesh.SetNormalsInterpolation(UsdGeom.Tokens.vertex)

    if uvs := part.get("uvs"):
        primvar = UsdGeom.PrimvarsAPI(mesh).CreatePrimvar(
            "st", Sdf.ValueTypeNames.TexCoord2fArray, UsdGeom.Tokens.vertex
        )
        primvar.Set(Vt.Vec2fArray([tuple(uv) for uv in uvs]))

    color = mesh.CreateDisplayColorPrimvar(UsdGeom.Tokens.constant)
    color.Set(Vt.Vec3fArray([tuple(part["display_color"])]))

    return mesh


def _extent(
    points: Sequence[tuple[float, float, float]],
) -> tuple[Gf.Vec3f, Gf.Vec3f] | None:
    """The corners of the axis-aligned bounding box, or None when empty."""
    if not points:
        return None
    lo = [min(p[axis] for p in points) for axis in range(3)]
    hi = [max(p[axis] for p in points) for axis in range(3)]
    return Gf.Vec3f(*lo), Gf.Vec3f(*hi)


# --- the prim tree --------------------------------------------------


def _author_node(layer: Sdf.Layer, path: str, node: Mapping[str, Any]) -> None:
    spec = Sdf.CreatePrimInLayer(layer, path)
    # `CreatePrimInLayer` leaves an `over`, and authors ancestors as overs too
    # -- harmless here, since a node is always authored before its children.
    spec.specifier = Sdf.SpecifierDef

    reference = node.get("reference")
    if reference is not None:
        # Typeless: the referenced Mesh supplies the type, and a local opinion
        # would win over it. See the module docstring.
        spec.referenceList.prependedItems = [Sdf.Reference(primPath=_part_path(reference))]
        if node.get("instanceable"):
            spec.instanceable = True
    else:
        spec.typeName = node.get("type_name", "Xform")

    if xform := node.get("xform"):
        if spec.typeName in NON_TRANSFORMABLE:
            raise ValueError(f"{path} is not transformable but carries a transform")
        _author_xform(spec, xform)

    for child in node.get("children", ()):
        _author_node(layer, f"{path}/{child['name']}", child)


@functools.cache
def _part_path(name: str) -> Sdf.Path:
    """The library path a part of this name lives at.

    Cached because every one of the scene's prims references one of a handful
    of parts, and parsing the same path back out of a string each time is a
    measurable slice of authoring a large scene.
    """
    return Sdf.Path(f"{PARTS}/{name}")


def _author_xform(spec: Sdf.PrimSpec, xform: Mapping[str, Any]) -> None:
    """The translate / orient / scale op stack, in that order.

    Float precision throughout, matching the f32 the document carries -- a
    double-precision op would only pad the values back out with zeroes.
    """
    # The document is xyzw (Bevy's `Quat` layout); Gf.Quatf takes the real
    # part first.
    x, y, z, w = xform["orient"]
    for name, value_type, value in (
        ("xformOp:translate", Sdf.ValueTypeNames.Float3, Gf.Vec3f(*xform["translate"])),
        ("xformOp:orient", Sdf.ValueTypeNames.Quatf, Gf.Quatf(w, Gf.Vec3f(x, y, z))),
        ("xformOp:scale", Sdf.ValueTypeNames.Float3, Gf.Vec3f(*xform["scale"])),
    ):
        Sdf.AttributeSpec(spec, name, value_type).default = value

    # Uniform, as `UsdGeom.Xformable` authors it: the op stack is a fact about
    # the prim, not something that varies over time.
    Sdf.AttributeSpec(
        spec, "xformOpOrder", Sdf.ValueTypeNames.TokenArray, Sdf.VariabilityUniform
    ).default = _XFORM_OP_ORDER
