"""Generating, caching and spawning the vineyard USD.

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
import os
import pathlib
import tempfile
from typing import TYPE_CHECKING

from filelock import FileLock
from isaaclab.sim.spawners.from_files.from_files import _spawn_from_usd_file
from isaaclab.sim.utils import clone

import vinerylab
import vinerylab._core
import vinerylab.usd.build

from .vineyard_cfg import FRAGMENTS

if TYPE_CHECKING:
    from pxr import Usd

    from .vineyard_cfg import VineyardCfg

SCENE_SUFFIX = ".usd"
"""Extension the cached scene is written with -- see the module docstring."""


@clone
def spawn_vineyard(
    prim_path: str,
    cfg: VineyardCfg,
    translation: tuple[float, float, float] | None = None,
    orientation: tuple[float, float, float, float] | None = None,
    **kwargs,
) -> Usd.Prim:
    """Spawn a generated vineyard, generating it first if it isn't cached.

    Decorated with :func:`clone`, so a regex prim path such as
    ``{ENV_REGEX_NS}/Vineyard`` spawns once and is copied to every matching
    parent -- the generation cost is paid once regardless of ``num_envs``.

    Args:
        prim_path: The prim path or pattern to spawn the vineyard at.
        cfg: The configuration instance.
        translation: Translation w.r.t. the parent prim. Defaults to None.
        orientation: Orientation ``(x, y, z, w)`` w.r.t. the parent prim.
            Defaults to None.
        **kwargs: Forwarded to the USD-file spawn path.

    Returns:
        The prim of the spawned vineyard.
    """
    return _spawn_from_usd_file(
        prim_path, resolve_usd_path(cfg), cfg, translation, orientation, **kwargs
    )


def resolve_usd_path(cfg: VineyardCfg) -> str:
    """The cached USD for `cfg`, generating it on a miss.

    Concurrent callers -- distributed training ranks sharing a cache dir --
    are serialized on a lock file, and the scene is written to a temporary
    name and moved into place, so a partially written file is never visible
    under the final path.
    """
    directory = _cache_dir(cfg)
    directory.mkdir(parents=True, exist_ok=True)
    usd_path = directory / f"vineyard_{_fingerprint(cfg)}{SCENE_SUFFIX}"

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


def to_params(cfg: VineyardCfg) -> vinerylab.VineyardParams:
    """The cfg's geometry fragments as the pyclasses the generator takes."""
    return vinerylab.VineyardParams(
        **{
            name: _params_class(fragment_cls)(**getattr(cfg, name).to_dict())
            for name, fragment_cls in FRAGMENTS
        }
    )


def _params_class(fragment_cls: type) -> type:
    """`TerrainCfg` -> `vinerylab.TerrainParams`."""
    return getattr(vinerylab, fragment_cls.__name__.removesuffix("Cfg") + "Params")


def _fingerprint(cfg: VineyardCfg) -> str:
    """A cache key over the geometry fragments and the generator that reads them.

    Only the fragments in `FRAGMENTS` take part. The rest of the cfg --
    `rigid_props`, `scale`, `semantic_tags`, `visible`, `spawn_path` -- is
    applied to the prim after spawning and does not change the USD, so
    including it would throw away cache hits for nothing.
    """
    payload = {name: getattr(cfg, name).to_dict() for name, _ in FRAGMENTS}
    payload["__generator__"] = _generator_id()
    digest = hashlib.sha256(json.dumps(payload, sort_keys=True).encode())
    return digest.hexdigest()[:16]


@functools.cache
def _generator_id() -> str:
    """Identifies the generator, so a changed one doesn't read a stale cache.

    Two modules decide what lands on disk: the Rust extension that solves the
    scene, and the Python module that authors it as USD. Both are keyed --
    a change to either produces a different file from the same parameters.

    The version alone is not enough during development, where `maturin
    develop` or an edit changes a generator without touching it. Size and
    mtime do change every time, which errs toward regenerating -- the safe
    direction.
    """
    fields = [vinerylab.__version__]
    for module in (vinerylab._core, vinerylab.usd.build):
        stat = pathlib.Path(module.__file__).stat()
        fields += [str(stat.st_size), str(stat.st_mtime_ns)]
    return "-".join(fields)


def _cache_dir(cfg: VineyardCfg) -> pathlib.Path:
    if cfg.cache_dir is not None:
        return pathlib.Path(cfg.cache_dir)
    if override := os.environ.get("VINERYLAB_CACHE_DIR"):
        return pathlib.Path(override)
    base = os.environ.get("XDG_CACHE_HOME") or (pathlib.Path.home() / ".cache")
    return pathlib.Path(base) / "vinerylab" / "scenes"
