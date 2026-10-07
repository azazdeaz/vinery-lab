"""Record a short video of the demo: the robot prunes one vine, each cut a
case of how the shear meets a cane, filmed close up in Isaac Sim's RTX
renderer by a camera this script moves. It runs headless.

    uv run video.py OUT.mp4

writes the frames to `OUT.mp4`, near-lossless, and the captions beside it as
`OUT.srt`, then fails if any take's cut was not made. `record.sh` runs it and
encodes the two into the video to upload.

The storyboard is the constants below: `TAKES`, which case each cut is and
how it is framed, the opening and closing shots, and `PACE`, how fast each
stage of a cut plays. Frames are sampled on the simulation's clock, one every
so many physics steps however long those take to compute, so the video plays
in simulated time.
"""

import argparse
import collections
import dataclasses
import pathlib
import subprocess

import numpy as np

import isaaclab.sim as sim_utils
from isaaclab.app import launch_simulation

import main as demo
from bumblebee import Bumblebee
from driver import SIM_DT
from pruner import KEEP_BUDS, Cane, Cut, Pruning, aimed, attached, nearest_first, plan

SIZE = (1920, 1080)
"""The frames, in pixels. `record.sh` scales them down to the video's size,
which smooths the thin canes."""

FPS = 25
"""Frames a second of video: a frame is 8 physics steps of simulated time,
at the simulation's own speed."""

PACE = {
    "move": 3.0,
    "approach": 2.0,
    "push": 2.0,
    "close": 3.0,
    "open": 2.0,
    "retract": 2.0,
    "tip": 3.0,
    "home": 3.0,
}
"""How many times slower than the simulation the video plays each stage of
a take's cut, by `Pruning.stage_name`: the arm swinging out to the cane,
closing in and pushing it, the blade closing, the blade opening again as the
piece falls, and the arm backing out and tipping out a piece that fell into
the mouth. A cut no take has is filmed wide, and only its swings -- out
through the canes, and tipping -- are slowed, as is the arm folding home;
the rest of those and the drive between vines play at the simulation's own
speed."""

SETTLE = 1.0
"""Seconds of simulated time not filmed at the start, while the robot drops
onto its wheels and the arm settles on its drives."""


@dataclasses.dataclass(frozen=True)
class Shot:
    """Where the camera stands and what it looks at, relative to a point it
    frames: in meters along the alley the way the robot drives, across it
    toward the row, and up."""

    eye: tuple[float, float, float]
    look: tuple[float, float, float] = (0.0, 0.0, 0.0)
    """Where it aims, from the point."""
    lens: float = 24.0
    """Focal length, in millimetres of a 21 mm wide sensor: 24 is a 47 degree
    view across, 50 a 24 degree one."""
    f_stop: float = 0.0
    """The aperture, focused on where the camera aims; 0 keeps everything
    sharp. The renderer's depth of field is strong for the number: at f/48 a
    close-up's background is already soft."""
    caption: str | None = None
    """What the shot is captioned with when no take's caption is up."""


@dataclasses.dataclass(frozen=True)
class Take:
    """One case of the shear meeting a cane: a cane of the vine, the cuts
    made on it and how they are filmed."""

    caption: str
    shot: Shot
    """Framing the middle of the take's cut points."""
    push: tuple[float, float, float] = (0.0, 0.0, 0.0)
    """Each cut's `Cut.push`."""
    kept: tuple[int, ...] = (KEEP_BUDS,)
    """How many buds the cane keeps after each cut, the cuts made in this
    order -- top down, so each has wood left under it to cut."""


# The close-ups are taken from the robot's side, a little over the mouth and
# behind it, on the side its moving blade opens to: clear of the arm, which
# comes in from the other side. From there the fixed blade is on the right.
CLOSE = Shot(eye=(-0.35, -0.3, 0.25), lens=35.0, f_stop=48.0)
"""The cuts' close-up."""

BEHIND = Shot(eye=(-0.45, -0.2, 0.25), lens=35.0, f_stop=48.0)
"""Further round behind the mouth: a cane pushed toward the row bends across
the picture."""

ACROSS = Shot(eye=(-0.7, 1.4, 0.6))
"""From across the row and back far enough for every point a take cuts the
cane at, which run down it against the robot's chassis."""

