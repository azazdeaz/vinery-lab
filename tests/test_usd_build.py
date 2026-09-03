"""The Rust/Python contract: a scene document in, a correct USD stage out.

`tests/fixtures/tiny_scene.json` is a hand-written miniature of what the
generator emits -- nested structural prims, a `Scope` override, transforms at
several depths, and both kinds of part. It is deliberately *not* generated from
the Rust side: it pins the document format independently, so a change to either
end that breaks the agreement fails here rather than downstream in Isaac.

Everything asserted below is something that fails silently in a renderer if the
builder stops doing it -- see `vinerylab/usd/build.py`'s module docstring.
"""

from __future__ import annotations

import json
import pathlib

import pytest
from pxr import Gf, Usd, UsdGeom

from vinerylab.usd import GEOM, PARTS, ROOT, build_stage

FIXTURE = pathlib.Path(__file__).parent / "fixtures" / "tiny_scene.json"

VINE = f"{ROOT}/Planting/Row_00/Vine_000"
LEAF = f"{VINE}/Shoot_00/Leaf_00"


def part_mesh(stage: Usd.Stage, name: str) -> UsdGeom.Mesh:
    """The `Mesh` inside a part. A part is an Xform wrapping it, so that
    instancing a reference to the part shares the geometry."""
    return UsdGeom.Mesh(stage.GetPrimAtPath(f"{PARTS}/{name}/{GEOM}"))


@pytest.fixture(scope="module")
def doc() -> dict:
    return json.loads(FIXTURE.read_text())


@pytest.fixture
def stage(doc: dict, tmp_path: pathlib.Path) -> Usd.Stage:
    return build_stage(doc, str(tmp_path / "scene.usda"))


# --- stage metadata -------------------------------------------------


def test_stage_declares_the_documents_coordinate_convention(stage: Usd.Stage):
    """Root-layer-only metadata: a consumer cannot correct for a wrong one,
    and USD defaults to Y-up when it is unauthored."""
    assert UsdGeom.GetStageUpAxis(stage) == UsdGeom.Tokens.z
    assert UsdGeom.GetStageMetersPerUnit(stage) == 1.0


def test_the_scene_root_is_the_default_prim(stage: Usd.Stage):
    """So a consumer referencing the layer gets the whole asset."""
    default = stage.GetDefaultPrim()
    assert default.GetPath() == ROOT
    assert default.GetTypeName() == "Xform"


def test_the_parts_library_is_an_abstract_class_inside_the_root(stage: Usd.Stage):
    """`class` so a renderer doesn't draw the library as a pile at the origin;
    inside the root so relationship targets can be namespace-mapped later."""
    library = stage.GetPrimAtPath(PARTS)
    assert library.IsAbstract()
    assert library.GetTypeName() == "Scope"
    assert PARTS.startswith(f"{ROOT}/")


# --- the parts ------------------------------------------------------


def test_a_part_wraps_its_mesh_in_an_xform(stage: Usd.Stage):
    """The whole reason `instanceable` buys anything. An instance shares its
    *descendants* through a prototype while its own attributes stay on the
    instance, so a bare Mesh would share nothing at all."""
    assert stage.GetPrimAtPath(f"{PARTS}/Leaf_1").GetTypeName() == "Xform"
    assert part_mesh(stage, "Leaf_1").GetPrim().GetTypeName() == "Mesh"


def test_a_part_is_a_polygon_mesh_and_not_a_subdivision_cage(stage: Usd.Stage):
    """USD's default is catmullClark, which rounds the teeth off a leaf."""
    assert part_mesh(stage, "Leaf_1").GetSubdivisionSchemeAttr().Get() == UsdGeom.Tokens.none


def test_a_parts_faces_are_triangles_reconstructed_from_the_index_count(
    stage: Usd.Stage,
):
    mesh = part_mesh(stage, "Leaf_1")
    assert list(mesh.GetFaceVertexIndicesAttr().Get()) == [0, 1, 2]
    assert list(mesh.GetFaceVertexCountsAttr().Get()) == [3]
    assert len(mesh.GetPointsAttr().Get()) == 3


def test_a_part_bounds_its_own_points(stage: Usd.Stage):
    """`extent` is what a consumer frustum-culls against."""
    lo, hi = part_mesh(stage, "Vine_0").GetExtentAttr().Get()
    assert tuple(lo) == pytest.approx((-0.02, 0.0, 0.0))
    assert tuple(hi) == pytest.approx((0.02, 0.0, 0.9))


def test_every_part_carries_a_constant_display_color(stage: Usd.Stage):
    """Nothing binds a material, so this is the only channel a renderer reads;
    at the wrong interpolation it falls through to white."""
    for name, expected in [("Leaf_1", (0.24, 0.42, 0.16)), ("Vine_0", (0.31, 0.24, 0.18))]:
        primvar = part_mesh(stage, name).GetDisplayColorPrimvar()
        assert primvar.GetInterpolation() == UsdGeom.Tokens.constant
        assert tuple(primvar.Get()[0]) == pytest.approx(expected)


def test_optional_attributes_are_authored_only_where_the_document_has_them(
    stage: Usd.Stage,
):
    """A blade is a surface with no inside and has to be drawn from behind;
    wood is not, and must not pay for double-sided shading."""
    leaf = part_mesh(stage, "Leaf_1")
    vine = part_mesh(stage, "Vine_0")

    assert leaf.GetDoubleSidedAttr().Get() is True
    assert vine.GetDoubleSidedAttr().Get() is False

    assert vine.GetNormalsAttr().Get() is not None
    assert vine.GetNormalsInterpolation() == UsdGeom.Tokens.vertex
    assert leaf.GetNormalsAttr().Get() is None

    assert UsdGeom.PrimvarsAPI(leaf).HasPrimvar("st")
    assert not UsdGeom.PrimvarsAPI(vine).HasPrimvar("st")


