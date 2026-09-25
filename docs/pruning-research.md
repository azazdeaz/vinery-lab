# Cutting shoots: prior art

What robots and machines that cut vines, branches and fruit stems do, and how
simulators have modelled a cut. It is the background to
[`vinerylab.isaaclab.cutting`](../python/vinerylab/isaaclab/cutting.py) and the
straddler demo's `--trim` hedger, and a list of what could be built on them.

## Where this leaves LeVinery

| Topic | In the literature | Here |
| --- | --- | --- |
| Where to cut | Picked from the plant's structure: "between the Nth and N+1th bud", "above the second node", a node graph plus an offset | `Shears.cut(body, at)`: a rod segment and a fraction along it |
| Cutting a flexible stem | Not simulated. A cut is a pose check, or breaks a joint placed in advance | Any point on a rod, while the simulation runs |
| Non-selective trimming | Hedgers cut the canopy's sides to a plane at 3–6 km/h | `Shears.cut_through`, driven by the straddler's bars at about 4 km/h |
| What falls | Collected or left on the ground | Laid on the ground one segment at a time (`Shears.settle`) |

## Green canopy: hedgers and trimmers

This is the machine the `--trim` demo copies. It fits the scene's flexible
shoots better than a dormant-pruning robot does, since those are the green
shoots of a summer canopy.

- **Trimming ("cimatura")** is done in June–July and cuts shoots 30–40 cm
  beyond the last containment wire. Sickle bars, a double alternating blade,
  run at 3–4 km/h; rotary knives run at 5–6 km/h ([Rinieri][rinieri]).
- **Hedger bars** come 0.9–1.8 m long. Some systems run up to 15 km/h, and an
  over-the-row frame with three bars holds them square to the row on a slope
  with a gravity pivot ([Whitco][whitco]).
- **Shoot thinning** is also mechanised, but blind. Non-selective thinners
  vary from 10% to 85% in how many shoots they remove, because they cannot
  find the cordon under the leaves. Hand thinning costs about $650/ha against
  about $25/ha by machine, which is the case for a vision-guided thinner
  ([Washington State Wine Commission][wawine]).

A hedger cuts the whole canopy face. LeVinery's trellised shoots are static
meshes, so the demo's bars sit just outside them and cut only the flexible
strays; cutting the held canopy, or topping it with a horizontal bar, needs
those shoots built as rods too.

## Dormant pruning robots

These choose each cut from a 3D model of the vine and make it with an arm.

- **Botterill et al. (2017)** built the first full cane-pruning system: a
  shrouded platform straddling the row, trinocular stereo building a 3D vine
  model as it drives, and a UR5 sweeping a 6 mm router bit (100 W, 24 krpm)
  through each cane along an RRT-planned path with 5 cm of margin either
  side. It averaged 8.4 cuts and about 2 minutes per vine. The router
  sometimes failed to part a cane, and the authors name shears as future
  work ([JFR][botterill]).
- **Bumblebee (CMU/Cornell, 2021)** detects buds, orders them along each cane
  over a graph of the vine, and cuts between the Nth and N+1th bud (four kept
  per cane in the proof of concept). Each target is a 6D pose taken from the
  cane's local direction. The arm plans to a stand-off 15 cm out, then closes
  in on a straight line square to the cane. Other canes are not obstacles; only
  the cordon, trunk and trellis are. A bypass shear needs about 320 N to cut
  an 8 mm cane, roughly linear in diameter. It scored 87% pruning accuracy at
  213 s per vine ([arXiv 2112.00291][bumblebee]).
- **Felicetti et al. (2025)** go the other way: a tractor-mounted rotary head
  on a four-bar linkage, placed by LiDAR and synchronised to the travel speed
  by GPS, with one operator ([JFR][felicetti]).
- **The Barracuda** shears have recesses in their teeth that a trellis wire
  slips into while the cane is cut: 94.5% effective with 2.6% wire damage
  ([Waikato][barracuda]).

How the cut point is described:

- **Cut types.** Fernandes et al. sort cuts into clean, base-bud, spur and
  replacement cuts. A cut point is a 3D point between two reference nodes
  plus an angle ([arXiv 2109.07247][fernandes]).
- **A graph of the vine.** ViNet estimates the vine as a graph of nodes and
  branches, which an expert system turns into pruning advice for a person or
  a robot. The 3D2Cut dataset behind it has over 1500 annotated images
  ([CEA 2023][vinet], [dataset][3d2cut]).
- **A skeleton with no cut points.** A skeleton of segments and radii can be
  built from a point cloud and still leave choosing the cut as open work
  ([arXiv 2307.11706][skeleton]).
