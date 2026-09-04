:construction: This project is still under development.

## Project description

This is a parametric vineyard generator. It generates USD scenes for robotics simulation. Mainly targeting Isaac Lab.

<img width="2490" height="1471" alt="Screenshot from 2026-09-03 19-46-54" src="https://github.com/user-attachments/assets/ab6b0ed5-0165-4ae7-adcd-b4bd4752887a" />


## Features
 - GUI based vineyard configurator
 - Leaves are modelled as detailed meshes to enable depth perception based workflows
 - Performance tuning. LoD and mesh variance are configurable to support low-end hardware and large vineyards
 - Every plant, shoot and leaf is an addressable prim, while the meshes behind them are shared
 - Ships with static colliders — the ground as its own mesh, posts and trunks as capsules

## Upcoming features
 - Cover crops and weeds
 - Optinally use PointInstancer to spawn organs without a unique prim path
 - Flexible shoot simulation with newton-physics
 - Simulate human workers and other safety critical scenarios

## Planned features
 - Reconstruct real vineyard parcels from EU vineyard-register data, public orthophotos, and regional DTMs
 - Support multiple vine-training systems
 - RTK-GNSS data generation
 - GeoJSON and TASKDATA.xml export

## Quick commands to demo

Run the editor
```bash
cargo run --release
```

Generate and run in Isaac Lab
```bash
cd examples/isaaclab_demo/
uv run main.py
```

## Workflow
 - Start the viewer `cargo run --release`.
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



https://github.com/user-attachments/assets/0b71d0a7-6847-4c83-9d3f-a9ab9ea2a4cb





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
