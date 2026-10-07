"""Driving the robot and working the shear from the keyboard.

The left hand has the machine and the right hand the shear: `W`/`S` drive
the base forward and back and `A`/`D` turn it; the arrows jog the shear's
mouth along the robot and across it, `Page Up`/`Page Down` up and down, and
`Enter` closes the shear on whatever is in the mouth, cutting the first cane
the blade closes on, then opens it again; the robot holds still while it
shuts.

The mouth is a gantry head, not a wrist: it stays squared up to the row --
blades pointing out over the rail, pivot upright, so an upright cane lies
across it -- and its position is held in the robot's own frame, so it rides
along when the base drives. A jog the arm cannot reach is refused, which
keeps the target where the arm is.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import numpy as np

from bumblebee import ARM_HOME, Bumblebee
from driver import CRUISE_SPEED, MAX_YAW_RATE, Driver
from kinematics import frames, solve
from pruner import JOINT_STEP, Stroke, reached, squared

if TYPE_CHECKING:
    from vinerylab.isaaclab import Shears

READY = (0.0, 1.0, -1.6, 2.4, -0.8, 1.0, 1.6)
"""The slide and arm positions the arm reaches out to at the start: turned
to the row with the elbow folded, the mouth squared up out over the rail at
the height of a spur cut, a stand-off short of the row. A posture rather
than a point: solved for a point this close to the rail, the arm folds back
along it with the slide at its end stop, and the swing out from `ARM_HOME`
to that sweeps the hand through the row. From `ARM_HOME` to this one, every
link stays over the deck."""

JOG_STEP = 0.005
"""How far the mouth moves per control tick a jog key is held, in meters: a
quarter of a metre a second at the controller's rate."""

UP, OUT = np.array([0.0, 0.0, 1.0]), np.array([0.0, 1.0, 0.0])
"""The mouth's one orientation, in the base frame: the pivot along `UP`,
the blades along `OUT`, which is toward the row."""

CUT = "ENTER"
"""The key that closes the shear."""

KEYS = (
    ("W / S", "drive forward / back"),
    ("A / D", "turn left / right"),
    ("Up / Down", "jog the mouth forward / back"),
    ("Left / Right", "jog it toward the row / away"),
    ("Page Up / Down", "jog it up / down"),
    ("Enter", "cut"),
)
"""The keys and what they do, as the viewport lists them."""


class Teleop:
    """The keyboard's hold on the robot, driven a control tick at a time.

    `held` is the set of keys down, by carb's names for them; `listen` keeps
    it from Kit's keyboards. `control` takes what `Pruning.control` takes and
    returns what it returns, so the simulation loop runs either.
    """

    done = False
    """Never: teleoperation ends with the run."""

    def __init__(self, machine: Bumblebee, driver: Driver, shears: Shears):
        self.machine, self.driver = machine, driver
        self.held: set[str] = set()
        self.stroke = Stroke(machine, shears)
        self.closing = False
        # From the folded arm out to the ready posture, on a planned move,
        # with the mouth squared up exactly where that posture puts it.
        self.target = np.array([0.0, *ARM_HOME])
        self.mouth = (frames(machine.chain, READY)[-1] @ machine.tool)[:3, 3]
        self.goal, _, _ = solve(
            machine.chain, machine.tool, squared(self.mouth, UP, OUT), READY, iterations=50
        )

    def listen(self) -> None:
        """Follow the keys of every keyboard Kit has, from now on: the
        window's, and any logical one a script feeds."""
        import carb.input

        def on_input(event) -> bool:
            if event.deviceType == carb.input.DeviceType.KEYBOARD:
                key = event.event
                if key.type == carb.input.KeyboardEventType.KEY_PRESS:
                    self.held.add(key.input.name)
                elif key.type == carb.input.KeyboardEventType.KEY_RELEASE:
                    self.held.discard(key.input.name)
            return True

        self._subscription = carb.input.acquire_input_interface().subscribe_to_input_events(
            on_input
        )

    def control(self, q: np.ndarray, hand: np.ndarray, shear: float) -> tuple[np.ndarray, float]:
        """Advance one tick: drive the base, jog the mouth, work the shear.
        `hand` is the pose of the body the shear is on in world coordinates
        and `shear` the blade's angle. Returns the slide and arm positions to
        hold and the blade angle to hold."""
        # The stroke is made with the hand still: while the blade shuts, the
        # base and the mouth hold and the keys wait.
        held = set() if self.closing else self.held
        self.driver.drive(
            CRUISE_SPEED * (("W" in held) - ("S" in held)),
            MAX_YAW_RATE * (("A" in held) - ("D" in held)),
        )
        jog = np.array(
            [
                ("UP" in held) - ("DOWN" in held),
                ("LEFT" in held) - ("RIGHT" in held),
                ("PAGE_UP" in held) - ("PAGE_DOWN" in held),
            ],
            dtype=float,
        )
        if jog.any():
            # Solved from the last goal, not from where the arm is, so a jog
            # never flips the arm into another posture.
            pose = squared(self.mouth + JOG_STEP * jog / np.linalg.norm(jog), UP, OUT)
            result = solve(self.machine.chain, self.machine.tool, pose, self.goal, iterations=50)
            if reached(result):
                self.mouth, self.goal = pose[:3, 3], result[0]
        self.target = self.target + np.clip(self.goal - self.target, -JOINT_STEP, JOINT_STEP)
        if self.stroke.tick(hand, shear, self.closing):
            if self.closing:
                print(f"[INFO]: shear closed, {'a cane' if self.stroke.cut else 'nothing'} cut")
            # Shut, it opens; open, it shuts again while the key is down.
            self.closing = not self.closing and CUT in held
        return self.target, self.stroke.jaw


def show_keys() -> None:
    """List `KEYS` in the bottom right corner of Kit's viewport, clear of the
    world axes Kit draws in the left one."""
    import omni.ui as ui
    from omni.kit.viewport.utility import get_active_viewport_window

    with get_active_viewport_window().get_frame("teleop_keys"), ui.VStack():
        ui.Spacer()
        with ui.HStack(height=0):
            ui.Spacer()
            with ui.ZStack(width=0, height=0):
                ui.Rectangle(style={"background_color": 0x99000000, "border_radius": 4})
                with ui.HStack(width=0, height=0, spacing=16, style={"margin": 6}):
                    for column in zip(*KEYS, strict=True):
                        ui.Label("\n".join(column))
