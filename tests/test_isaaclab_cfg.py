"""Tests for the Isaac Lab spawner config.

Requires Isaac Lab; skipped entirely where it isn't installed, so the rest of
the suite still runs. Everything here is parameterized over the one
`FRAGMENTS` list the implementation itself walks, so an element added there is
covered without new test code.
"""

from __future__ import annotations

import copy
import dataclasses
import math
import os

import pytest

isaaclab_cfg = pytest.importorskip(
    "vinerylab.isaaclab.vineyard_cfg", reason="Isaac Lab is not installed"
)
vineyard = pytest.importorskip("vinerylab.isaaclab.vineyard")

import vinerylab  # noqa: E402

FRAGMENTS = isaaclab_cfg.FRAGMENTS
VineyardCfg = isaaclab_cfg.VineyardCfg


def params_attrs(params_cls: type) -> set[str]:
    """The settable fields of a `*Params` pyclass.

    PyO3 exposes `get_all`/`set_all` fields as class-level descriptors and has
    no `__dict__` to read them from, so they are recovered off the class with
    the dunders and methods filtered out.
    """
    return {
        name
        for name in dir(params_cls)
        if not name.startswith("_") and not callable(getattr(params_cls, name, None))
    }


@pytest.fixture
def cfg() -> VineyardCfg:
    """A cfg with something moved in more than one fragment."""
    return VineyardCfg(
        parcel=isaaclab_cfg.ParcelCfg(row_spacing=2.8),
        vine=isaaclab_cfg.VineCfg(arms=1, seed=42),
    )


# ─── The cfg mirrors the generator ──────────────────────────────────


@pytest.mark.parametrize("name,cfg_cls", FRAGMENTS, ids=[n for n, _ in FRAGMENTS])
def test_fragment_fields_match_the_generator(name, cfg_cls):
    """Every fragment names exactly the fields its pyclass takes.

    The cfg classes are hand-written mirrors of Rust structs, so they go stale
    silently: a field added on the Rust side is simply unreachable from Isaac
    Lab, and one removed there is accepted here and then rejected at spawn
    time with a `TypeError` deep inside the conversion.
    """
    assert {f.name for f in dataclasses.fields(cfg_cls)} == params_attrs(
        vineyard._params_class(cfg_cls)
    )


@pytest.mark.parametrize("name,cfg_cls", FRAGMENTS, ids=[n for n, _ in FRAGMENTS])
def test_fragment_defaults_match_the_generator(name, cfg_cls):
    """An untouched cfg generates the same scene as untouched params.

    Compared with an f32 tolerance: the params are `f32` in Rust, so a default
    written here as `2.4` reads back as `2.4000000953674316`.
    """
    from_cfg, from_params = cfg_cls(), vineyard._params_class(cfg_cls)()
    for field in dataclasses.fields(cfg_cls):
        ours, theirs = getattr(from_cfg, field.name), getattr(from_params, field.name)
        if isinstance(theirs, float):
            assert math.isclose(ours, theirs, rel_tol=1e-6), field.name
        else:
            assert ours == theirs, field.name


def test_to_params_round_trips_every_fragment(cfg):
    params = vineyard.to_params(cfg)
    assert math.isclose(params.parcel.row_spacing, 2.8, rel_tol=1e-6)
    assert params.vine.arms == 1
    assert params.vine.seed == 42
    # Untouched fragments still arrive at their defaults rather than missing.
    assert params.terrain.detail == isaaclab_cfg.TerrainCfg().detail


# ─── The cfg survives what Isaac Lab does to configs ────────────────


def test_cfg_survives_config_machinery(cfg):
    """`to_dict`, `deepcopy` and `replace` all work.

    These are the three that fail if a pyclass or an `Sdf.Layer` is held on
    the cfg -- a pyclass has no `__dict__` for `class_to_dict` to walk and
    cannot be pickled -- and they are what Isaac Lab does to a scene config
    for YAML logging, hydra overrides and checkpoint replay.
    """
    assert math.isclose(cfg.to_dict()["parcel"]["row_spacing"], 2.8, rel_tol=1e-6)
    assert copy.deepcopy(cfg).vine.arms == 1
    assert cfg.replace(vine=isaaclab_cfg.VineCfg(arms=2)).vine.arms == 2
    # replace leaves the original alone
    assert cfg.vine.arms == 1


def test_func_resolves_to_the_spawner(cfg):
    """The lazy `{DIR}` form points at `spawn_vineyard` and stays a string in
    `to_dict()`, so a dumped config is still YAML."""
    assert str(cfg.func) == "vinerylab.isaaclab.vineyard:spawn_vineyard"
    assert isinstance(cfg.to_dict()["func"], str)


# ─── Caching ────────────────────────────────────────────────────────


def test_cache_is_keyed_on_geometry_only(cfg, tmp_path):
    """A second call reuses the file; geometry changes it; prim-level
    properties do not.

    `rigid_props` and friends are applied to the spawned prim afterwards and
    never reach the USD, so letting them into the key would throw away hits
    for nothing.
    """
    cfg.cache_dir = str(tmp_path)

    first = vineyard.resolve_usd_path(cfg)
    stamp = os.stat(first).st_mtime_ns

    assert vineyard.resolve_usd_path(cfg) == first
    assert os.stat(first).st_mtime_ns == stamp, "a cache hit must not rewrite the file"

    moved = cfg.replace(parcel=isaaclab_cfg.ParcelCfg(row_spacing=3.5))
    moved.cache_dir = str(tmp_path)
    assert vineyard.resolve_usd_path(moved) != first

    from isaaclab.sim.schemas import RigidBodyBaseCfg

    decorated = cfg.replace(rigid_props=RigidBodyBaseCfg(kinematic_enabled=True))
    decorated.cache_dir = str(tmp_path)
    assert vineyard.resolve_usd_path(decorated) == first


def test_generated_scene_is_a_usable_asset(cfg, tmp_path):
    """What lands in the cache opens, and vines are addressable in it."""
    from pxr import Usd

    cfg.cache_dir = str(tmp_path)
    stage = Usd.Stage.Open(vineyard.resolve_usd_path(cfg))
    assert stage.GetDefaultPrim().GetName() == "Vineyard"
    assert stage.GetPrimAtPath("/Vineyard/Planting/Row_000/Vine_000")


def test_cached_scene_composes_when_referenced(cfg, tmp_path):
    """Referencing the cached file gives a complete asset under `/World`.

    This is what a file path buys over the anonymous layer it replaces, and
    it is what `_spawn_from_usd_file` does under the hood. Worth asserting
    directly because the failure mode is silent: a prototype that does not
    compose through the reference leaves prims present but empty, and the
    scene renders as nothing at all.
    """
    from pxr import Usd

    cfg.cache_dir = str(tmp_path)
    usd_path = vineyard.resolve_usd_path(cfg)

    stage = Usd.Stage.CreateInMemory()
    prim = stage.DefinePrim("/World/Vineyard", "Xform")
    assert prim.GetReferences().AddReference(usd_path)

    vine = stage.GetPrimAtPath("/World/Vineyard/Planting/Row_000/Vine_000")
    assert vine, "the referenced subtree is reachable under its new root"
    assert vine.GetChildren(), (
        "the vine composes its prototype's geometry, rather than arriving as"
        " an empty prim -- the silent failure a dropped reference produces"
    )