TAKES = (
    Take("A planned cut: the cane lies in the middle of the mouth", CLOSE),
    Take("One cane, cut three times from the top down", ACROSS, kept=(6, 4, 2)),
    Take("Pushed back into the crotch of the blades", BEHIND, push=(0.0, 0.0, 0.035)),
    Take("Pushed aside by the fixed blade before the cut", CLOSE, push=(0.0, -0.035, 0.0)),
    Take(
        "Pushed aside by the moving blade, which carries it across",
        CLOSE,
        push=(0.0, 0.035, 0.02),
    ),
)
"""The cases, each on its own cane, filmed in this order but for one rule:
those that push come after those that do not. A push carries the open blades
further than a planned cut does, through a neighbouring cane still standing,
and that cane's own take would find it cut already -- so the takes that push
go on every second cane along the cordon, after the canes between are cut.
A take of several cuts and no push goes on the cane between nearest the
robot, which the arm reaches past no other: a cane it brushes on the way can
be carried into the mouth beside the one cut. The vine's canes no take has are cut first,
filmed `REST` -- but for one left over among those every second, which comes
last."""

ROW, VINE = 1, 4
"""The row pruned, and which of its vines in driving order: away from the
headland, so there is vineyard behind every shot."""

OPENING = Shot(
    eye=(-5.5, -1.2, 1.8),
    look=(0.5, -0.8, 1.0),
    lens=35.0,
    caption="Pruning a dormant vine to two-bud spurs, in Isaac Sim",
)
"""Down the alley behind the robot as it drives up to the vine."""

REST = Shot(eye=(-1.5, -1.6, 2.2), look=(0.0, 0.0, 1.1), lens=28.0)
"""The vine, from over the robot: while the arm swings out to a take's cane,
brushing through the canes on its way, and while the canes no take has are
cut."""

CLOSING = Shot(
    eye=(-0.5, 2.0, 3.5), look=(0.8, -0.6, 0.7), caption="The vine pruned, its neighbours not yet"
)
"""High over the far alley: the vine pruned and its neighbours not, the
robot folding its arm and driving on."""

EASE = 1.5
"""Seconds of video the camera takes from one shot to the next."""

CUT = 1.0
"""How far the camera may travel from one shot to the next, in meters,
before it cuts to the next rather than easing there: a longer move would
pass through the arm or the row."""

ENDING = 6.0
"""Seconds of video after the vine's last cut: the arm folding home and the
robot driving on."""

CAMERA = "/World/VideoCamera"


def schedule(row: list[Cut], mouth: np.ndarray, side: np.ndarray) -> list[tuple[Cut, Take | None]]:
    """Which take each of a vine's canes is filmed for, if any, in the order
    to cut them. `row` is a planned cut on each cane, in order along the
    cordon, `mouth` where the shear starts out from, and `side` the
    horizontal direction from the robot to the row.

    The takes that push go on every second cane, after the canes between are
    cut, and one of several cuts and no push on the cane between nearest the
    robot: see `TAKES`. The other canes between are cut nearest neighbour first,
    those no take has first of all; then the takes that push, nearest
    neighbour first again, and any cane left over.
    """
    still = [take for take in TAKES if not any(take.push)]
    pushing = [take for take in TAKES if any(take.push)]
    between, spaced = row[1::2], row[0::2]
    if len(between) < len(still) or len(spaced) < len(pushing):
        raise RuntimeError(
            f"{len(row)} canes are too few for {len(still)} takes"
            f" and {len(pushing)} more that push on every second cane"
        )
    several = [take for take in still if len(take.kept) > 1]
    nearest = sorted(between, key=lambda cut: cut.pose[:3, 3] @ side)[: len(several)]
    between = nearest_first([cut for cut in between if all(cut is not n for n in nearest)], mouth)
    rest = len(between) - len(still) + len(several)
    singles, nearest = iter(between[rest:]), iter(nearest)
    order = [(cut, None) for cut in between[:rest]]
    order += [(next(nearest if take in several else singles), take) for take in still]
    spaced = nearest_first(spaced, order[-1][0].pose[:3, 3] if order else mouth)
    leftover = [(cut, None) for cut in spaced[len(pushing) :]]
    return order + list(zip(spaced, pushing, strict=False)) + leftover


