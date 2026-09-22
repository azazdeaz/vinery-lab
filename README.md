:warning: Developer preview. Expect breaking changes.

[![CI](https://github.com/azazdeaz/vinery-lab/actions/workflows/ci.yml/badge.svg)](https://github.com/azazdeaz/vinery-lab/actions/workflows/ci.yml)

# Vinery Lab :grapes:

Open-source parametric vineyard generator for developing and testing vineyard robots.

![Configured scene in Isaac Lab](https://github.com/user-attachments/assets/3eafecd5-9707-407f-a500-ec52634abfd1)
*Configured scene in Isaac Lab*

<br>


![Configured scene in Isaac Lab](https://github.com/user-attachments/assets/37b38b86-edd5-49aa-8c8d-58143415bb86)
*Standalone parameter editor and visualizer app*


> Try the parameter editor in your browser: [azazdeaz.github.io/vinery-lab](https://azazdeaz.github.io/vinery-lab/) — bit slower and needs WebGPU.



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
 - Complete example projects demonstrating task executions
 - **Share what you need for your project :rocket:**


## Parameter editor app

Run the parameter editor

> Requires [Rust](https://www.rust-lang.org/tools/install) installed

```bash
cargo run --release
```

This should bring up the preview app to configure the vineyard

https://github.com/user-attachments/assets/c00b227f-74c6-4446-bd41-2c487d7f5605



## Isaac Lab examples

Navigate the rows with a quadruped. See its [README](examples/isaaclab_demo/README.md) for more detail.
```bash
cd examples/isaaclab_demo/
uv run main.py
```

The same demo on the PhysX backend (the stray shoots spawn static there)
```bash
uv run main.py --physics physx
```

The same demo on MJWarp alone, the fastest of the backends (the stray shoots spawn static there too)
```bash
uv run main.py --physics newton_mjwarp
```

Drive a straddling robot down every row. See its [README](examples/straddler_demo/README.md) for more detail.
```bash
cd examples/straddler_demo/
uv run main.py
```


## How it works (main points)

 - The vineyard can be composed with a [`VineyardCfg`](python/vinerylab/isaaclab/vineyard_cfg.py) object,
 which is a standard Isaac Lab `FileCfg` config class. See the [example](examples/isaaclab_demo/main.py#L43).
 - Every parameter, with its default and range, is listed in [docs/parameters.md](docs/parameters.md).
 - The options are many, so prefer to use the parameter editor GUI and copy the configuration snippet to your script.
 - When the simulation starts, the meshes and layouts are generated a USD file and cached for the next run.
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

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT), at your option.

## Development

See [AGENTS.md](AGENTS.md) for the repo map, and
[docs/development.md](docs/development.md) for building and running the checks.
