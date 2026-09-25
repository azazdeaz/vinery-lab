"""Cutting a rod anywhere along it while the simulation runs.

A flexible shoot is a rod: a chain of capsule bodies, one per segment, joined
by rod joints (see `physics`). A cut at a joint is one switch -- turn the joint
off -- and everything past it falls away as a chain of its own. A cut anywhere
else moves the joint to the cut first: the capsule the cut lands on is
shortened to end there, the next one down the chain is stretched back to start
there, the mass moves with the length, and then the joint between them goes.
The two bodies either side of the cut are two the model already has.

That is the constraint everything here works around. Newton sizes every array
-- bodies, shapes, joints, which shapes may collide -- when the model is
finalized, and the CUDA graph a step replays holds pointers into them. A value
written into one of those arrays in place is seen by the next replay; a new
body, or anything that reallocates, is not. So a cut only writes values.

Under the coupled solver each entry steps its own copy of the model's arrays.
The body and joint copies are refreshed from the model by a model-change
notification. The shape copies are written directly: the shape refresh clones
the contact-pair list anew, and the graph would go on reading the old one.

The rendered shoot follows. The tube drawn along a segment is rescaled to its
capsule's new length, and whatever hangs off the shortened segment past the
cut -- a leaf -- is hidden, since a prim cannot move to the piece that carries
on without it.

A piece that falls has nothing to land on, because the rods do not collide with
the static scene (see `physics.SHOOT_GROUP`). `settle` stops each of its bodies
where it reaches the ground.
"""

from __future__ import annotations

from collections.abc import Callable

import numpy as np

SPLINTER = 0.01
"""The shortest piece, in meters, a cut leaves either side of it.

A cut closer than this to a joint is made at the joint instead. A body
shortened to nothing has no mass left, and the solver divides by it.
"""

STEM = "Stem"
"""The prim each rod segment's tube is drawn as, below the segment's own.

The generator picks it -- `STEM` in `src/elements/shoot.rs`."""


