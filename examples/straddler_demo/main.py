"""Drive a straddling robot down every row of a generated vineyard.

The robot is an inverted U on four swerve modules: it drives along a row with
a leg in the alley either side and the trellis passing under its frame. The
vineyard and the robot are set up here; the robot itself is in `straddler`,
the route in `route` and the driving in `driver`.

The row the drive starts on, the physics backend and the viewer are
command-line choices, e.g. `--row 3 --physics newton_mjwarp --viz newton`;
`--help` lists the full launcher set.
The default backend is Newton coupled with VBD, which bends the vineyard's
flexible shoots -- see `vinerylab.isaaclab.physics`. Under any other the stray
shoots are spawned static.

`--trim` hangs a hedger under the frame, which cuts every stray shoot reaching
across it -- see `straddler.Trimmer` and `vinerylab.isaaclab.cutting`.
"""

import argparse
import dataclasses
from collections.abc import Callable

import numpy as np

import isaaclab.sim as sim_utils
from isaaclab.app import add_launcher_args, launch_simulation
from isaaclab.assets import Articulation
from isaaclab.utils.assets import ISAAC_NUCLEUS_DIR

from vinerylab.isaaclab import (
    ParcelCfg,
    ShootCfg,
    TerrainCfg,
    VineyardCfg,
    make_physics_cfg_newton,
)
from vinerylab.isaaclab.physics import steps_rods

from driver import DECIMATION, SIM_DT, Driver
from newton_patches import fix_heightfield_offsets
from route import row_route
from straddler import Straddler, Trimmer, set_finish, straddler_cfg

# The scene is generated on first use and cached on these parameters, so a
# second run of this script spawns it without re-running the generator.
VINEYARD_CFG = VineyardCfg(
    # Gentle relief: a gantry carries its frame two metres up, and a slope
    # steep enough for a quadruped to enjoy is one this would lean on.
    terrain=TerrainCfg(length=36.0, width=26.0, max_inclination=6.0, feature_size=14.0),
    # A low trellis, so the frame that clears it is a height a real vineyard
    # straddler is built to.
    parcel=ParcelCfg(orientation=-8.0, row_spacing=2.4, trellis_height=1.5),
    # A few stray shoots reaching into the alley for the robot to push
    # through. They bend under the default backend, and are spawned static
    # under any other.
    shoot=ShootCfg(stray=0.05),
)

# With a trimmer aboard, a vineyard that needs one: a fifth of its shoots
# strayed out of the canopy, where the bars reach them.
TRIM_VINEYARD_CFG = dataclasses.replace(VINEYARD_CFG, shoot=ShootCfg(stray=0.2))

# # Long straight rows with a lot of stray shoots
# VINEYARD_CFG = VineyardCfg(
#     terrain=TerrainCfg(width=24.0),
#     shoot=ShootCfg(stray=0.18),
# )

VINEYARD_PATH = "/World/Vineyard"
ROBOT_PATH = "/World/Robot"

# The parts of the robot a shoot may bend. The whole machine straddles the
# canopy, so all of it is in reach -- and there are only nine bodies, each of
# which costs a proxy in the solver that bends them.
ROBOT_CONTACT = [rf"{ROBOT_PATH}/.*"]


