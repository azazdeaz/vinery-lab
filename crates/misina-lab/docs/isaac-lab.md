# Isaac Lab

`misina_lab.isaaclab` spawns a generated scene as an ordinary Isaac Lab scene
asset. A generator's cfg is a `@configclass` extending `GeneratedSceneCfg`,
with one field per fragment mirroring its `*Params` classes. On first use
the spawner converts the fragments to params, generates the scene, caches the
USD under a key built from the fragments and the generator, and spawns the
file through Isaac Lab's own USD-file path. Everything `FileCfg` offers
applies, and the prim path may be an env regex: the scene generates once
however many envs there are.

```python
from typing import ClassVar

import boxlab
from isaaclab.utils.configclass import configclass
from misina_lab.isaaclab import GeneratedSceneCfg


@configclass
class BoxesCfg(GeneratedSceneCfg):
    PARAMS: ClassVar[type] = boxlab.BoxesParams

    # >>> generated: aggregate
    boxes: BoxCfg = BoxCfg()
    FRAGMENTS: ClassVar[tuple[str, ...]] = ("boxes",)
    # <<< generated: aggregate
```

The fragment classes and the marked region are generated from the Rust
structs ([editing-parameters.md](editing-parameters.md)); the class around
them is hand-written.

## The contract

| Attribute | What it says |
| --- | --- |
| `PARAMS` | the aggregate pyclass, `boxlab.BoxesParams`. Everything else is derived from it: each fragment's class off a default aggregate, the extension module from `PARAMS.__module__`, the package from the module's path |
| `FRAGMENTS` | the fragment fields by name, in the order the aggregate takes them; what `to_params` converts and the cache key is built from |
| `PLANT` | the prim name a rod belongs to, `Plant` for `Plant_007`, for a scene that can author one; everything under one plant's prim collides as a group |
| `without_rods()` | this cfg with nothing flexible in it, or `None` when it authors no rod as it stands; the default authors none |
| `cache_dir`, `force_regenerate` | where the files go, `$<PACKAGE>_CACHE_DIR` or `~/.cache/<package>/scenes` by default, and whether to skip a hit |

The fragments are dataclasses rather than the pyclasses themselves: a
pyclass has no `__dict__` for `class_to_dict` to walk and cannot be
deep-copied, so holding one on a cfg would break `to_dict`, `replace` and
every YAML round trip Isaac Lab does with a scene config. Two things
`configclass` insists on: every annotation carries a value, so the base
holds `MISSING` for `PARAMS`; and a class attribute must not hold a
dataclass *class*, even inside a tuple, which is why `FRAGMENTS` is names.

## The cache

The key is a hash over the fragments' `to_dict()` and the generator's
identity: the extension's version, the size and mtime of its file and of the
USD builder's, and the OpenUSD version. The rest of the cfg, `rigid_props`,
`scale`, `semantic_tags`, is applied to the prim after spawning and does not
change the USD, so it takes no part. Concurrent callers sharing a cache
directory, distributed training ranks, serialize on a lock file, and the file
is written to a temporary name and moved into place. The scene is a `.usd`
file rather than an anonymous layer because a path survives cloning,
serialization and a new process.

## Rods

Nothing in a generated scene is a rigid body except a flexible organ
authored as a `Cable` ([export.md](export.md#colliders)), imported by Newton
as a **rod**: one capsule body per segment, joined by rod joints and clamped
to the wood it grew from. Only Newton's VBD solver steps one, and a robot
needs MuJoCo, so a scene with any runs the two side by side as entries of a
coupled solver, `rigid` and `rods`, with the robot's bodies handed across as
proxies. `make_physics_cfg_newton(cfg, robot, contact_bodies, substeps)`
picks between that and plain MJWarp by asking `cfg.without_rods()`.

The choice is provisional, an override can replace the backend before the
scene is built, so the spawner checks the cfg against the backend in force
when it runs. Where nothing steps a rod the scene is spawned from
`without_rods()` instead, the same shapes as static meshes. Where something
does, `tune_rods(cfg.PLANT)` registers a hook on the model build that sets
every rod joint's stiffness and damping and groups what collides: each
plant's rods and wood share a positive group of their own, and the ground,
the trellis and the robot are negative, so a rod meets its own plant, the
scene and the robot and passes through every other plant. Without the
grouping the model holds a candidate pair for every two segments in the
scene. The tuning numbers were measured on a metre-long, centimetre-thick
rod; `rods.py` says what each one does and what went wrong at other values.

`Shears` cuts a rod anywhere along it while the simulation runs. Newton
sizes its arrays when the model is finalized and the CUDA graph holds
pointers into them, so a cut only writes values: the capsule the cut lands
on is shortened, the next one is stretched back to start there, the mass
moves with the length, and the joint between them goes. Everything past the
cut falls away as a chain of its own. Build one after the first reset and
keep it; it tracks which rods are cut.

## Where it lives

- [`misina_lab/isaaclab/spawn.py`](../../../python/misina-lab/misina_lab/isaaclab/spawn.py): `GeneratedSceneCfg`, `spawn_generated`, `resolve_usd_path`, `to_params`, and the cache key
- [`misina_lab/isaaclab/rods.py`](../../../python/misina-lab/misina_lab/isaaclab/rods.py): `make_physics_cfg_newton`, `tune_rods`, and the numbers
- [`misina_lab/isaaclab/cutting.py`](../../../python/misina-lab/misina_lab/isaaclab/cutting.py): `Shears`
- [`src/scene/mod.rs`](../src/scene/mod.rs): `cable`, what the Rust side authors
