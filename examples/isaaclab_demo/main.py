"""Walk a quadruped down the alleys of a generated vineyard.

The vineyard and the quadruped are set up here; the route comes from `route`,
the walking from `driver`, and the debug markers that show what the follower
is doing from `markers`.

Launch Isaac Sim Simulator first.
"""

import argparse

from isaaclab.app import AppLauncher

# add argparse arguments
parser = argparse.ArgumentParser(description="This script drives a quadruped through a generated vineyard.")
# append AppLauncher cli args
AppLauncher.add_app_launcher_args(parser)
# demos should open Kit visualizer by default
parser.set_defaults(visualizer=["kit"])
# parse the arguments
args_cli = parser.parse_args()

# launch omniverse app
app_launcher = AppLauncher(args_cli)
simulation_app = app_launcher.app

"""Rest everything follows."""

import numpy as np

import isaaclab.sim as sim_utils
from isaaclab.assets import Articulation
from isaaclab.sim.utils.stage import get_current_stage
from isaaclab.utils.assets import ISAAC_NUCLEUS_DIR

from vinerylab.isaaclab import ParcelCfg, TerrainCfg, VineyardCfg
from vinerylab.usd import GEOM

from driver import DECIMATION, SIM_DT, Driver
from markers import DebugMarkers
from route import alley_route

##
# Pre-defined configs
##
from isaaclab_assets.robots.anymal import ANYMAL_C_CFG  # isort:skip


# The scene is generated on first use and cached on these parameters, so a
# second run of this script spawns it without re-running the generator.
VINEYARD_CFG = VineyardCfg(
    terrain=TerrainCfg(height=82.0, width=82.0, max_elevation=8.9),
    parcel=ParcelCfg(orientation=-14.0, row_spacing=2.0),
)
VINEYARD_PATH = "/World/Vineyard"
ROBOT_PATH = "/World/Robot"


def design_scene() -> Articulation:
    """The vineyard, a sky, and one quadruped to walk it."""
    # HDR dome light (IBL + visible sky). Outdoor locomotion envs use this map.
    cfg = sim_utils.DomeLightCfg(
        intensity=750.0,
        texture_file=f"{ISAAC_NUCLEUS_DIR}/Materials/Textures/Skies/PolyHaven/kloofendal_43d_clear_puresky_4k.hdr",
    )
    cfg.func("/World/Light", cfg)

    VINEYARD_CFG.func(VINEYARD_PATH, VINEYARD_CFG)
    # The generated scene carries no physics schemas at all, so the ground has
    # to be made solid here or the robot drops through it. Every part is
    # spawned instanceable, and nothing may be authored inside an instance, so
    # the terrain gives up its instancing first -- there is one ground, so it
    # was sharing its prototype with nobody.
    # ponytail: terrain only -- do the same for the posts and trunks when the
    # robot needs something to bump into.
    get_current_stage().GetPrimAtPath(f"{VINEYARD_PATH}/Terrain").SetInstanceable(False)
    sim_utils.define_collision_properties(
        f"{VINEYARD_PATH}/Terrain/{GEOM}", sim_utils.CollisionPropertiesCfg(collision_enabled=True)
    )

    return Articulation(ANYMAL_C_CFG.replace(prim_path=ROBOT_PATH))


def run_simulator(sim: sim_utils.SimulationContext, robot: Articulation, route: np.ndarray):
    """Runs the simulation loop."""
    driver = Driver(route, robot)
    markers = DebugMarkers(route)
    driver.place()

    step = 0
    while simulation_app.is_running():
        if step % DECIMATION == 0:
            driver.control()
            markers.show(robot, driver.target, driver.command)
        robot.write_data_to_sim()
        sim.step()
        robot.update(SIM_DT)
        step += 1


def main():
    """Main function."""
    sim = sim_utils.SimulationContext(sim_utils.SimulationCfg(dt=SIM_DT))
    route = alley_route(VINEYARD_CFG)
    robot = design_scene()
    # Look down the first alley from behind the robot's start.
    sim.set_camera_view(eye=(route[0] + [4.0, 4.0, 3.0]).tolist(), target=route[0].tolist())
    # Play the simulator
    sim.reset()
    # Now we are ready!
    print(f"[INFO]: Setup complete, {len(route)} waypoints to walk...")
    # Run the simulator
    run_simulator(sim, robot, route)


if __name__ == "__main__":
    # run the main function
    main()
    # close sim app
    simulation_app.close()