class Shears:
    """Cuts the rods of the running simulation.

    Build one once the model exists -- after the first
    `SimulationContext.reset()` -- and keep it: it tracks which rods are cut
    and which pieces are still falling. `stage` is where the rods are drawn,
    the running one by default.

    A rod's bodies are numbered from the wood out, and every position along
    one here is measured the same way: 0 at the end nearer the wood.
    """

    def __init__(self, stage=None):
        # Imported here and not at module scope: `newton` brings `pxr` with it,
        # and Kit's own `pxr` wins the import only if nothing loaded the pip one
        # first. See `physics.tune_shoots`.
        import isaaclab.sim as sim_utils
        from isaaclab_newton.physics import NewtonManager
        from newton import JointType

        self._manager = NewtonManager
        self._stage = stage if stage is not None else sim_utils.get_current_stage()
        model = self._model = NewtonManager._model
        if model is None:
            raise RuntimeError(
                "no Newton model to cut: build Shears after the first reset, under Newton"
            )
        self._parent = model.joint_parent.numpy()
        self._child = model.joint_child.numpy()
        rods = np.flatnonzero(model.joint_type.numpy() == int(JointType.ROD))
        # The joint at each end of a body, while it is still on: the one it
        # hangs from and the one the rest of its rod hangs from.
        self._root_joint = {int(self._child[joint]): int(joint) for joint in rods}
        self._tip_joint = {int(self._parent[joint]): int(joint) for joint in rods}

        self.bodies = np.array(sorted(self._root_joint.keys() | self._tip_joint.keys()), dtype=int)
        """Every rod body, by model index. The arrays below are in this order."""
        self._index = {int(body): i for i, body in enumerate(self.bodies)}
        shape_body = model.shape_body.numpy()
        shapes = np.flatnonzero(np.isin(shape_body, self.bodies))
        # One capsule per body, which the rod importer builds on the body's own
        # z axis. Its span along that axis is what a cut moves.
        self._shape = shapes[np.argsort(shape_body[shapes])]
        placed = model.shape_transform.numpy()[self._shape]
        if not np.allclose(placed[:, [0, 1, 3, 4, 5]], 0.0):
            raise ValueError("a rod capsule is off its body's z axis; a cut would move it wrong")
        scale = model.shape_scale.numpy()[self._shape]
        self._radius = scale[:, 0].copy()
        self._half = scale[:, 1].copy()
        self._center = placed[:, 2].copy()
        # The tube each body is drawn with was built at this length.
        self._drawn = self._half.copy()
        self._labels = list(model.body_label)

        # Every copy of the shape arrays a solver steps from: the model's, and
        # the coupled solver's entries' own. See the module docstring.
        solver = NewtonManager._solver
        entries = solver.entry_names() if hasattr(solver, "entry_names") else ()
        self._shape_copies = [model] + [
            view
            for view in (solver.view(name) for name in entries)
            if view.shape_scale.ptr != model.shape_scale.ptr
        ]
        if any(len(copy.shape_scale) != len(model.shape_scale) for copy in self._shape_copies):
            raise ValueError(
                "a solver entry renumbers the shapes; its copy cannot be written by index"
            )

        self.loose = np.zeros(len(self.bodies), dtype=bool)
        """Which bodies are cut free of their rod, in `bodies` order."""
        self._falling: set[int] = set()

    def cut(self, body: int, at: float) -> bool:
        """Cut rod body `body` at `at`, a fraction of its capsule's length, and
        let everything past the cut fall. Returns whether anything fell.

        A cut within `SPLINTER` of a joint is made at that joint. A body at the
        end of a rod -- its last segment, or the stub an earlier cut left --
        has no body past it to carry the offcut: a cut on it takes the whole
        body, and one within `SPLINTER` of its free end takes nothing.
        """
        i = self._index[body]
        tip, root = self._tip_joint.get(body), self._root_joint.get(body)
        length = 2 * self._half[i]
        if tip is None:
            if (1 - at) * length < SPLINTER or root is None:
                return False
            joint = root
        elif (1 - at) * length < SPLINTER:
            joint = tip
        elif at * length < SPLINTER:
            # A rod's first body hangs from no joint: cut past it instead.
            joint = root if root is not None else tip
        else:
            self._split(i, self._index[int(self._child[tip])], at)
            joint = tip
        self._release(joint)
        return True

    def cut_through(self, origin, u, v) -> int:
        """Cut every rod still whole where it passes through the rectangle
        `origin + a u + b v`, `a` and `b` in [0, 1] -- a knife's reach, in world
        coordinates. Returns how many cuts were made.

        A rod through it twice is cut at the crossing nearer the wood; the
        other is on the piece that falls.
        """
        origin, u, v = (np.asarray(each, dtype=float) for each in (origin, u, v))
        start, end = self._capsules()
        normal = np.cross(u, v)
        before, after = (start - origin) @ normal, (end - origin) @ normal
        crossing = (before * after < 0) & ~self.loose
        at = np.where(crossing, before / np.where(crossing, before - after, 1.0), 0.0)
        point = start + at[:, None] * (end - start) - origin
        a, b = point @ u / (u @ u), point @ v / (v @ v)
        hits = np.flatnonzero(crossing & (0 <= a) & (a <= 1) & (0 <= b) & (b <= 1))
        cuts = 0
        # In body order, which is root first along each rod.
        for i in hits:
            if not self.loose[i]:
                cuts += self.cut(int(self.bodies[i]), float(at[i]))
        return cuts

    def settle(self, height: Callable[[float, float], float]) -> None:
        """Lay every falling body that has reached the ground down on it.

        `height(x, y)` is the ground under a point, e.g.
        `vinerylab.usd.Ground.height`. A landed body is cut from the rest of
        its piece, tipped level about the horizontal axis across it, so its
        leaves keep their places along it, and made kinematic with no velocity,
        which Newton holds still whatever pushes on it.

        One body at a time, because a piece keeps the stiffness of the rod it
        was cut from: held by its first landed body, the rest of it would stand
        up off the ground for good. Cut loose, the rest falls on and lands a
        body at a time, along the line the piece fell on.
        """
        if not self._falling:
            return
        from newton import BodyFlags, ModelFlags

        start, end = self._capsules()
        landed = {
            i
            for i in self._falling
            if min(start[i, 2], end[i, 2]) - self._radius[i]
            <= height(*(start[i, :2] + end[i, :2]) / 2)
        }
        if not landed:
            return
        self._falling -= landed
        state = self._manager._state_0
        for i in landed:
            body = int(self.bodies[i])
            for joint in (self._tip_joint.get(body), self._root_joint.get(body)):
                if joint is not None:
                    self._disable(joint)
            middle = (start[i] + end[i]) / 2
            middle[2] = height(*middle[:2]) + self._radius[i]
            pose = _lying(state.body_q.numpy()[body], end[i] - start[i], middle, self._center[i])
            _write([state.body_q], body, lambda _: pose)
            _write([state.body_qd], body, np.zeros_like)
            _write([self._model.body_flags], body, lambda _: int(BodyFlags.KINEMATIC))
        self._manager.add_model_change(ModelFlags.BODY_PROPERTIES)

    def _capsules(self) -> tuple[np.ndarray, np.ndarray]:
        """Each rod body's capsule axis in world coordinates, as its end nearer
        the wood and its far end. Shape is (N, 3) each."""
        pose = self._manager._state_0.body_q.numpy()[self.bodies]
        axis = _rotate_z(pose[:, 3:])
        return (
            pose[:, :3] + axis * (self._center - self._half)[:, None],
            pose[:, :3] + axis * (self._center + self._half)[:, None],
        )

    def _split(self, i: int, next_: int, at: float) -> None:
        """Hand the part of body `i` past `at` to `next_`, the body after it."""
        # ponytail: the piece goes onto the next body along that body's own
        # axis, which a bent rod points off `i`'s: by 10 degrees at the joint, a
        # full-length piece lands about a centimeter out of line. Tilt the
        # capsule to follow `i` if a cut ever shows it.
        piece = 2 * self._half[i] * (1 - at)
        self._half[i] *= at
        self._center[i] -= piece / 2
        self._half[next_] += piece / 2
        self._center[next_] -= piece / 2

        mass = self._model.body_mass.numpy()[self.bodies[[i, next_]]]
        # A rod's first body is kinematic, and stays massless however long.
        self._reshape(i, mass[0] * at)
        self._reshape(next_, mass[1] + mass[0] * (1 - at))
        # Past the cut, on the shortened body: nothing there to hang from.
        end = self._center[i] + self._half[i]
        from pxr import UsdGeom

        body = self._stage.GetPrimAtPath(self._labels[self.bodies[i]])
        for child in body.GetChildren() if body.IsValid() else ():
            if child.GetName() != STEM:
                origin = UsdGeom.Xformable(child).GetLocalTransformation().ExtractTranslation()
                if origin[2] > end:
                    UsdGeom.Imageable(child).MakeInvisible()

        from newton import ModelFlags

        self._manager.add_model_change(ModelFlags.BODY_INERTIAL_PROPERTIES)

    def _reshape(self, i: int, mass: float) -> None:
        """Write body `i`'s capsule span and `mass` into the model, and redraw
        its tube to match."""
        half, center, radius = self._half[i], self._center[i], self._radius[i]
        body, shape = int(self.bodies[i]), int(self._shape[i])

        def moved(transform):
            transform[2] = center
            return transform

        def resized(scale):
            scale[1] = half
            return scale

        copies = self._shape_copies
        _write([copy.shape_transform for copy in copies], shape, moved)
        _write([copy.shape_scale for copy in copies], shape, resized)
        _write([copy.shape_collision_radius for copy in copies], shape, lambda _: half + radius)
        if mass > 0:
            # A solid cylinder about its middle: a capsule this slender is one.
            across = mass * (3 * radius**2 + (2 * half) ** 2) / 12
            inertia = np.diag([across, across, mass * radius**2 / 2])
            model = self._model
            _write([model.body_mass], body, lambda _: mass)
            _write([model.body_inv_mass], body, lambda _: 1 / mass)
            _write([model.body_com], body, lambda _: [0.0, 0.0, center])
            _write([model.body_inertia], body, lambda _: inertia)
            _write([model.body_inv_inertia], body, lambda _: np.linalg.inv(inertia))

        from pxr import Gf, UsdGeom

        stem = self._stage.GetPrimAtPath(f"{self._labels[body]}/{STEM}")
        if stem.IsValid():
            # The tube was built centered on the body at the capsule's first
            # length, so a scale along z and a shift redraws it at any span.
            xform = UsdGeom.XformCommonAPI(stem)
            xform.SetTranslate(Gf.Vec3d(0.0, 0.0, float(center)))
            xform.SetScale(Gf.Vec3f(1.0, 1.0, float(half / self._drawn[i])))

    def _release(self, joint: int) -> None:
        """Turn `joint` off, and mark everything past it as falling."""
        body = int(self._child[joint])
        self._disable(joint)
        while True:
            i = self._index[body]
            self.loose[i] = True
            self._falling.add(i)
            if (joint := self._tip_joint.get(body)) is None:
                return
            body = int(self._child[joint])

    def _disable(self, joint: int) -> None:
        from newton import ModelFlags

        _write([self._model.joint_enabled], joint, lambda _: False)
        self._manager.add_model_change(ModelFlags.JOINT_PROPERTIES)
        del self._tip_joint[int(self._parent[joint])]
        del self._root_joint[int(self._child[joint])]


