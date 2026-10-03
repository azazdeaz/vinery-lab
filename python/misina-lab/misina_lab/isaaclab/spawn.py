"""Generating, caching and spawning a generated scene's USD.

`GeneratedSceneCfg` is the base of every generator's spawner cfg. A subclass
adds one field per fragment, mirroring the generator's `*Params` pyclasses,
and names as class attributes the aggregate they convert to and the table of
them the conversion and the cache key walk; `spawn_generated` does the rest.

The scene is written to a real file rather than composed from an in-memory
`Sdf.Layer`. An anonymous layer would have to be pinned for the process
lifetime to keep the composition alive, and its identifier does not resolve in
a new process, so any stage save would leave a dangling reference. A file path
is owned by the stage, survives cloning and serialization, and lets Isaac Lab's
own USD-file spawn path do the rest.

The shape here follows `spawn_from_urdf`: produce a USD file, then delegate to
the undecorated `_spawn_from_usd_file`.

The cache holds `.usd` (binary crate) -- about a third the bytes of the text
form and roughly 4x faster for USD to parse.
"""

from __future__ import annotations

import functools
import hashlib
import json
import logging
import os
import pathlib
import sys
import tempfile
from collections.abc import Callable
from dataclasses import MISSING
from types import ModuleType
from typing import TYPE_CHECKING, ClassVar, Self

from filelock import FileLock
from isaaclab.sim import SimulationContext
from isaaclab.sim.spawners.from_files.from_files_cfg import FileCfg
from isaaclab.sim.utils import clone
from isaaclab.utils.configclass import configclass

from .rods import steps_rods, tune_rods

if TYPE_CHECKING:
    from pxr import Usd

SCENE_SUFFIX = ".usd"
"""Extension the cached scene is written with -- see the module docstring."""

logger = logging.getLogger(__name__)


@configclass
class GeneratedSceneCfg(FileCfg):
    """Spawn a generated scene: the base a generator's cfg subclasses.

    The scene is generated on first use and cached as a USD file keyed on the
    fragments, then spawned through Isaac Lab's ordinary USD-file path -- so
    everything `FileCfg` offers (`scale`, `semantic_tags`, `rigid_props`,
    `collision_props`, visual materials, contact sensors) applies here too,
    and the prim path may be an env regex. Everything on the cfg that is not a
    fragment is applied to the spawned prim rather than baked into the USD,
    and takes no part in the cache key.

    There is no `usd_path`: the fragments are what identifies the asset, and
    the file backing it is an implementation detail of the cache.

    The fragments are plain `@configclass` dataclasses rather than the
    pyclasses themselves on purpose: a pyclass has no `__dict__`, which is
    what `isaaclab.utils.dict.class_to_dict` dispatches on, and it cannot be
    deep-copied -- so holding one on a cfg would break `cfg.to_dict()`,
    `cfg.replace()` and every YAML/hydra round-trip Isaac Lab does with a
    scene config. `to_params` converts them at spawn time instead.
    """

    PARAMS: ClassVar[type] = MISSING  # type: ignore[assignment]
    """The generator's params aggregate, a pyclass of its extension module:
    `boxlab.BoxesParams`. A subclass sets it; `configclass` needs every
    annotation to carry a value, so the base holds `MISSING`."""

    FRAGMENTS: ClassVar[tuple[str, ...]] = ()
    """The fragment fields by name, in the order `PARAMS` takes them. Generated
    beside the fields themselves."""

    PLANT: ClassVar[str | None] = None
    """Prim name of the plant a rod belongs to -- `Plant` for `Plant_007` --
    for a scene that can author one. Everything under one plant's prim collides
    as a group and passes through every other plant; see `rods.tune_rods`."""

    func: Callable | str = "misina_lab.isaaclab.spawn:spawn_generated"
    """Fully qualified rather than the `{DIR}` form: Isaac Lab resolves `{DIR}`
    against the module of the class that first declared the field, and a
    subclass in another package would inherit that resolution."""

    cache_dir: str | None = None
    """Where generated scenes are cached. Defaults to ``$<PACKAGE>_CACHE_DIR``,
    else ``$XDG_CACHE_HOME/<package>/scenes``, else ``~/.cache/<package>/scenes``,
    for the package `PARAMS` comes from -- `BOXLAB_CACHE_DIR` and
    ``~/.cache/boxlab/scenes`` for `boxlab`."""

    force_regenerate: bool = False
    """Regenerate even on a cache hit. For iterating on the generator itself."""

    def without_rods(self) -> Self | None:
        """This cfg with nothing flexible in it, or `None` when it authors no
        rod as it stands.

        Under a backend that cannot step a rod the spawner spawns the answer
        in place of the cfg; a generator whose scene can hold one overrides
        this. The default authors none.
        """
        return None