- **Across the field**, a 2025 review finds pruning success of 66–97% on
  grapevine and lower on cherry and apple. It names occlusion, lab-only
  manipulation results and varying cane toughness as the open problems
  ([arXiv 2505.07318][review]).

Measured forces on dormant cane: a flat knife takes 234.5 N and 1.78 J, a
serrated one 303.8 N and 2.14 J, both falling as the cutting angle rises
towards 40° ([EJOSAT][knife]).

## Orchard pruning

- **A two-phase controller.** You et al. prune UFO cherry trees with electric
  bypass shears rated to 3.2 cm. A vision controller trained with PPO in
  PyBullet drives the cutter in at 3 cm/s until the wrist feels more than
  1.5 N, then an admittance controller seats the branch in the jaws. Cutting
  succeeded 58% of the time, at 35 s per cut ([arXiv 2206.07201][you-robot],
  [arXiv 2109.13162][you-hybrid]).

## Fruit stems

The tomato case reuses the cut and changes where it is made.

- **SWEEPER** (sweet pepper) clamps the stem and cuts it with a vibrating
  knife on the same downward stroke ([JFR 2020][sweeper]).
- **Harvey** (sweet pepper) falls back, when it cannot see the peduncle, on
  cutting 50 mm above the top of the fruit. That alone gave 58% attach-and-cut
  success ([arXiv 1709.10275][harvey]).
- **AHPPEBot** (tomato) labels seven keypoints on a peduncle and argues for
  cutting near the junction with the stem, not at its bend: 86.67%
  harvesting success at 32.5 s each ([arXiv 2405.06959][ahppebot]).
- **Forces.** The thickest tomato peduncles need at most about 60 N to cut,
  an order of magnitude below a woody cane, and a pedicel's end near the stem
  takes about 85% more shear force than its end near the fruit
  ([Agronomy 2024][pedicel]). Strawberry peduncles are sized the same way
  ([arXiv 2207.12552][strawberry]).

## How simulators model a cut

- **As a pose check.** The Oregon State PyBullet pruning environments grow
  procedural trees (a modified L-Py) and count a cut as done when the cutter
  is within 5 cm of the target and its jaws within 30°. No geometry is
  severed. The RL policy reached about 30% in simulation and in the field,
  against about 60% for a motion-planning oracle
  ([arXiv 2507.23015][jain]). A later version bends the branches as
  cantilevers and checks the cutter to within 7 cm and two 30° angles
  ([arXiv 2609.24906][visuomotor]).
- **As a joint placed in advance.** "Find the Fruit" trains in Isaac Sim with
  fragile joints between plant parts, which break under force and are
  penalised when they do, and transfers zero-shot to real plants
  ([arXiv 2505.16547][findfruit]). MuJoCo has no cut either; turning a joint
  off once its force passes a threshold is the usual workaround.
- **As a crack through a mesh.** DiSECt simulates knives through fruit and
  vegetables with FEM: springs across the future cut weaken with the knife's
  stress until they vanish, so nothing is re-meshed, and the whole thing is
  differentiable ([arXiv 2105.12244][disect]).
- **Rods without cuts.** Cosserat-rod simulators coupled to Isaac Sim model
  cables more finely than a capsule chain, but none of them cut
  ([DeformX, arXiv 2606.22116][deformx]).

`Shears` goes past each of these. It severs a stem at any point along it, not
at a joint placed in advance, and it does so on a solver already stepping the
rest of the scene.

## What this suggests next

- **Spur and cane pruning rules.** Walk a rod from the cordon and cut at a
  given arc length or segment count, the way the dormant-pruning robots count
  buds. `Shears.cut(body, at)` already takes that position.
- **A force gate.** Refuse a cut the tool cannot make, from the stem's
  diameter and a per-crop force: about 320 N at 8 mm for dormant cane, at
  most about 60 N for a tomato peduncle.
- **Approach, then cut.** For an arm-mounted cutter, fire the cut only once
  the jaws are within a pose tolerance of the target (5–7 cm and 30° above),
  or once contact force passes a threshold, rather than on reaching a point.
- **Separate metrics.** Score "chose the right cut" apart from "made the
  cut". A real robot's accuracy folds the two together; a simulation has the
  ground truth to split them.
- **Topping and full-face hedging.** Build the trellised shoots as rods, or
  keep a pool of spare bodies to hand them to, so a bar can cut the held
  canopy too.
- **Tomato harvesting.** Build a truss as a short branching rod with the
  fruit on it, and cut near the stem junction.

## Sources

