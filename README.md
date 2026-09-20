:construction: This project is still under development.

[![CI](https://github.com/azazdeaz/vinery-lab/actions/workflows/ci.yml/badge.svg)](https://github.com/azazdeaz/vinery-lab/actions/workflows/ci.yml)

# Vinery Lab :grapes:

Parametric vineyard generator for robotics simulation. Mainly targeting Isaac Lab.

Running the configured scene in Isaac Lab
<img width="2401" height="1073" alt="image" src="https://github.com/user-attachments/assets/3eafecd5-9707-407f-a500-ec52634abfd1" />


Parameter editor and visualizer app
<img width="1850" height="974" alt="image" src="https://github.com/user-attachments/assets/24ea9490-20b5-49a5-a547-a2f96910a381" />

> Try the parameter editor in your browser: [azazdeaz.github.io/vinery-lab](https://azazdeaz.github.io/vinery-lab/) — bit slower, needs WebGPU, and no wireframe view.


## Features
 - Parameter configurator with live preview
 - Fully reproducible scene generation
 - Leaves are modelled as detailed meshes to enable depth perception based workflows
 - Performance tuning. LoD and mesh variance are configurable to support low-end hardware and large vineyards
 - Every plant, shoot and leaf is an addressable prim
 - Flexible stray shoots that the robot can push aside
 - Cover crops and weeds

## Upcoming features
 - Optionally use PointInstancer to spawn organs without a unique prim path
 - Simulate human workers and other safety critical scenarios
 - Install from PyPI
 - More detailed cover crop and weed definitions

## Planned features
 - Reconstruct real vineyard parcels from EU vineyard-register data, public orthophotos, and regional DTMs
 - Support multiple vine-training systems
 - RTK-GNSS data generation
 - GeoJSON and TASKDATA.xml export
 - **Share what you need for your project :rocket:**

## Quick commands to demo

Run the parameter editor
```bash
cargo run --release
```

Generate and run in Isaac Lab
```bash
cd examples/isaaclab_demo/
uv run main.py
```

The same demo on the PhysX backend (it doesn't support flexible shoots)
```bash
uv run main.py --physics physx
```

Drive a straddling robot down every row
```bash
cd examples/straddler_demo/
uv run main.py
```


## How it works (main points)

 - The vineyard can be composed with a [`VineyardCfg`](python/vinerylab/isaaclab/vineyard_cfg.py) object, which is a standard Isaac Lab FileCfg config class. See the [example](examples/isaaclab_demo/main.py#L43).
 - Every parameter, with its default and range, is listed in [docs/parameters.md](docs/parameters.md).
 - The options are many, so prefer to use the parameter editor GUI and copy the configuration snippet to your script.
 - When the simulation starts, the meshes and layouts are generated and cached as a USD file.
 - The cached USD file is then spawned as a regular Isaac Lab USD file asset.


## Workflow
 - Start the parameter editor with `cargo run --release`.
 - Edit the scene parameters in the UI.
 - Press **Copy Isaac Lab cfg** to put the current settings on the clipboard as
   a `VineyardCfg(...)` construction — only the fields you moved.
 - Paste it into your Isaac Lab environment config and spawn it:

```python
from vinerylab.isaaclab import VineyardCfg, ParcelCfg, SceneCfg, VineCfg

VINEYARD_CFG = VineyardCfg(
    scene=SceneCfg(seed=42),
    parcel=ParcelCfg(row_spacing=2.8, vine_spacing=1.05),
    vine=VineCfg(arms=1),
)

# a plain script, or a direct env's `_setup_scene()`
VINEYARD_CFG.func("/World/Vineyard", VINEYARD_CFG)

# or a manager-based scene config
vineyard = AssetBaseCfg(prim_path="/World/Vineyard", spawn=VINEYARD_CFG)
```


https://github.com/user-attachments/assets/09b202da-6a36-403b-b541-5625c0605ce7


> The scene is generated on first use and cached as a USD file keyed on those
parameters, so only the first run pays for it — and an env regex prim path
(`{ENV_REGEX_NS}/Vineyard`) generates once and clones, whatever `num_envs` is.
`VineyardCfg` is a `FileCfg`, so `scale`, `semantic_tags`, `rigid_props`,
`collision_props` and visual materials all work on it as they would on a
`UsdFileCfg`. See `examples/isaaclab_demo/main.py`.

> The scene arrives solid: the ground collides as its own mesh, and every post
and trunk carries a capsule. Nothing else does — a robot walks through the
canopy — and nothing is a rigid body, so a vineyard stands where it was put.

> Cached scenes live in `$VINERYLAB_CACHE_DIR`, else
`$XDG_CACHE_HOME/vinerylab/scenes`, else `~/.cache/vinerylab/scenes`; set
`cache_dir` on the cfg to override, or `force_regenerate=True` while iterating
on the generator itself.


## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT), at your option.

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for implementation details and development guidelines.