def _write(arrays, index: int, change: Callable) -> None:
    """Replace element `index` of each of `arrays` by `change` of its value,
    in place: a copy the graph reads keeps its address."""
    for array in arrays:
        element = array[index : index + 1]
        value = element.numpy()
        value[0] = change(value[0])
        element.assign(value)


def _lying(pose: np.ndarray, axis: np.ndarray, middle: np.ndarray, offset: float) -> np.ndarray:
    """A body `pose` (position, then an (x, y, z, w) rotation) tipped level
    about the horizontal axis across its capsule `axis`, and moved so the
    capsule's middle -- `offset` up the body's z axis -- is at `middle`."""
    axis = axis / np.linalg.norm(axis)
    level = np.array([axis[0], axis[1], 0.0])
    # One standing on end has no way it leans; lay it along x.
    level = level / np.linalg.norm(level) if np.linalg.norm(level) > 1e-6 else np.array([1.0, 0, 0])
    # The shortest rotation from `axis` to `level`, then the pose's own.
    turn = np.append(np.cross(axis, level), 1 + axis @ level)
    turn /= np.linalg.norm(turn)
    (x1, y1, z1, w1), (x2, y2, z2, w2) = turn, pose[3:]
    rotation = [
        w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
        w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
        w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
        w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
    ]
    return np.concatenate([middle - level * offset, rotation])


def _rotate_z(quat: np.ndarray) -> np.ndarray:
    """The z axis of each (x, y, z, w) rotation in `quat`, shape (N, 4) -> (N, 3)."""
    x, y, z, w = quat.T
    return np.column_stack([2 * (x * z + w * y), 2 * (y * z - w * x), 1 - 2 * (x * x + y * y)])
