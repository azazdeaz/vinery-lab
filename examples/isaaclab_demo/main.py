"""Walk a quadruped down the alleys of a generated vineyard.

The vineyard and the quadruped are set up here; the route comes from `route`,
the walking from `driver`, and the debug markers that show what the follower
is doing from `markers`.

The physics backend and the viewer are command-line choices, e.g.
`--physics newton_mjwarp --viz newton`; `--help` lists the full launcher set.
"""

import argparse

import numpy as np

import isaaclab.sim as sim_utils
from isaaclab.app import add_launcher_args, launch_simulation, make_physics_cfg
from isaaclab.assets import Articulation
from isaaclab.utils.assets import ISAAC_NUCLEUS_DIR

from vinerylab.isaaclab import ParcelCfg, TerrainCfg, VineyardCfg

from driver import DECIMATION, SIM_DT, Driver
from markers import DebugMarkers
from newton_patches import fix_heightfield_offsets
from route import alley_route

##
# Pre-defined configs
##
from isaaclab_assets.robots.anymal import ANYMAL_C_CFG  # isort:skip


# The scene is generated on first use and cached on these parameters, so a
# second run of this script spawns it without re-running the generator.
VINEYARD_CFG = VineyardCfg(
    terrain=TerrainCfg(height=22.0, width=22.0, max_elevation=0.9),
    parcel=ParcelCfg(orientation=-14.0, row_spacing=2.0),
)
VINEYARD_PATH = "/World/Vineyard"
ROBOT_PATH = "/World/Robot"


def parse_args() -> argparse.Namespace:
    """This script's arguments, on top of Isaac Lab's launcher ones."""
    parser = argparse.ArgumentParser(description="This script drives a quadruped through a generated vineyard.")
    parser.add_argument(
        "--physics",
        default="physx",
        help="Physics backend: physx, isaacsim_physx, newton_mjwarp, newton_vbd or ovphysx.",
    )
    # Adds --device, --visualizer/--viz, --livestream and the rest; it wants the
    # script's own arguments registered first.
    add_launcher_args(parser)
    # demos should open Kit visualizer by default
    parser.set_defaults(visualizer=["kit"])
    args = parser.parse_args()
    # A single articulation steps faster on the CPU under PhysX; Newton is a
    # GPU solver. Either way, an explicit --device wins.
    if not getattr(args, "device_explicit", False):
        args.device = "cpu" if "physx" in args.physics else "cuda:0"
    return args


def design_scene() -> Articulation:
    """The vineyard, a sky, and one quadruped to walk it."""
    # HDR dome light (IBL + visible sky). Outdoor locomotion envs use this map.
    cfg = sim_utils.DomeLightCfg(
        intensity=750.0,
        texture_file=f"{ISAAC_NUCLEUS_DIR}/Materials/Textures/Skies/PolyHaven/kloofendal_43d_clear_puresky_4k.hdr",
    )
    cfg.func("/World/Light", cfg)

    # The generated scene brings its own colliders: the ground as its own mesh,
    # the posts and trunks as capsules.
    VINEYARD_CFG.func(VINEYARD_PATH, VINEYARD_CFG)

    return Articulation(ANYMAL_C_CFG.replace(prim_path=ROBOT_PATH))


def run_simulator(sim: sim_utils.SimulationContext, robot: Articulation, route: np.ndarray):
    """Runs the simulation loop."""
    driver = Driver(route, robot)
    markers = DebugMarkers(route)
    driver.place()

    step = 0
    while sim.is_headless_or_exist_active_visualizer():
        if step % DECIMATION == 0:
            driver.control()
            markers.show(robot, driver.target, driver.command)
        robot.write_data_to_sim()
        sim.step()
        robot.update(SIM_DT)
        step += 1


def main():
    args_cli = parse_args()
    sim_cfg = sim_utils.SimulationCfg(dt=SIM_DT, device=args_cli.device, physics=make_physics_cfg(args_cli.physics))
    # Starts Isaac Sim when the chosen backend or viewer needs it, and closes it on exit.
    with launch_simulation(sim_cfg, args_cli):
        sim = sim_utils.SimulationContext(sim_cfg)
        route = alley_route(VINEYARD_CFG)
        robot = design_scene()
        # Look down the first alley from behind the robot's start.
        sim.set_camera_view(eye=(route[0] + [4.0, 4.0, 3.0]).tolist(), target=route[0].tolist())
        # Play the simulator
        sim.reset()
        fix_heightfield_offsets()
        # Now we are ready!
        print(f"[INFO]: Setup complete, {len(route)} waypoints to walk...")
        # Run the simulator
        run_simulator(sim, robot, route)


if __name__ == "__main__":
    main()