class Rig:
    """The camera, easing from one shot to the next: eye, aim, focal length
    and aperture in one array, on a smoothstep over `EASE` seconds."""

    def __init__(self, stage):
        from pxr import UsdGeom

        self.prim = stage.GetPrimAtPath(CAMERA)
        self.camera = UsdGeom.Camera(self.prim)
        self.pose = self.start = self.goal = None
        self.eased = 1.0

    def aim(self, goal: np.ndarray) -> None:
        """Move towards `goal` by a frame, starting a new ease if it is a new
        goal -- or cutting to it, if it is `CUT` away -- and place the camera
        there."""
        from pxr import Gf, UsdGeom

        if self.goal is None or np.linalg.norm(goal[:3] - self.goal[:3]) > CUT:
            self.pose = self.start = self.goal = goal
        elif not np.allclose(goal, self.goal):
            self.start, self.goal, self.eased = self.pose, goal, 0.0
        self.eased = min(self.eased + 1 / (EASE * FPS), 1.0)
        s = self.eased * self.eased * (3 - 2 * self.eased)
        self.pose = self.start + s * (self.goal - self.start)
        eye, target, (lens, f_stop) = self.pose[:3], self.pose[3:6], self.pose[6:]
        # Placed on the prim: under Newton, `Camera.set_world_poses` moves a
        # Newton site the renderer never sees. USD's look-at is the view
        # matrix, world to camera, so the prim takes its inverse.
        view = Gf.Matrix4d().SetLookAt(Gf.Vec3d(*eye), Gf.Vec3d(*target), Gf.Vec3d(0, 0, 1))
        xform = UsdGeom.Xformable(self.prim)
        xform.ClearXformOpOrder()
        xform.AddTransformOp().Set(view.GetInverse())
        self.camera.GetFocalLengthAttr().Set(float(lens))
        self.camera.GetFStopAttr().Set(float(f_stop))
        self.camera.GetFocusDistanceAttr().Set(float(np.linalg.norm(target - eye)))


class Film:
    """Frames piped to ffmpeg as they come, and captions timed against
    them, written out as SubRip when the film is done."""

    def __init__(self, path: pathlib.Path):
        self.path = path
        self.frames = 0
        self.captions: list[tuple[int, int, str]] = []
        w, h = SIZE
        self.ffmpeg = subprocess.Popen(
            # Near-lossless and quick to write: `record.sh` encodes the video
            # to upload from this.
            [
                *f"ffmpeg -y -loglevel error -f rawvideo -pix_fmt rgb24 -s {w}x{h} -r {FPS}".split(),
                *"-i - -c:v libx264 -preset fast -crf 10 -pix_fmt yuv420p".split(),
                str(path),
            ],
            stdin=subprocess.PIPE,
        )

    def add(self, rgb: np.ndarray, caption: str | None) -> None:
        """Append a frame, under `caption`."""
        if caption and not (self.captions and self.captions[-1][2] == caption):
            self.captions.append((self.frames, self.frames, caption))
            print(f"[INFO]: caption {caption!r} at {self.frames / FPS:.1f} s")
        if caption:
            first, _, text = self.captions[-1]
            self.captions[-1] = (first, self.frames + 1, text)
        self.ffmpeg.stdin.write(rgb.tobytes())
        self.frames += 1

    def close(self) -> None:
        self.ffmpeg.stdin.close()
        self.ffmpeg.wait()

        def stamp(frame: int) -> str:
            s, ms = divmod(round(1000 * frame / FPS), 1000)
            return f"{s // 3600:02}:{s // 60 % 60:02}:{s % 60:02},{ms:03}"

        self.path.with_suffix(".srt").write_text(
            "".join(
                f"{i}\n{stamp(first)} --> {stamp(last)}\n{text}\n\n"
                for i, (first, last, text) in enumerate(self.captions, 1)
            )
        )


