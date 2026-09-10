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
    The fallback channel, for a consumer that resolves no material -- Hydra
    Storm and usdview, which read no MDL. Still authored on every drawn prim,
    and still the colour the materials below are built from, so the two cannot
    drift apart.

Every drawn prim binds an MDL material of its own
    Unbound, Kit shades the whole scene with one default surface tinted by
    ``displayColor``: a single roughness and a single specular level for bark,
    foliage and soil alike. The document carries a surface response per part,
    and this module turns each into a material.

    The target is Kit/Omniverse, so the shaders are MDL and the universal
    ``outputs:surface`` is left unauthored -- a renderer that reads no MDL
    resolves no surface for its context and falls back to ``displayColor``,
    which is authored above.

    A material is authored as a **child of the prim it shades**, which is what
    keeps it inside the subtree a reference maps: a binding whose target lands
    outside the referenced prim cannot be namespace-mapped and is dropped, so
    a material in a scope of its own would silently unbind on every instance.

    ``OmniPBR`` for an opaque surface. A translucent one -- a leaf blade --
    needs ``OmniSurface``, the only stock Kit material with a subsurface term,
    and pairs it with ``thin_walled``: a blade has no interior, and thin-walled
    turns the subsurface term into a second, flipped diffuse lobe rather than a
    volume, which is both what a backlit leaf looks like and far cheaper than
    volumetric scattering at a canopy's instance count.

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

Colliders are authored where physics can reach them
    Nothing here is a rigid body -- a vineyard stands still -- so a collider is
    a static one, which is what lets the ground collide as an exact triangle
    mesh (``physics:approximation = "none"``, illegal on a dynamic collider).

    The ground's collision schema lives *inside* its part, so it composes in
    through the reference; the generator keeps every prim referencing such a
    part non-instanceable, because a collider inside a prototype is reachable
    only through an instance proxy. The ground also carries
    ``newton:heightfield:resolution``, its grid spacing in meters: Newton
    rasterizes it into a height field at that spacing rather than handing
    MuJoCo a mesh, which MuJoCo would collide as its convex hull. Everything else a robot bumps into is a
    ``Capsule`` prim of its own -- the only round shape PhysX has natively,
    needing no cooking -- marked ``purpose = "guide"`` so no renderer draws it.

A flexible organ is a curve and the material it bends by
    A ``BasisCurves`` carrying ``PhysicsCurvesDeformableSimAPI`` is imported as a
    rod: one capsule body per segment, joined by spring joints. Three things
    make that work, and dropping any of them leaves a curve that does nothing.

    * The curve must be **linear** and **non-periodic**; anything else is
      skipped with a warning rather than refused.
    * Stiffness and density are read off a *bound* ``PhysicsCurvesDeformableMaterialAPI``
      material. Without the binding the importer falls back to defaults stiff
      enough that nothing visibly bends.
    * A rod floats free. What holds it is ``physics:masses``: a massless
      segment is a static one, so zeroing the leading points bolts the curve to
      where it was authored. See `_cable_point_masses`.

    Only Newton's VBD solver simulates one. Under every other backend nothing
    drives the segment prims, so a flexible organ draws correctly at its rest
    shape and never moves.

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
import math
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

from pxr import Gf, Sdf, Usd, UsdGeom, UsdPhysics, UsdShade, Vt

FORMAT = 6
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

_COLLISION_API = Sdf.TokenListOp.Create(prependedItems=["PhysicsCollisionAPI"])
"""What `UsdPhysics.CollisionAPI.Apply` writes, for the prims authored through
`Sdf` -- which enforces no schema and so has no `Apply` of its own."""

_CABLE_API = Sdf.TokenListOp.Create(
    prependedItems=[
        "PhysicsCurvesDeformableSimAPI",
        "PhysicsCollisionAPI",
        # Declared, not just authored: USD warns about a binding on a prim that
        # does not apply this.
        "MaterialBindingAPI",
    ]
)
"""Schemas that make a `BasisCurves` a simulated, collidable cable."""