# --- the prim tree --------------------------------------------------


def test_the_hierarchy_becomes_prim_paths(stage: Usd.Stage):
    """The paths are the product: a downstream Isaac config keys on them."""
    for path in [f"{ROOT}/Planting", f"{ROOT}/Planting/Row_00", VINE, LEAF]:
        assert stage.GetPrimAtPath(path).IsValid(), path


def test_a_grouping_prim_keeps_its_declared_type(stage: Usd.Stage):
    """A row carries no transform of its own -- its plants are each draped
    onto terrain a single row transform could not follow."""
    assert stage.GetPrimAtPath(f"{ROOT}/Planting/Row_00").GetTypeName() == "Scope"


def test_a_referencing_prim_takes_its_type_from_the_part(stage: Usd.Stage):
    """Defined typeless on purpose, so the part's own type composes in rather
    than competing with a local opinion."""
    wood = stage.GetPrimAtPath(f"{VINE}/Wood")
    assert wood.GetTypeName() == "Xform"
    assert wood.IsInstanceable()


def test_a_reference_puts_its_geometry_in_a_shared_prototype(stage: Usd.Stage):
    """The payoff: the mesh lives in the prototype, so thousands of instances
    cost one copy of the points between them. An instance's descendants are
    not addressable in exchange, which is why nothing is authored under one."""
    wood = stage.GetPrimAtPath(f"{VINE}/Wood")
    assert not wood.GetChildren(), "the instance itself exposes no descendants"

    prototype = wood.GetPrototype()
    assert prototype.IsValid()
    geom = UsdGeom.Mesh(prototype.GetChild(GEOM))
    assert len(geom.GetPointsAttr().Get()) == 3


def test_instances_of_one_part_share_a_single_prototype(stage: Usd.Stage):
    """Two prims referencing the same part must land on the same prototype, or
    the instancing is bookkeeping that saves nothing."""
    twin = stage.DefinePrim(f"{VINE}/Wood_Twin")
    twin.GetReferences().AddInternalReference(f"{PARTS}/Vine_0")
    twin.SetInstanceable(True)

    original = stage.GetPrimAtPath(f"{VINE}/Wood")
    assert twin.GetPrototype() == original.GetPrototype()


def test_a_transform_round_trips_through_the_op_stack(stage: Usd.Stage):
    xformable = UsdGeom.Xformable(stage.GetPrimAtPath(VINE))
    assert [op.GetOpName() for op in xformable.GetOrderedXformOps()] == [
        "xformOp:translate",
        "xformOp:orient",
        "xformOp:scale",
    ]
    translate, _, scale = xformable.GetOrderedXformOps()
    assert tuple(translate.Get()) == (1.0, 2.0, 0.0)
    assert tuple(scale.Get()) == (1.0, 1.0, 1.0)


def test_a_quaternion_keeps_its_real_part_in_usds_order(stage: Usd.Stage):
    """The document is xyzw and Gf.Quatf is real-first. Getting this backwards
    yields a scene that is plausibly wrong rather than obviously wrong."""
    _, orient, _ = UsdGeom.Xformable(
        stage.GetPrimAtPath(f"{VINE}/Shoot_00")
    ).GetOrderedXformOps()
    quat = orient.Get()

    assert quat.GetReal() == pytest.approx(0.7071068)
    assert tuple(quat.GetImaginary()) == pytest.approx((0.0, 0.0, 0.7071068))

    # A quarter turn about +Z takes +X onto +Y.
    turned = Gf.Rotation(Gf.Quatd(quat)).TransformDir(Gf.Vec3d(1, 0, 0))
    assert tuple(turned) == pytest.approx((0.0, 1.0, 0.0), abs=1e-6)


def test_a_deep_transform_composes_onto_its_ancestors(stage: Usd.Stage):
    """Vine at (1,2,0), shoot 0.8 up and turned a quarter turn, leaf 0.05
    along the shoot's local +X -- which the turn puts along world +Y."""
    world = UsdGeom.Xformable(stage.GetPrimAtPath(LEAF)).ComputeLocalToWorldTransform(
        Usd.TimeCode.Default()
    )
    assert tuple(world.ExtractTranslation()) == pytest.approx((1.0, 2.05, 1.0), abs=1e-6)


# --- guards ---------------------------------------------------------


def test_an_unknown_format_version_is_refused(doc: dict, tmp_path: pathlib.Path):
    """A stale cached document must fail loudly rather than compose into
    something subtly wrong."""
    with pytest.raises(ValueError, match="format"):
        build_stage({**doc, "format": 999}, str(tmp_path / "stale.usda"))


def test_the_stage_survives_a_round_trip_through_disk(doc: dict, tmp_path: pathlib.Path):
    """What Isaac Lab actually opens is the file, not the in-memory stage."""
    from vinerylab.usd import build_usd

    path = tmp_path / "scene.usda"
    build_usd(doc, str(path))
    assert path.exists()

    reopened = Usd.Stage.Open(str(path))
    assert UsdGeom.GetStageUpAxis(reopened) == UsdGeom.Tokens.z
    assert reopened.GetDefaultPrim().GetPath() == ROOT
    assert reopened.GetPrimAtPath(LEAF).IsInstanceable()