@clone
def spawn_generated(
    prim_path: str,
    cfg: GeneratedSceneCfg,
    translation: tuple[float, float, float] | None = None,
    orientation: tuple[float, float, float, float] | None = None,
    **kwargs,
) -> Usd.Prim:
    """Spawn a generated scene, generating it first if it isn't cached.

    Decorated with :func:`clone`, so a regex prim path such as
    ``{ENV_REGEX_NS}/Boxes`` spawns once and is copied to every matching
    parent -- the generation cost is paid once regardless of ``num_envs``.

    The scene is checked against the physics backend in force. Where nothing
    steps a rod, a scene authored with any is spawned without them -- the same
    shapes, as static meshes -- and where something does, the rods are tuned
    for it; see :func:`for_backend`.

    Args:
        prim_path: The prim path or pattern to spawn the scene at.
        cfg: The configuration instance.
        translation: Translation w.r.t. the parent prim. Defaults to None.
        orientation: Orientation ``(x, y, z, w)`` w.r.t. the parent prim.
            Defaults to None.
        **kwargs: Forwarded to the USD-file spawn path.

    Returns:
        The prim of the spawned scene.
    """
    # Imported here, not at module level: it pulls in `pxr`, and Kit's own copy
    # of `pxr` only wins the import if nothing loaded the pip one before Kit started.
    from isaaclab.sim.spawners.from_files.from_files import _spawn_from_usd_file

    cfg = for_backend(cfg)
    return _spawn_from_usd_file(
        prim_path, resolve_usd_path(cfg), cfg, translation, orientation, **kwargs
    )


def for_backend(cfg: GeneratedSceneCfg) -> GeneratedSceneCfg:
    """The cfg to spawn under the physics backend in force.

    With rods and a backend that steps one, `cfg` itself, and the rods are
    tuned for it (`tune_rods`). With one that cannot -- MJWarp refuses a model
    holding a rod, PhysX ignores the curve -- what `cfg.without_rods()` gives,
    which is its own scene in the cache. `cfg` itself again with nothing to
    decide: no rod authored, or no simulation running.
    """
    sim = SimulationContext.instance()
    if sim is None:
        return cfg
    static = cfg.without_rods()
    if static is None:
        return cfg
    physics = sim.cfg.physics
    if steps_rods(physics):
        if cfg.PLANT is None:
            raise TypeError(
                f"{type(cfg).__name__} authors rods but names no PLANT to group them by"
            )
        tune_rods(cfg.PLANT)
        return cfg
    solver = getattr(physics, "solver_cfg", physics)
    logger.warning(
        "%s cannot bend a rod: the scene is spawned with its rods as static meshes.",
        type(solver).__name__,
    )
    return static


def resolve_usd_path(cfg: GeneratedSceneCfg) -> str:
    """The cached USD for `cfg`, generating it on a miss.

    Concurrent callers -- distributed training ranks sharing a cache dir --
    are serialized on a lock file, and the scene is written to a temporary
    name and moved into place, so a partially written file is never visible
    under the final path.
    """
    directory = _cache_dir(cfg)
    directory.mkdir(parents=True, exist_ok=True)
    usd_path = directory / f"{_stem(cfg)}_{_fingerprint(cfg)}{SCENE_SUFFIX}"

    if usd_path.exists() and not cfg.force_regenerate:
        return str(usd_path)

    with FileLock(str(usd_path) + ".lock"):
        # Re-check: another rank may have generated it while we waited.
        if usd_path.exists() and not cfg.force_regenerate:
            return str(usd_path)

        handle, tmp_name = tempfile.mkstemp(
            dir=directory, prefix=usd_path.stem + ".", suffix=SCENE_SUFFIX
        )
        os.close(handle)
        tmp_path = pathlib.Path(tmp_name)
        # `write_usd` authors a new layer at this path; an existing empty file
        # would only get in its way.
        tmp_path.unlink()
        try:
            to_params(cfg).write_usd(str(tmp_path))
            os.replace(tmp_path, usd_path)
        except BaseException:
            tmp_path.unlink(missing_ok=True)
            raise

    return str(usd_path)