_CABLE_MATERIAL_API = Sdf.TokenListOp.Create(prependedItems=["PhysicsCurvesDeformableMaterialAPI"])

CABLE_MATERIAL = "PhysicsMaterial"
"""Name of the physics material prim authored under every cable."""

MATERIAL = "Material"
"""Name of the visual material prim authored under every drawn prim."""

SHADER = "Shader"
"""Name of the `Shader` inside it."""

MDL_OPAQUE = ("OmniPBR.mdl", "OmniPBR")
MDL_TRANSLUCENT = ("OmniSurface.mdl", "OmniSurface")
"""The MDL module and sub-identifier behind an opaque and a translucent
surface. Named by bare relative path, which Kit resolves against its own MDL
search path -- an absolute one would pin the stage to one install."""

_BOLTED_POINTS = 2
"""Leading control points given no mass, which bolts the segment between them
down. See `_cable_point_masses`."""

CABLE_MATERIAL_ATTRS: dict[str, float] = {
    # Young's modulus of a green cane, in Pa. The importer derives all four rod
    # stiffnesses -- stretch, shear, bend, twist -- from this, Poisson's ratio
    # and the cross-section, so it is the one knob that says how stiff a
    # flexible organ is. At a shoot's radius it leaves a cane that stands up
    # under its own weight and folds out of a robot's way.
    "youngsModulus": 1.0e9,
    "poissonsRatio": 0.3,
    # Fresh cane is mostly water.
    "density": 800.0,
}
"""Cable material, in SI. Not on the scene document: these are tuning, and the
document carries scene facts.

Authored under the current AOUSD names. The unprefixed ones (`stretchStiffness`
and friends) are a deprecated revision that the importer reads as *structural*
values and warns about."""


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
        raise ValueError(f"scene document is format {format_version}, this builder speaks {FORMAT}")

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

    # Inside the part, so the binding composes in through every reference to it.
    UsdShade.MaterialBindingAPI.Apply(mesh.GetPrim())
    _author_material(stage.GetRootLayer().GetPrimAtPath(mesh.GetPath()), part)

    if approximation := part.get("collision"):
        # The mesh is its own collider. Authored inside the part so it composes
        # in through the reference -- which the generator keeps
        # non-instanceable for exactly this reason.
        UsdPhysics.CollisionAPI.Apply(mesh.GetPrim())
        UsdPhysics.MeshCollisionAPI.Apply(mesh.GetPrim()).CreateApproximationAttr(approximation)

    if resolution := part.get("heightfield_resolution"):
        # Newton rasterizes this collider into a height field at this spacing.
        # Backends that read the triangles ignore the attribute.
        mesh.GetPrim().CreateAttribute(
            "newton:heightfield:resolution", Sdf.ValueTypeNames.Float
        ).Set(resolution)

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


# --- materials ------------------------------------------------------


