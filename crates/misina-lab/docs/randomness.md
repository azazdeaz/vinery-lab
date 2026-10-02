# Randomness

One seed for the whole scene, held on the generator's scene-wide params.
Every layer salts it with a constant of its own before drawing, so nudging
one layer never re-rolls another: tuning how many stems a plant carries must
not reshape the trunk under them.

```text
a representative's mesh   seed ^ LAYER_STREAM ^ salt(representative index)
an instance's placement   seed ^ CHILD_STREAM ^ salt(its Order)
```

`Rng` is an inlined SplitMix64 rather than a crate's default generator,
because the requirement is that the same seed gives the same scene on every
machine and every version, which a fixed algorithm guarantees and a crate
does not promise. `salt` spreads a counter across the whole 64-bit range, so
neighbouring indices seed unrelated streams instead of the same one shifted
by a step.

## Rules

**Each layer keeps its own stream constants**, one arbitrary `*_STREAM: u64`
per kind of draw it makes. The palette's per-mesh jitter has its own,
`COLOR_STREAM`.

**The draw order of a stream is part of an element's output.** Inserting a
draw in the middle re-rolls everything downstream of it, so elements
document their order where a reader would otherwise be tempted to reorder
it.

**Where a slot can go empty, the draws happen anyway.** A bud that pushed no
stem still consumes its draws, so that a lighter prune drops one stem
instead of reshuffling every one after it.

**What must vary per instance is drawn per instance, into the config.** The
quantizer only sees what is in the config
([quantization.md](quantization.md#the-budget)).

## Where it lives

- [`src/rng.rs`](../src/rng.rs): `Rng`, `salt`
- [`src/palette.rs`](../src/palette.rs): `shade`, the per-mesh jitter, and `COLOR_STREAM`
