"""Prune a dormant vineyard with a Bumblebee-like robot.

A skid-steer UGV drives the alley beside a row of bare winter canes, stops at
each vine, and prunes it with the arm on its slide: every cane cut back to two
buds, each cut approached from a stand-off on a straight line and made by the
shear closing on the cane. The robot is `bumblebee`, the driving `driver`,
and the planning -- from the scene's own ground truth, not from a sensor --
and the cutting `pruner`.

The row pruned, the physics backend and the viewer are command-line choices;
`--help` lists the full launcher set. The default backend is Newton coupled
with VBD, the one that bends and cuts a cane -- see `vinerylab.isaaclab
.physics`. Under any other the canes spawn static and nothing can be cut.
"""

import argparse

import numpy as np
import torch

import isaaclab.sim as sim_utils
from isaaclab.app import add_launcher_args, launch_simulation
from isaaclab.assets import Articulation
from isaaclab.utils.assets import ISAAC_NUCLEUS_DIR

from vinerylab.isaaclab import (
    CoverCfg,
    ParcelCfg,
    ShootCfg,
    TerrainCfg,
    VineCfg,
    VineyardCfg,
    make_physics_cfg_newton,
)
from vinerylab.isaaclab.newton_patches import fix_heightfield_offsets
from vinerylab.isaaclab.physics import steps_rods

from bumblebee import (
    ARM_HOME,
    ARM_JOINTS,
    HAND,
    SHEAR_JOINT,
    SLIDE_JOINT,
    Bumblebee,
    bumblebee_cfg,
)
from driver import DECIMATION, SIM_DT, Driver
from pruner import Pruning, Tally, Vine, nearest_first, plan, read_vines

# The scene is generated on first use and cached on these parameters, so a
# second run of this script spawns it without re-running the generator.
VINEYARD_CFG = VineyardCfg(
    # A small, nearly flat plot: two rows are all the robot works, and every
    # cane on them is a rod, which is what the size is paying for.
    terrain=TerrainCfg(length=18.0, width=6.0, max_inclination=3.0, feature_size=14.0),
    parcel=ParcelCfg(row_spacing=2.4, trellis_height=1.5, headland=2.0, min_row_length=4.0),
    # One cane per spur, so pruning to two buds leaves a two-bud spur.
    vine=VineCfg(shoots_per_spur=1.0),
    # Winter: bare lignified canes with buds a dormant internode apart.
    shoot=ShootCfg(dormant=True, internode=0.12, length=1.1, radius=0.005, lean=0.08),
    # A dry, short winter floor.
    cover=CoverCfg(dryness=0.6, height=0.06),
)

VINEYARD_PATH = "/World/Vineyard"
ROBOT_PATH = "/World/Robot"

# The parts of the robot a cane may bend: the arm, which pushes through the
# canes around the one it is cutting. Each costs a proxy in the solver that
# bends them, so the chassis stays out.
ROBOT_CONTACT = [rf"{ROBOT_PATH}/.*(shoulder_lift|elbow|wrist_[123])_link"]

# Bumblebee's own figures, to read the tally against.
PAPER = "Bumblebee: 87% of cuts made at the right place, 213 s per vine, 68% reachable"