def main():
    parser = argparse.ArgumentParser(description="Records the pruning demo's video, headless.")
    parser.add_argument("out", type=pathlib.Path, help="the mp4 to write; captions go beside it")
    out = parser.parse_args().out.resolve()
    sim_cfg = demo.simulation_cfg("cuda:0")
    # Kit, headless, with nothing to look at the scene but the camera below:
    # its renderer needs Kit's, which a camera made after launch cannot ask for.
    launcher = {"headless": True, "enable_cameras": True, "require_kit": True}
    launcher |= {"visualizer": None, "visualizer_explicit": True}
    with launch_simulation(sim_cfg, launcher):
        # Imported once Kit is up: they bring `pxr`, and Kit's copy has to win.
        from isaaclab.sensors import Camera, CameraCfg
        from isaaclab_physx.renderers import IsaacRtxRendererCfg

        sim = sim_utils.SimulationContext(sim_cfg)
        machine = Bumblebee()
        robot = demo.design_scene(machine)
        camera = Camera(
            CameraCfg(
                prim_path=CAMERA,
                update_period=0.0,
                width=SIZE[0],
                height=SIZE[1],
                data_types=["rgb"],
                spawn=sim_utils.PinholeCameraCfg(clipping_range=(0.02, 200.0)),
                renderer_cfg=IsaacRtxRendererCfg(),
            )
        )
        driver, vines, to_row, shears = demo.start(sim, robot, machine, ROW)
        stage = sim_utils.get_current_stage()
        vine = vines[VINE]
        side = np.array([*to_row, 0.0])
        axes = np.column_stack([[*driver.forward, 0.0], side, [0.0, 0.0, 1.0]])

        # Each take by the ids of its cuts -- a `Cut` compares by value, so it
        # is no key itself -- and its cuts by the take.
        takes: dict[int, Take] = {}
        cuts_of: dict[Take, list[Cut]] = collections.defaultdict(list)

        def filmed(take: Take, cane: Cane) -> list[Cut]:
            """`take`'s cuts on `cane`."""
            cuts = []
            for kept in take.kept:
                below, above = cane.buds[kept - 1], cane.buds[kept]
                pose = aimed(below, above, shears, side)
                if pose is None:
                    raise RuntimeError(f"{cane.prim} runs along the approach at bud {kept}")
                cuts.append(Cut(cane, pose, below, above, np.array(take.push)))
                takes[id(cuts[-1])] = take
                cuts_of[take].append(cuts[-1])
            return cuts

        def planner(planted, mouth) -> list[Cut]:
            """The vine filmed's cuts, in `schedule`'s order; none on any other
            vine."""
            if planted is not vine:
                return []
            row = sorted(plan(vine, shears, side), key=lambda cut: cut.pose[:3, 3] @ axes[:, 0])
            cuts = []
            for planned, take in schedule(row, mouth, side):
                cuts += filmed(take, planned.cane) if take else [planned]
            return cuts

        def framing(shot: Shot, point: np.ndarray) -> np.ndarray:
            eye, target = point + axes @ shot.eye, point + axes @ shot.look
            return np.array([*eye, *target, shot.lens, shot.f_stop])

        rig, film = Rig(stage), Film(out)
        # The vine filmed and the next, for the robot to drive on to.
        stops = vines[VINE : VINE + 2]
        run = demo.run_simulator(
            sim, robot, machine, driver, stops, to_row, shears, planner=planner, render=False
        )
        # Frames due, counted a fraction a physics step: an eighth, at the
        # simulation's own speed.
        due, cut, done, entering = 0.0, None, None, False
        made: collections.Counter[Take | None] = collections.Counter()
        try:
            for step, pruning in enumerate(run):
                if step * SIM_DT < SETTLE:
                    continue
                working = pruning.cut if isinstance(pruning, Pruning) else None
                if working is not cut:
                    # A cut is over once the next one starts: checked then, as
                    # a cut lower down the cane would take its buds too.
                    if cut is not None:
                        cutting = not attached(cut.removed, shears, stage)
                        made[takes.get(id(cut))] += cutting
                        print(
                            f"[INFO]: {'made' if cutting else 'missed'} the cut on"
                            f" {cut.cane.prim} by {film.frames / FPS:.1f} s"
                        )
                    if working is None:
                        done = film.frames
                    entering = takes.get(id(working)) is not takes.get(id(cut))
                    cut = working
                take = takes.get(id(cut))
                doing = pruning.stage_name if isinstance(pruning, Pruning) else None
                swinging = doing in ("move", "tip", "home")
                due += (PACE.get(doing, 1.0) if take or swinging else 1.0) / 8
                if due < 1:
                    continue
                due -= 1
                if cut is None:
                    shot, point = CLOSING if done is not None else OPENING, vine.position
                elif take is None or (entering and pruning.stage_name == "move"):
                    # Swinging out to a take's cane, the arm can cross where
                    # the close-up's camera stands.
                    shot, point = REST, vine.position
                else:
                    middle = np.mean([each.pose[:3, 3] for each in cuts_of[take]], axis=0)
                    shot, point = take.shot, middle
                rig.aim(framing(shot, point))
                sim.render()
                camera.update(SIM_DT, force_recompute=True)
                rgb = camera.data.output["rgb"].torch[0, ..., :3].cpu().numpy()
                film.add(rgb, take.caption if take else shot.caption)
                if done is not None and film.frames >= done + ENDING * FPS:
                    break
        finally:
            film.close()
        print(f"[INFO]: {film.frames} frames, {film.frames / FPS:.1f} s, written to {out}")
        # The canes bend chaotically, so no two runs are quite alike: a blade
        # can catch a neighbouring cane, which its own take then finds cut
        # already. Such a run fails, and Kit exits with its status.
        short = [take.caption for take in TAKES if made[take] < len(take.kept)]
        if short:
            raise RuntimeError(f"not every cut of these takes was made: {short}")


if __name__ == "__main__":
    main()
