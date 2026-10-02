# Quantization

Every organ of a layer carries a **config**, and each distinct config would
need a mesh of its own. **Quantization** reduces the population to a budget
of representatives, builds one mesh per representative, and has every organ
draw the mesh of the one it is closest to. A field of ten thousand leaves
comes out of it as a few dozen meshes and ten thousand references.

```rust
pub trait Metric<T> { fn distance(&self, a: &T, b: &T) -> f32; }

pub fn farthest_first<T: Clone, M: Metric<T>>(
    items: &[T], k: usize, max_radius: f32, metric: &M,
) -> Codebook<T>;
```

`quantize.rs` is the whole of it and knows nothing about Bevy or botany.
Gonzalez's farthest-first traversal is a 2-approximation for metric
**k-center**: it minimizes the *worst* distance from an organ to its
representative rather than a sum, which makes it density-blind, which is
what lets a rare variant survive. One diseased leaf among ten thousand
healthy ones still earns a mesh, provided the metric puts it far away.

## Writing a metric

- **Configs are flat, and the metric decides what matters.** No
  shape/instance split and no weight structs: one `LeafConfig` with all its
  fields, and a `LeafMetric` whose `distance` picks fields and hard-codes
  weights. The weights convert each field into roughly how far apart it
  *looks*.
- **A field the builder ignores must not reach the metric.** A field that is
  pure placement, the angle a leaf hangs at, is read by no part of the mesh,
  so two configs differing only in it share a mesh and still hang at their
  own angle. Get this wrong and the budget is spent telling apart things
  that build identically.
- **Categorical fields get a step.** A young plant and a mature one are
  different plants, not different sizes, so the metric puts them further
  apart than any two of one kind can be. A leaf's outline is the same: five
  drawings are five shapes, not five nearby numbers, so any budget of five
  or more keeps all five. And two configs that build no mesh at all should
  be *zero* apart, so the budget is not spent on things that cost nothing.

## The budget

`params.variations` is a **budget**, not a count: how many representatives
the clustering may keep. Raise it to spend memory on variety, lower it to
trade variety for memory. The covering radius on the `Codebook` is the
diagnostic: how far the worst-served organ is from the mesh it drew.

A budget can only buy what the population contains. Configs that are all
identical collapse to one representative however high the budget, so each
layer needs at least one real per-instance axis: a girth drawn per plant, a
length and a node spacing drawn per stem. They go in the *config* rather
than into a placement scale. A scale would be free but invisible to the
clustering, so the budget would land on one arbitrary size instead of
covering the range that occurs. Two independent axes beat one; a budget
spent covering a line buys much less than one covering a plane.

## Determinism

The same params must give the same scene on every machine and every run; a
downstream cache is keyed on it. `farthest_first` starts from index 0 and
breaks ties toward the lowest index, so the codebook is a function of the
input slice alone. The slice's order is the caller's to fix: collect with
`scene::Order` and sort before calling ([elements.md](elements.md#rules)).

## Where it lives

- [`src/quantize.rs`](../src/quantize.rs): `Metric`, `Codebook`, `farthest_first`