def to_params(cfg: GeneratedSceneCfg):
    """The cfg's geometry fragments as the pyclasses the generator takes."""
    return cfg.PARAMS(
        **{
            name: params_cls(**getattr(cfg, name).to_dict())
            for name, params_cls in fragment_params(type(cfg)).items()
        }
    )


def fragment_params(cfg_cls: type[GeneratedSceneCfg]) -> dict[str, type]:
    """Each fragment's pyclass by field name: `terrain` maps to what a default
    aggregate holds under `terrain`, so no name is looked up anywhere."""
    defaults = cfg_cls.PARAMS()
    return {name: type(getattr(defaults, name)) for name in cfg_cls.FRAGMENTS}


def _fingerprint(cfg: GeneratedSceneCfg) -> str:
    """A cache key over the geometry fragments and the generator that reads them.

    Only the fragments in `FRAGMENTS` take part. The rest of the cfg --
    `rigid_props`, `scale`, `semantic_tags`, `visible`, `spawn_path` -- is
    applied to the prim after spawning and does not change the USD, so
    including it would throw away cache hits for nothing.
    """
    payload = {name: getattr(cfg, name).to_dict() for name in cfg.FRAGMENTS}
    payload["__generator__"] = _generator_id(_core(cfg))
    digest = hashlib.sha256(json.dumps(payload, sort_keys=True).encode())
    return digest.hexdigest()[:16]


@functools.cache
def _generator_id(core: ModuleType) -> str:
    """Identifies the generator, so a changed one doesn't read a stale cache.

    Three things decide what lands on disk: the extension module that solves
    the scene, the builder here that authors it as USD, and the OpenUSD build
    that writes the file -- a run under Kit and a kitless one author with
    different ones. All three are keyed: a change to any produces a different
    file from the same parameters.

    The extension's version alone is not enough during development, where a
    rebuild changes a generator without touching it. Size and mtime do change
    every time, which errs toward regenerating -- the safe direction.
    """
    # Imported here, not at module level: see `spawn_generated`.
    from pxr import Usd

    from ..usd import build

    fields = [core.__version__, ".".join(map(str, Usd.GetVersion()))]
    for module in (core, build):
        stat = pathlib.Path(module.__file__).stat()
        fields += [str(stat.st_size), str(stat.st_mtime_ns)]
    return "-".join(fields)


def _core(cfg: GeneratedSceneCfg) -> ModuleType:
    """The extension module `PARAMS` came from: `boxlab._core` for `BoxesParams`."""
    return sys.modules[cfg.PARAMS.__module__]


def _package(cfg: GeneratedSceneCfg) -> str:
    """The generator's package, `boxlab`: the one its extension module sits in."""
    return _core(cfg).__name__.rpartition(".")[0]


def _stem(cfg: GeneratedSceneCfg) -> str:
    """What a cached file is named after: `boxes` for `BoxesParams`."""
    return cfg.PARAMS.__name__.removesuffix("Params").lower()


def _cache_dir(cfg: GeneratedSceneCfg) -> pathlib.Path:
    if cfg.cache_dir is not None:
        return pathlib.Path(cfg.cache_dir)
    package = _package(cfg)
    if override := os.environ.get(f"{package.upper()}_CACHE_DIR"):
        return pathlib.Path(override)
    base = os.environ.get("XDG_CACHE_HOME") or (pathlib.Path.home() / ".cache")
    return pathlib.Path(base) / package / "scenes"
