---
name: readme-video
description: Re-records the README's demo video of the viewer with docs/video/record.sh, and changes what it shows — the camera moves, the panel edits, their timing — by editing the storyboard docs/video/tour.json. Use after a change to how the viewer, the panel or the scene looks, or when asked to update, re-record or re-cut the README video. Not for still screenshots.
---

# README video

The README's **Parameter editor app** section shows a 40-second video of the
viewer: the parcel from above while the terrain and the row layout are edited,
a descent into an alley, the vines trained lower, a glimpse of the wireframe,
stray shoots, a cover crop drying to straw, the vines going dormant, and a
pull back over the winter vineyard. It is one command away:

```bash
docs/video/record.sh
```

It builds the viewer, plays the storyboard `docs/video/tour.json` in it one
video frame per rendered frame, and writes `target/video/vinerylab.mp4` and,
beside it, `vinerylab-sheet.png`: the frames at 0, 2, 4… seconds, four to a
row. About a minute on a GPU. It needs cargo, ffmpeg and a display; the window
opens on the desktop at the video's size, ignores the pointer, and closes
itself when the video is done.

## Re-record it

1. Run `docs/video/record.sh`.
2. Check the sheet, and the frames around every camera move, against the list
   below. The viewer's log, which the script prints, has a
   `tour: step N at T s` line for each step: `N` counts the entries of `steps`
   from 1, and `T` is seconds into the video.
   `ffmpeg -ss T -i target/video/vinerylab.mp4 -frames:v 1 -update 1 frame.png`
   takes the frame at `T`. A video that fails one is not done: change the
   storyboard, never the video.
3. Hand the mp4 to a person to upload. GitHub plays only video it hosts, and
   only a signed-in browser can put it there, by dragging the file into an
   issue comment or the README's editor on github.com. The URL that gives
   replaces the `https://github.com/user-attachments/assets/…` line under
   **Parameter editor app** in README.md.

A video is right when:

- its first frame shows the whole scene, shaded. If shapes pop in, raise the
  storyboard's `warmup`, the wall-clock seconds the opening shot gets before
  recording starts (3 by default).
- no camera move passes through a vine, a post or the cover. Sample each move
  every half second:
  `ffmpeg -i target/video/vinerylab.mp4 -vf "select='between(t,12,16)*not(mod(n,15))',scale=480:-2,tile=4x2" -frames:v 1 -update 1 move.png`
- the panel shows each control, tooltip up, before it moves, and the scene has
  visibly answered an edit before the next one starts
- the panel's text is legible at the README's width, about 830 px
- the file is under 10 MB, the most GitHub takes on a free plan.
  `docs/video/encode.sh`, which the pruning demo's video goes through too,
  spends 9 MB whatever the length, so a longer video gets fewer bits a
  second: if the foliage turns to mush, cut shots rather than raise it.

## Change what it shows

The storyboard is the window (`size`, and `scale`, which enlarges the panel),
`warmup`, the opening shot (`start`, played before recording starts), and
`steps`. Its format is documented on `Tour`, `Step` and `Pose` in
[crates/misina-lab/src/tour.rs](../../../crates/misina-lab/src/tour.rs); read
those first. In short: everything in a step starts together and takes `secs`,
and the next step starts `wait` later, `secs` if left out — so `"wait": 0` runs
two steps at once, which is how the camera keeps moving under panel edits. The
last step's `wait` is the closing hold: the video ends when it runs out.
`note` is a comment.

- **Parameters** are named `fragment.field`, as Python names them:
  `params.shoot.stray` is `"shoot.stray"`.
  [docs/parameters.md](../../../docs/parameters.md) lists every one with its
  slider's range. The storyboard is checked against the params when it loads,
  and by `cargo test` (`the_readme_tour_is_valid`).
- **An edit** is two steps, one right after the other: `show` the field — its
  section opens, the others fold, the panel scrolls to it and its tooltip comes
  up — then `set` it. Any step between them that neither shows nor sets the
  field, a camera move included, takes the tooltip down. A number slides there
  over `secs` the way a drag would, and the scene answers 150 ms after it
  stops, as it does to a person; a flag or a name switches at once. Edits
  persist: every later shot sees them.
- **View options** outside the params, such as the footer's Wireframe, are
  `toggle`d by their caption.
- **The camera** orbits a focus. `focus` is x and y in meters on the parcel,
  which is centred on 0, then a height above the ground there — read when the
  step starts, so a camera step that starts before a terrain edit has settled
  aims at the old ground. `yaw` is 0 looking from −y and 90 from +x; `pitch`
  is above the horizon; `radius` is the distance. Parts left out stay as they
  are.
- **Alleys.** With the rows at orientation θ and spacing s, a row runs through
  the origin and the others are s apart, so the alley next to it is centred on
  (−sin θ, cos θ) · s/2 — `[-0.38, 0.82]` for the 25° and 1.8 m the storyboard
  sets. The camera looks down it from yaw θ + 90 or θ − 90.
- **Keep the camera out of the plants.** Beside a row it has to be above the
  canopy, about 2 m. To get down into an alley, line up over it first, then
  change only `pitch` and `radius`, which keeps the camera in the alley's
  vertical plane. Once down, a `yaw` change moves the camera sideways by about
  `radius × sin(Δyaw)`, which has to stay well inside half the alley, 0.9 m
  in the storyboard. Stray shoots and dormant canes grow out into the alley
  and past the top wire: be up over the canopy before they appear. On steep
  hills the ground behind a low camera can rise above it.
- **Finding a shot**: write a scratch storyboard of cuts — steps with
  `"secs": 0` and `"wait": 2` after a first step of `"wait": 1`, so each of
  the sheet's frames falls a second into a cut, after its edits have built —
  and run
  `docs/video/record.sh scratch.json /tmp/scratch.mp4`.

`VINERYLAB_TOUR=docs/video/tour.json cargo run --release` plays the storyboard
on the wall clock, without recording: a preview, rebuild stalls included.

## Something no step can do

Add it to the tour as a field of `Step`, phrased as what a person would change —
the panel, the camera — rather than as something drawn over the video. The tour
is in the framework and knows nothing of vines; anything vineyard-specific
belongs in the storyboard.