def parse_args() -> argparse.Namespace:
    """This script's arguments, on top of Isaac Lab's launcher ones."""
    parser = argparse.ArgumentParser(
        description="This script prunes a row of a generated dormant vineyard with a UGV-mounted arm."
    )
    parser.add_argument(
        "--row",
        type=int,
        default=0,
        help="The row to prune, numbered as the scene names them -- Row_000 first.",
    )
    parser.add_argument(
        "--physics",
        help=(
            "An Isaac Lab backend in place of the default: physx, isaacsim_physx, "
            "ovphysx or newton_mjwarp. None of them bends a cane, so none can cut one."
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


def design_scene(machine: Bumblebee) -> Articulation:
    """The vineyard, a sky, and the robot."""
    cfg = sim_utils.DomeLightCfg(
        intensity=750.0,
        texture_file=f"{ISAAC_NUCLEUS_DIR}/Materials/Textures/Skies/PolyHaven/kloofendal_43d_clear_puresky_4k.hdr",
    )
    cfg.func("/World/Light", cfg)
    VINEYARD_CFG.func(VINEYARD_PATH, VINEYARD_CFG)
    return Articulation(bumblebee_cfg(machine, ROBOT_PATH))


def row_geometry(stage, row: int) -> tuple[float, np.ndarray]:
    """The heading the robot drives the alley beside `row` in, and the
    direction from that alley to the row.

    The row on the robot's left, so the alley is on the side of the row that
    its next row is on -- or the other side for the last row.
    """
    from pxr import Usd, UsdGeom

    rows = stage.GetPrimAtPath(f"{VINEYARD_PATH}/Planting").GetChildren()
    if not 0 <= row < len(rows):
        raise ValueError(f"no row {row}: the vineyard has {len(rows)} rows")

    def posts(prim) -> np.ndarray:
        return np.array(
            [
                UsdGeom.Xformable(post)
                .ComputeLocalToWorldTransform(Usd.TimeCode.Default())
                .ExtractTranslation()[:2]
                for post in prim.GetChildren()
                if post.GetName().startswith("Pole_")
            ]
        )

    line = posts(rows[row])
    along = line[-1] - line[0]
    along /= np.linalg.norm(along)
    left = np.array([-along[1], along[0]])
    neighbour = posts(rows[row + 1 if row + 1 < len(rows) else row - 1]).mean(axis=0)
    # The alley is on the neighbour's side of the row, so the robot faces the
    # way that puts the neighbour on its right and the row on its left.
    if (neighbour - line[0]) @ left > 0:
        along, left = -along, -left
    return float(np.arctan2(along[1], along[0])), left


def hand_pose(robot: Articulation, body: int) -> np.ndarray:
    """The world frame of one of the robot's bodies, as a transform."""
    return frame_of(
        robot.data.body_link_pos_w.torch[0, body].cpu().numpy(),
        robot.data.body_link_quat_w.torch[0, body].cpu().numpy(),
    )


def base_pose(robot: Articulation) -> np.ndarray:
    """The world frame of the robot's base link, as a transform."""
    return frame_of(
        robot.data.root_pos_w.torch[0].cpu().numpy(), robot.data.root_quat_w.torch[0].cpu().numpy()
    )


def frame_of(position: np.ndarray, quat: np.ndarray) -> np.ndarray:
    """A position and an (x, y, z, w) rotation as a transform."""
    x, y, z, w = quat
    frame = np.eye(4)
    frame[:3, :3] = np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
            [2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
            [2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)],
        ]
    )
    frame[:3, 3] = position
    return frame


