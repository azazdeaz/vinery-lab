"""The generated vineyard as a Newton scene, without Isaac Lab.

The counterpart of `examples/isaaclab_demo/main.py`: generate the vineyard USD
with `vinerylab`, then load it -- here into a `newton.ModelBuilder` rather than
onto a Kit stage. Nothing needs Isaac Lab or Isaac Sim.

    uv run main.py
    uv run main.py --viewer null --num-frames 1

Nothing moves yet. There is no robot and no cable in this scene, so no solver
is constructed either; this is the environment the rest of the directory builds
on. `usd_cable_test.py` is the same scene with a dozen offshoots simulated as
Newton VBD cables.

The vineyard brings its own colliders -- the ground as an exact triangle mesh,
a capsule per post and trunk -- so nothing here adds a ground plane.
"""

from __future__ import annotations

import hashlib
import pathlib

from pxr import Usd

import newton
import newton.examples
import vinerylab

CACHE = pathlib.Path(__file__).parent
"""Where generated stages land. They are keyed on the parameters that made
them, so several can coexist and editing `PARAMS` never reuses a stale one."""

PARAMS = vinerylab.VineyardParams(
    terrain=vinerylab.TerrainParams(width=40.0, height=30.0, max_elevation=1.5),
    parcel=vinerylab.ParcelParams(orientation=-14.0, row_spacing=2.0),
)
"""A parcel sized for a demo. Import time is dominated by the canopy, which
Newton flattens to one shape per leaf: this parcel is ~20k shapes and about 15
seconds, while the 82 m square `isaaclab_demo` runs is ~186k and 2.5 minutes.
Drop `VISUALS` to load the colliders alone, which is under a second at any
size."""

VISUALS = True
"""Whether to load the canopy. Off, Newton sees the ground mesh and the post
and trunk capsules -- everything that can be collided with, and nothing else."""


def vineyard_usd(params: vinerylab.VineyardParams) -> pathlib.Path:
    """The USD for `params`, generating it on a miss.

    The file name carries a digest of the parameters, so a run with edited
    parameters generates rather than silently reusing the previous stage.
    """
    digest = hashlib.sha256(repr(params).encode()).hexdigest()[:16]
    path = CACHE / f"vineyard_{digest}.usd"
    if not path.exists():
        params.write_usd(str(path))
    return path


class Example:
    def __init__(self, viewer, args):
        self.viewer = viewer
        self.fps = 60
        self.frame_dt = 1.0 / self.fps
        self.sim_time = 0.0

        builder = newton.ModelBuilder()
        builder.add_usd(
            Usd.Stage.Open(str(vineyard_usd(PARAMS))),
            load_visual_shapes=VISUALS,
            load_static_visual_shapes=VISUALS,
        )
        self.model = builder.finalize()
        self.state = self.model.state()

        self.viewer.set_model(self.model)

    def step(self):
        # No dynamics in this scene; the clock is all there is to advance.
        self.sim_time += self.frame_dt

    def render(self):
        self.viewer.begin_frame(self.sim_time)
        self.viewer.log_state(self.state)
        self.viewer.end_frame()


if __name__ == "__main__":
    viewer, args = newton.examples.init()
    newton.examples.run(Example(viewer, args), args)