def _author_material(spec: Sdf.PrimSpec, surface: Mapping[str, Any]) -> None:
    """Author an MDL material describing `surface` beneath `spec`, and bind it.

    Beneath rather than beside: a binding target outside the referenced prim
    is dropped by namespace mapping, so a material that does not travel with
    the prim it shades unbinds on every instance.

    The caller declares `MaterialBindingAPI` -- `Sdf` enforces no schema, and
    USD warns about a binding on a prim that does not apply it.
    """
    module, identifier, inputs = _mdl_shader(surface)

    material = Sdf.PrimSpec(spec, MATERIAL, Sdf.SpecifierDef, "Material")
    shader = Sdf.PrimSpec(material, SHADER, Sdf.SpecifierDef, "Shader")
    for name, value_type, value in (
        ("info:implementationSource", Sdf.ValueTypeNames.Token, "sourceAsset"),
        ("info:mdl:sourceAsset", Sdf.ValueTypeNames.Asset, Sdf.AssetPath(module)),
        ("info:mdl:sourceAsset:subIdentifier", Sdf.ValueTypeNames.Token, identifier),
    ):
        Sdf.AttributeSpec(shader, name, value_type, Sdf.VariabilityUniform).default = value
    for name, value_type, value in inputs:
        Sdf.AttributeSpec(shader, f"inputs:{name}", value_type).default = value

    # The MDL render context only. A renderer that does not speak MDL resolves
    # no surface here and falls back to `displayColor`; authoring the universal
    # `outputs:surface` as well would take that fallback away.
    out = Sdf.AttributeSpec(shader, "outputs:out", Sdf.ValueTypeNames.Token)
    terminal = Sdf.AttributeSpec(material, "outputs:mdl:surface", Sdf.ValueTypeNames.Token)
    terminal.connectionPathList.explicitItems = [out.path]

    # Not custom: `MaterialBindingAPI` declares the relationship, and `Sdf`
    # defaults the other way because it checks no schema.
    binding = Sdf.RelationshipSpec(spec, "material:binding", custom=False)
    binding.targetPathList.explicitItems = [material.path]


def _mdl_shader(
    surface: Mapping[str, Any],
) -> tuple[str, str, list[tuple[str, Sdf.ValueTypeName, Any]]]:
    """The MDL module, sub-identifier and shader inputs for one surface.

    `roughness` and `reflectance` mean what they do in the document: perceptual
    roughness, and a specular level where 0.5 is the 4% an ordinary dielectric
    reflects. OmniPBR's `specular_level` is that scale exactly. OmniSurface
    instead weights a Fresnel lobe at its own IOR, so the level is doubled onto
    that weight -- same anchor, one lobe at 0.5 -- and saturates above it.
    """
    color = Gf.Vec3f(*(float(c) for c in surface["display_color"]))
    roughness = float(surface["roughness"])
    reflectance = float(surface["reflectance"])
    translucency = float(surface.get("translucency", 0.0))

    if not translucency:
        return (
            *MDL_OPAQUE,
            [
                ("diffuse_color_constant", Sdf.ValueTypeNames.Color3f, color),
                ("reflection_roughness_constant", Sdf.ValueTypeNames.Float, roughness),
                ("specular_level", Sdf.ValueTypeNames.Float, reflectance),
                # Nothing in a vineyard is a conductor, and OmniPBR's default
                # is already 0 -- authored so the value is on the prim rather
                # than in the reader's memory of the MDL.
                ("metallic_constant", Sdf.ValueTypeNames.Float, 0.0),
            ],
        )

    return (
        *MDL_TRANSLUCENT,
        [
            ("diffuse_reflection_color", Sdf.ValueTypeNames.Color3f, color),
            # OmniSurface dims the base to 0.8 by default; the document's
            # colour is the colour, as it is for an opaque part.
            ("diffuse_reflection_weight", Sdf.ValueTypeNames.Float, 1.0),
            ("specular_reflection_roughness", Sdf.ValueTypeNames.Float, roughness),
            ("specular_reflection_weight", Sdf.ValueTypeNames.Float, min(1.0, 2.0 * reflectance)),
            # A blade has no interior. Thin-walled makes the subsurface term a
            # flipped diffuse lobe instead of a volume -- what a leaf lit from
            # behind looks like, at a fraction of the cost.
            ("thin_walled", Sdf.ValueTypeNames.Bool, True),
            ("enable_diffuse_transmission", Sdf.ValueTypeNames.Bool, True),
            ("subsurface_weight", Sdf.ValueTypeNames.Float, translucency),
            # What the transmitted light is tinted by, which is why a backlit
            # leaf reads green rather than black.
            ("subsurface_transmission_color", Sdf.ValueTypeNames.Color3f, color),
        ],
    )


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

    if collider := node.get("collider"):
        _author_collider(spec, collider)

    if cable := node.get("cable"):
        _author_cable(spec, cable)

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