def run_simulator(
    sim: sim_utils.SimulationContext,
    robot: Articulation,
    machine: Bumblebee,
    driver: Driver,
    vines: list[Vine],
    to_row: np.ndarray,
    shears,
):
    """Drive from vine to vine, pruning each, then stand at the end."""
    stage = sim_utils.get_current_stage()
    arm, _ = robot.find_joints([SLIDE_JOINT, *ARM_JOINTS], preserve_order=True)
    shear, _ = robot.find_joints([SHEAR_JOINT])
    hand, _ = robot.find_bodies([HAND])
    root_shift = np.array([*(-to_row * VINEYARD_CFG.parcel.row_spacing / 2), 0.0])
    stops = [vine.position[:2] + root_shift[:2] for vine in vines]

    driver.place(stops[0] - 1.5 * driver.forward)
    driver.stop = stops[0]
    # The arm rides folded between vines: a drive with no target holds zero,
    # which is the arm lying out along the chassis.
    home = torch.tensor([[0.0, *ARM_HOME]], dtype=torch.float32, device=robot.device)
    robot.set_joint_position_target_index(target=home, joint_ids=arm)
    tally = Tally()
    pruning: Pruning | None = None
    started = 0.0
    step = 0
    while sim.is_headless_or_exist_active_visualizer():
        if step % DECIMATION == 0:
            q = robot.data.joint_pos.torch[0, arm].cpu().numpy()
            if pruning is None:
                if driver.control() and stops and shears is not None:
                    vine = vines[len(vines) - len(stops)]
                    base = base_pose(robot)
                    mouth = hand_pose(robot, hand[0])[:3, 3]
                    cuts = nearest_first(plan(vine, shears, np.array([*to_row, 0.0])), mouth)
                    reachable = tally.reachable
                    pruning = Pruning(
                        machine.chain,
                        machine.tool,
                        machine.mouth,
                        cuts,
                        base,
                        q,
                        shears,
                        stage,
                        tally,
                    )
                    started = step * SIM_DT
                    print(
                        f"[INFO]: {vine.prim}: {len(cuts)} cuts planned,"
                        f" {tally.reachable - reachable} reachable"
                    )
            else:
                targets, close = pruning.control(
                    q,
                    hand_pose(robot, hand[0]),
                    float(robot.data.joint_pos.torch[0, shear[0]]),
                    machine.shear.opening,
                )
                robot.set_joint_position_target_index(
                    target=torch.tensor(
                        targets, dtype=torch.float32, device=robot.device
                    ).unsqueeze(0),
                    joint_ids=arm,
                )
                robot.set_joint_position_target_index(
                    target=torch.full(
                        (1, 1), 0.0 if close else machine.shear.opening, device=robot.device
                    ),
                    joint_ids=shear,
                )
                if pruning.done:
                    tally.vines += 1
                    tally.seconds += step * SIM_DT - started
                    print(f"[INFO]: {tally.summary()}")
                    pruning = None
                    robot.set_joint_position_target_index(target=home, joint_ids=arm)
                    stops.pop(0)
                    driver.stop = stops[0] if stops else None
                    if not stops:
                        print(f"[INFO]: done. {tally.summary()}\n[INFO]: {PAPER}")
        robot.write_data_to_sim()
        sim.step()
        robot.update(SIM_DT)
        step += 1


def main():
    args_cli = parse_args()
    sim_cfg = sim_utils.SimulationCfg(
        dt=SIM_DT,
        device=args_cli.device,
        physics=make_physics_cfg_newton(VINEYARD_CFG, ROBOT_PATH, ROBOT_CONTACT),
    )
    # Starts Isaac Sim when the chosen backend or viewer needs it, and closes it
    # on exit. An explicit --physics replaces the config built above.
    with launch_simulation(sim_cfg, args_cli):
        # Kit ships the movie capture window but does not load it, so turn it
        # on to record the run from Window > Movie Capture.
        if "kit" in args_cli.visualizer:
            sim_utils.enable_extension("omni.kit.window.movie_capture")
        sim = sim_utils.SimulationContext(sim_cfg)
        machine = Bumblebee()
        robot = design_scene(machine)
        # Imported here: `vinerylab.usd` pulls in `pxr`, and Kit's own copy
        # only wins the import if nothing loaded the pip one first.
        from vinerylab.usd import Ground

        stage = sim_utils.get_current_stage()
        heading, to_row = row_geometry(stage, args_cli.row)
        sim.reset()
        fix_heightfield_offsets()
        # The robot's joints exist once the simulation has been reset.
        driver = Driver(robot, machine, heading, Ground(stage))

        shears = None
        if steps_rods(sim.cfg.physics):
            from vinerylab.isaaclab import Shears

            shears = Shears()
        else:
            print("[WARN]: the backend bends no cane, so nothing can be cut")
        vines = read_vines(stage, VINEYARD_PATH, args_cli.row, shears) if shears else []
        # In driving order down the alley.
        vines.sort(key=lambda vine: vine.position[:2] @ driver.forward)
        if not vines:
            raise RuntimeError(f"row {args_cli.row} has no cane to prune")
        first = vines[0].position
        sim.set_camera_view(
            eye=(first + [*(-4.0 * driver.forward - 2.5 * to_row), 2.5]).tolist(),
            target=(first + [0.0, 0.0, 1.0]).tolist(),
        )
        print(f"[INFO]: Setup complete, {len(vines)} vines to prune...")
        run_simulator(sim, robot, machine, driver, vines, to_row, shears)


if __name__ == "__main__":
    main()
