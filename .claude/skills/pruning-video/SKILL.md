---
name: pruning-video
description: Re-records the pruning demo's video in Isaac Sim with examples/pruning_demo/record.sh, and changes what it shows — the cases filmed, the camera, the pacing — by editing the storyboard at the top of examples/pruning_demo/video.py. Use after a change to the robot, the cutting, the planner or the dormant scene, or when asked to update, re-record or re-cut the pruning video. Not for the parameter editor's video, which is readme-video.
---

# Pruning video

The pruning demo's video is about a minute of the robot pruning one vine in
Isaac Sim: down the alley as it drives up, a first cane cut in a wider shot
as the arm swings out through the canes, then each case of the shear meeting
a cane, captioned — a planned cut, one cane cut three times, the cane pushed
back into the crotch of the blades, pushed aside by the fixed blade, pushed
aside by the moving blade — and the robot driving on. It is one command
away:

```bash
examples/pruning_demo/record.sh
```

It runs `video.py` headless in the demo's venv and writes
`target/video/pruning.mp4` and, beside it, `pruning-sheet.png`: the frames at
0, 2, 4… seconds, four to a row. A few minutes on an RTX GPU. It needs uv,
ffmpeg with libass, and the GPU; no display. A fresh venv asks once for the
Omniverse EULA, on the terminal.

Frames are sampled on the simulation's clock, so the video plays in simulated
time however slowly the frames render — slowed down while the arm swings and
closes in, a cane is pushed, the blade closes and the piece falls. The demo
hides the trellis wires, which have no collider yet.

## Re-record it

1. Run `examples/pruning_demo/record.sh`.
2. Check the log and the video against the list below. The log has a
   `caption '<text>' at T s` line as each caption comes up, `T` seconds into
   the video — the opening's, each take's as the arm swings out to its cane,
   the closing's — and a `made the cut on <cane> by T s` line, or `missed`,
   as each cut ends.
   `ffmpeg -ss T -i target/video/pruning.mp4 -frames:v 1 -update 1 frame.png`
   takes the frame at `T`. A video that fails one is not done: change the
   storyboard, never the video.
3. Hand the mp4 to a person to upload. GitHub plays only video it hosts, and
   only a signed-in browser can put it there, by dragging the file into an
   issue comment or the README's editor on github.com. The URL that gives
   goes on its own line under the title of `examples/pruning_demo/README.md`,
   in place of the old one if there is one.

`video.py` fails the run when a take's cut was not made, and `record.sh`
records again, five times at most. The canes bend chaotically, and the GPU
solvers are not bit-reproducible once the rendering load changes, so no two
storyboards' runs are quite alike: now and then a blade catches a
neighbouring cane, or a cane slips the mouth. About half the runs make every
cut. A run that fails five times is not bad luck: read its logs.

A video is right when:

- every cut is made; the run fails otherwise.
- in each close-up the shear is in the middle of the picture and sharp,
  the cane in its mouth while the blade closes, and nothing — the arm, a
  post — crosses the mouth. Sample a take every fifth of a second:
  `ffmpeg -ss T -t 5 -i target/video/pruning.mp4 -vf "fps=5,scale=384:-2,tile=5x5" -frames:v 1 -update 1 take.png`
- each cut piece is seen to fall.
- each caption sits under its own take and is legible at the README's
  width, about 830 px.
- the file is under 10 MB, the most GitHub takes on a free plan.
  `docs/video/encode.sh` spends 9 MB whatever the length, so a longer video
  gets fewer bits a second.

## Change what it shows

The storyboard is the constants at the top of
[video.py](../../../examples/pruning_demo/video.py); read their docstrings
first.

- **`TAKES`** are the cases, each on its own cane, filmed in their order but
  that the takes with a `push` come last. They go on every second cane along
  the cordon, after the canes between are cut: a push carries the open blades
  through a neighbouring cane still standing. `schedule` assigns the canes,
  and `test_video.py` checks it. The vine's canes no take has are cut first,
  filmed from `REST` — but for one left over among those every second, which
  comes last. A take has a caption, a `Shot`, a `push` and `kept`.
- **`push`** is `Cut.push`: how far the mouth moves on from the cut point
  before the shear closes, in the cut's own frame — x along the pivot, y
  across the mouth, z along the blades. +z drives the cane into the crotch,
  −y against the fixed blade, +y against the moving one. The mouth holds a
  cane up to 2.4 cm off its centre; at full opening the moving blade's tip is
  level with that centre, 5 cm out from the pivot, so a push toward it needs
  some +z as well, or the cane slips off the tip.
- **`kept`** is the buds the cane keeps after each of the take's cuts, made
  in that order: top down, so each cut has wood left below it.
- **A `Shot`** is where the camera stands and looks, relative to the point it
  frames — the middle of a take's cut points, or the foot of the vine for
  `OPENING`, `REST` and `CLOSING` — in meters along the alley the way the
  robot drives, across it toward the row, and up; then a focal length, an
  f-number, and a caption for when no take's is up. Depth of field is strong
  for the number: f/48 already softens a close-up's background, and 0 turns
  it off. A take of several cuts wants a shot far enough back to hold all of
  them, like `ACROSS`.
- **The camera** eases from one shot to the next over `EASE` seconds, and
  cuts instead when the move is longer than `CUT`, so it never flies through
  the robot or the row. Each swing out to a take's cane is filmed from
  `REST`, as the arm can cross where a close-up's camera stands.
- **`PACE`** slows down the stages of a take's cut by `Pruning.stage_name`,
  and the swing out of every cut; the frame rate stays `FPS`. `ENDING` is
  how long the video runs on after the vine is pruned.
- **`ROW`, `VINE`** pick the vine: away from the headland, so there is
  vineyard behind every shot.

**Keep the camera clear.** The close-ups stand on the robot's side of the
row, a little above the mouth and behind it, on the side the moving blade
opens to. The arm comes in from the other side, so the forearm stays out of
the picture whichever way the solver bends the wrist. A camera on the far
side of the row has the robot's chassis behind the cane: good contrast for a
wide shot, a busy background for a close-up.

**Finding a shot** is quicker from one paused moment than from a recording:
in a scratch copy of `video.py`'s `main`, stop iterating `run_simulator` at
the moment wanted — the first tick of a take's `close` stage, say — then for
each candidate `Shot` call `rig.aim`, `sim.render()` and
`camera.update(SIM_DT, force_recompute=True)`, and save
`camera.data.output["rgb"]`. The physics stands still between renders, so
the candidates show the same instant. Run one Kit at a time: two on the GPU
at once change each other's physics, and one has hung on its first frame.