def _author_collider(spec: Sdf.PrimSpec, collider: Mapping[str, Any]) -> None:
    """A capsule collision proxy: the shape, and the schema that makes it one.

    The prim's ``axis`` is left unauthored -- USD's default is already the
    ``+Z`` the document builds capsules about. ``height`` measures the
    cylindrical section alone, so the capsule reaches ``height / 2 + radius``
    either side of its origin, which is what ``extent`` bounds.

    ``purpose = "guide"`` keeps it out of a render without hiding it from
    physics, which reads collision independently of purpose.
    """
    radius = float(collider["radius"])
    height = float(collider["height"])
    reach = height / 2.0 + radius
    for name, value_type, value in (
        ("radius", Sdf.ValueTypeNames.Double, radius),
        ("height", Sdf.ValueTypeNames.Double, height),
        (
            "extent",
            Sdf.ValueTypeNames.Float3Array,
            Vt.Vec3fArray([Gf.Vec3f(-radius, -radius, -reach), Gf.Vec3f(radius, radius, reach)]),
        ),
    ):
        Sdf.AttributeSpec(spec, name, value_type).default = value

    # Uniform, as the schema declares it: what a prim is for does not vary
    # over time.
    Sdf.AttributeSpec(
        spec, "purpose", Sdf.ValueTypeNames.Token, Sdf.VariabilityUniform
    ).default = UsdGeom.Tokens.guide

    spec.SetInfo("apiSchemas", _COLLISION_API)