[rinieri]: https://www.rinieri.com/eng/blog/41/when-to-perform-mechanical-ciming-in-the-vineyard/
[whitco]: https://www.whitcovinquip.com.au/hedging-systems/
[wawine]: https://www.washingtonwine.org/research/precise-mechanical-solution-for-vineyard-shoot-thinning/
[botterill]: https://doi.org/10.1002/rob.21680
[bumblebee]: https://arxiv.org/abs/2112.00291
[felicetti]: https://onlinelibrary.wiley.com/doi/10.1002/rob.22453
[barracuda]: https://researchcommons.waikato.ac.nz/entities/publication/e4d7e06c-e0c5-448b-8a28-4f78cdbf3010
[fernandes]: https://arxiv.org/abs/2109.07247
[vinet]: https://doi.org/10.1016/j.compag.2023.107736
[3d2cut]: https://www.idiap.ch/en/scientific-research/data/3d2cut
[skeleton]: https://arxiv.org/abs/2307.11706
[review]: https://arxiv.org/abs/2505.07318
[knife]: https://dergipark.org.tr/en/pub/ejosat/article/532914
[you-robot]: https://arxiv.org/abs/2206.07201
[you-hybrid]: https://arxiv.org/abs/2109.13162
[sweeper]: https://onlinelibrary.wiley.com/doi/full/10.1002/rob.21937
[harvey]: https://arxiv.org/abs/1709.10275
[ahppebot]: https://arxiv.org/abs/2405.06959
[pedicel]: https://doi.org/10.3390/agronomy14102274
[strawberry]: https://arxiv.org/abs/2207.12552
[jain]: https://arxiv.org/abs/2507.23015
[visuomotor]: https://arxiv.org/abs/2609.24906
[findfruit]: https://arxiv.org/abs/2505.16547
[disect]: https://arxiv.org/abs/2105.12244
[deformx]: https://arxiv.org/abs/2606.22116

- Rinieri, *When to perform mechanical trimming in the vineyard* — [rinieri.com][rinieri]
- Whitco Vineyard Equipment, *Hedging systems* — [whitcovinquip.com.au][whitco]
- Washington State Wine Commission, *Precise mechanical solution for vineyard shoot thinning* — [washingtonwine.org][wawine]
- Botterill et al., *A robot system for pruning grape vines*, J. Field Robotics 34(6), 2017 — [doi][botterill]
- Silwal et al., *Bumblebee: a path towards fully autonomous robotic vine pruning*, 2021 — [arXiv][bumblebee]
- Felicetti et al., *Tractor mounted grapevine pruning system*, J. Field Robotics 42, 2025 — [doi][felicetti]
- Williams et al., *The Barracuda: a novel cane pruning technology for avoiding wires*, University of Waikato, 2024 — [link][barracuda]
- Fernandes et al., *Towards precise pruning points detection using semantic-instance-aware plant models*, 2021 — [arXiv][fernandes]
- Gentilhomme et al., *Towards smart pruning: ViNet*, Computers and Electronics in Agriculture 207, 2023 — [doi][vinet]; 3D2Cut dataset — [idiap.ch][3d2cut]
- Schneider et al., *3D skeletonization of complex grapevines for robotic pruning*, 2023 — [arXiv][skeleton]
- *Autonomous robotic pruning in orchards and vineyards: a review*, 2025 — [arXiv][review]
- *Effect of various knife type, cutting angle and speed on cutting force and energy of grape cane*, EJOSAT — [dergipark][knife]
- You et al., *An autonomous robot for pruning modern, planar fruit trees*, 2022 — [arXiv][you-robot]
- You et al., *Precision fruit tree pruning using a learned hybrid vision/interaction controller*, ICRA 2022 — [arXiv][you-hybrid]
- Arad et al., *Development of a sweet pepper harvesting robot*, J. Field Robotics, 2020 — [doi][sweeper]
- Lehnert et al., *In-field peduncle detection of sweet peppers for robotic harvesting*, 2017 — [arXiv][harvey]
- Ma et al., *AHPPEBot: autonomous robot for tomato harvesting based on phenotyping and pose estimation*, 2024 — [arXiv][ahppebot]
- *Tomato pedicel physical characterization for fruit-pedicel separation tomato harvesting robot*, Agronomy 14(10), 2024 — [doi][pedicel]
- *Peduncle gripping and cutting force for strawberry harvesting robotic end-effector design*, 2022 — [arXiv][strawberry]
- Jain et al., *Learning to prune branches in modern tree-fruit orchards*, 2025 — [arXiv][jain]
- *Visuomotor robotic pruning in planar orchards using hybrid reinforcement learning*, 2026 — [arXiv][visuomotor]
- Subedi et al., *Find the fruit: zero-shot sim2real RL for occlusion-aware plant manipulation*, 2025 — [arXiv][findfruit]
- Heiden et al., *DiSECt: a differentiable simulation engine for autonomous robotic cutting*, 2021 — [arXiv][disect]
- *DeformX: a versatile co-simulation framework for deformable linear objects*, 2026 — [arXiv][deformx]