def parse_args() -> argparse.Namespace:
    """This script's arguments, on top of Isaac Lab's launcher ones."""
    parser = argparse.ArgumentParser(
        description="This script drives a straddling robot down the rows of a generated vineyard."
    )
    parser.add_argument(
        "--row",
        type=int,
        default=0,
        help=(
            "The row to start the drive on, numbered as the scene names them -- Row_000 "
            "first -- and counting from the last row if negative. Every row is driven "
            "whichever one it starts on; this only picks where it begins."
        ),
    )
    parser.add_argument(
        "--trim",
        action="store_true",
        help=(
            "Hang a hedger under the frame: a cutter bar either side of the row that "
            "cuts every stray shoot reaching across it. Only the default backend bends, "
            "and so cuts, a shoot."
        ),
    )
    parser.add_argument(
        "--physics",
        help=(
            "An Isaac Lab backend in place of the default: physx, isaacsim_physx, "
            "ovphysx or newton_mjwarp. None of them bends a shoot; the strays spawn static."
        ),
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
        args.device = "cpu" if "physx" in (args.physics or "") else "cuda:0"
    return args


def design_scene(vineyard: VineyardCfg, machine: Straddler) -> Articulation:
    """The vineyard, a sky, and one straddler to drive it."""
    # HDR dome light (IBL + visible sky). Outdoor locomotion envs use this map.
    cfg = sim_utils.DomeLightCfg(
        intensity=750.0,
        texture_file=f"{ISAAC_NUCLEUS_DIR}/Materials/Textures/Skies/PolyHaven/kloofendal_43d_clear_puresky_4k.hdr",
    )
    cfg.func("/World/Light", cfg)

    # The generated scene brings its own colliders: the ground as its own mesh,
    # the posts and trunks as capsules.
    vineyard.func(VINEYARD_PATH, vineyard)

    robot = Articulation(straddler_cfg(machine, ROBOT_PATH))
    # The URDF gave its materials their colours; the rest of the surface has
    # nowhere in a URDF to come from, so it is set on the spawned prims.
    set_finish(ROBOT_PATH)
    return robot


def trimming(
    sim: sim_utils.SimulationContext, machine: Straddler, robot: Articulation, ground
) -> Callable[[], None] | None:
    """What the trimmer does at each control step: cut whatever crosses its
    bars where they are now, and lay what it cut down where it lands.

    None under a backend that bends no shoot, where there is nothing to cut.
    Call once the simulation is reset, since that is what builds the rods.
    """
    if not steps_rods(sim.cfg.physics):
        print("[WARN]: the backend bends no shoot, so the trimmer has nothing to cut")
        return None
    import torch
    from isaaclab.utils.math import quat_apply

    from vinerylab.isaaclab.cutting import Shears

    shears = Shears()
    # (bar, corner/along/up, xyz) in the base frame.
    bars = torch.tensor(machine.bars, dtype=torch.float32, device=robot.device)

    def trim():
        pose = robot.data.root_quat_w.torch[0].expand(bars.numel() // 3, 4)
        world = quat_apply(pose, bars.reshape(-1, 3)).reshape(bars.shape)
        world[:, 0] += robot.data.root_pos_w.torch[0]
        for corner, along, up in world.cpu().numpy():
            shears.cut_through(corner, along, up)
        shears.settle(ground.height)

    return trim


def run_simulator(
    sim: sim_utils.SimulationContext,
    robot: Articulation,
    driver: Driver,
    trim: Callable[[], None] | None = None,
):
    """Runs the simulation loop."""
    driver.place()

    step = 0
    while sim.is_headless_or_exist_active_visualizer():
        if step % DECIMATION == 0:
            driver.control()
            if trim is not None:
                trim()
        robot.write_data_to_sim()
        sim.step()
        robot.update(SIM_DT)
        step += 1


def main():
    args_cli = parse_args()
    vineyard = TRIM_VINEYARD_CFG if args_cli.trim else VINEYARD_CFG
    sim_cfg = sim_utils.SimulationCfg(
        dt=SIM_DT,
        device=args_cli.device,
        physics=make_physics_cfg_newton(vineyard, ROBOT_PATH, ROBOT_CONTACT),
    )
    # Starts Isaac Sim when the chosen backend or viewer needs it, and closes it
    # on exit. An explicit --physics replaces the config built above.
    with launch_simulation(sim_cfg, args_cli):
        # Kit ships the movie capture window but does not load it, so turn it
        # on to record the drive from Window > Movie Capture. Only Kit has a
        # viewport to capture, and only a running Kit can be asked for it.
        if "kit" in args_cli.visualizer:
            sim_utils.enable_extension("omni.kit.window.movie_capture")
        sim = sim_utils.SimulationContext(sim_cfg)
        route, heading, ground = row_route(vineyard, args_cli.row)
        # The robot is sized to the field it works: a leg in each alley, and
        # the frame over the trellis wire.
        machine = Straddler.for_vineyard(vineyard)
        if args_cli.trim:
            machine = dataclasses.replace(machine, trimmer=Trimmer())
        robot = design_scene(vineyard, machine)
        # Behind the robot at its start, looking the way it drives off. Taken
        # from the route rather than from `heading`, which is the row direction
        # only: alternate passes run down it backwards, and so does a start row
        # chosen with --row.
        back = route[0, :2] - route[1, :2]
        sim.set_camera_view(
            eye=(route[0] + [*(6.0 * back / np.linalg.norm(back)), 4.0]).tolist(),
            target=route[0].tolist(),
        )
        # Play the simulator
        sim.reset()
        fix_heightfield_offsets()
        # Now we are ready!
        print(f"[INFO]: Setup complete, {len(route)} waypoints to drive...")
        # Run the simulator
        trim = trimming(sim, machine, robot, ground) if machine.trimmer else None
        run_simulator(sim, robot, Driver(route, heading, machine, robot, ground), trim)


if __name__ == "__main__":
    main()