def _author_cable(spec: Sdf.PrimSpec, cable: Mapping[str, Any]) -> None:
    """A deformable curve and the material it bends by.

    Simulated but never drawn: ``purpose = "guide"`` keeps it out of a render
    the same way it keeps a collision capsule out of one. A ``BasisCurves``
    whose points move every frame renders with a visible glitch under Kit's RTX
    delegate, so a flexible organ is drawn by a mesh on each of the rod segments
    the importer drives instead -- see `src/elements/shoot.rs`.

    ``widths`` and ``displayColor`` are authored anyway, so the curve reads as
    the organ it stands for when guides are turned on. Nothing in the import
    path reads either, and neither sizes the rod: that is a chain of equal
    capsules sized by the material's ``physics:curvesThickness``.

    What holds the curve in place is ``physics:masses``; see
    `_cable_point_masses`.
    """
    points = Vt.Vec3fArray([tuple(p) for p in cable["points"]])
    widths = Vt.FloatArray([float(w) for w in cable["widths"]])
    thickness = float(cable["thickness"])
    for name, value_type, value in (
        ("points", Sdf.ValueTypeNames.Point3fArray, points),
        # One curve per prim: the importer builds a rod per curve, and a
        # multi-curve prim would weld into a single articulation.
        ("curveVertexCounts", Sdf.ValueTypeNames.IntArray, Vt.IntArray([len(points)])),
        # A curve is drawn as a tube around its centerline, so the box has to
        # clear the widths as well as the points -- which is what
        # `ComputeExtent` adds and a box around the points alone does not.
        (
            "extent",
            Sdf.ValueTypeNames.Float3Array,
            UsdGeom.BasisCurves.ComputeExtent(points, widths),
        ),
    ):
        Sdf.AttributeSpec(spec, name, value_type).default = value

    # One width per point -- the taper -- which is what `vertex` says. It is
    # *metadata* on the attribute; a sibling `widths:interpolation` attribute is
    # ignored, and the default happens to be `vertex` anyway, so a mistake here
    # is invisible until the count disagrees.
    taper = Sdf.AttributeSpec(spec, "widths", Sdf.ValueTypeNames.FloatArray)
    taper.default = widths
    taper.SetInfo("interpolation", UsdGeom.Tokens.vertex)

    # One colour for the whole curve, and one material, both carrying the
    # surface of the mesh this curve stands in for -- a cane drawn beside a
    # rigid shoot of the same kind must not shade differently from it.
    color = Sdf.AttributeSpec(spec, "primvars:displayColor", Sdf.ValueTypeNames.Color3fArray)
    color.default = Vt.Vec3fArray([tuple(cable["display_color"])])
    color.SetInfo("interpolation", UsdGeom.Tokens.constant)
    _author_material(spec, cable)

    # Uniform, as `UsdGeom.BasisCurves` declares them. Only a linear,
    # non-periodic curve imports as a cable; anything else is skipped. The
    # `guide` purpose is uniform for the same reason it is on a collider: what
    # a prim is for does not vary over time.
    for name, value in (
        ("type", UsdGeom.Tokens.linear),
        ("wrap", UsdGeom.Tokens.nonperiodic),
        ("purpose", UsdGeom.Tokens.guide),
    ):
        Sdf.AttributeSpec(
            spec, name, Sdf.ValueTypeNames.Token, Sdf.VariabilityUniform
        ).default = value

    spec.SetInfo("apiSchemas", _CABLE_API)
    Sdf.AttributeSpec(spec, "physics:collisionEnabled", Sdf.ValueTypeNames.Bool).default = True

    material = Sdf.PrimSpec(spec, CABLE_MATERIAL, Sdf.SpecifierDef, "Material")
    material.SetInfo("apiSchemas", _CABLE_MATERIAL_API)
    # The thickness here, not the `widths` above, is what sizes the capsules and
    # their inertia; without it the importer assumes a millimeter and says so.
    for name, value in (("curvesThickness", thickness), *CABLE_MATERIAL_ATTRS.items()):
        Sdf.AttributeSpec(material, f"physics:{name}", Sdf.ValueTypeNames.Float).default = value
    # Bound rather than merely authored: the importer reads the stiffnesses off
    # the *bound* material and silently uses its own defaults without this.
    Sdf.RelationshipSpec(
        spec, "material:binding:physics", custom=False
    ).targetPathList.explicitItems = [material.path]

    Sdf.AttributeSpec(
        spec, "physics:masses", Sdf.ValueTypeNames.FloatArray
    ).default = Vt.FloatArray(_cable_point_masses(points, thickness))


def _cable_point_masses(
    points: Sequence[tuple[float, float, float]], thickness: float
) -> list[float]:
    """Per-point masses that bolt a cable's first segment down and weigh the rest.

    Authoring these is the whole of the anchor. A control point is a junction
    rather than a body -- the importer lumps ``m[s] + m[s+1]/2`` onto the segment
    between two of them -- so zeroing the first two leaves the first segment
    massless, which Newton simulates as **static**: fixed in position *and* in
    orientation, the way a shoot is held where it leaves the wood.

    The schema's own anchor, a ``PhysicsAttachment``, is the worse tool here. It
    lowers only to *ball* joints, so one pins a position and leaves the curve
    pivoting about it, and two are needed to hold a direction. They are also
    compliant: past roughly 1e10 Pa a stiff rod overpowers them and the base
    swings free again. A massless body has no such ceiling, and costs no prim.

    Every other point carries the density times half of each segment it joins,
    which lumps back to the cylinder mass of each. The second segment is the one
    exception, left at half weight because the point below it is one of the
    zeroed pair -- an edge effect on the segment next to a rigid one.
    """
    volume = math.pi * (thickness / 2.0) ** 2
    spans = [math.dist(a, b) for a, b in zip(points, points[1:])]
    density = CABLE_MATERIAL_ATTRS["density"]
    return [
        0.0
        if index < _BOLTED_POINTS
        else density
        * volume
        * 0.5
        * ((spans[index - 1] if index else 0.0) + (spans[index] if index < len(spans) else 0.0))
        for index in range(len(points))
    ]
