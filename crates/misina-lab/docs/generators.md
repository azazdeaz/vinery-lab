# Generators

A **generator** is the one type the whole framework is keyed on. It names the
params aggregate, the scene and the Python package, and adds the element
plugins. Everything else, the panel, the headless build, the snippet, the
generated Python and docs, is a function of that type.

```rust
pub struct Boxes;

impl Generator for Boxes {
    type Params = BoxesParams;
    const NAME: &'static str = "Boxes";
    const PACKAGE: &'static str = "boxlab";
    fn plugin(app: &mut App) { /* the elements' plugins */ }
}
```

| Item | What it decides |
| --- | --- |
| `Params` | the aggregate of every element's params, declared with `generator!` |
| `NAME` | the root prim of the scene, and the stem of the Python names: `BoxesParams`, `BoxesCfg`, `BOXES_CFG`, `boxes_cfg.py` |
| `PACKAGE` | the Python package that re-exports the `_core` extension module and holds `{PACKAGE}.isaaclab`; also the prefix of the environment variables, `BOXLAB_PERF` |
| `plugin` | adds the element plugins; their build systems go in the generator's own stage sets, chained inside `Build` |

## The params aggregate

Each element's params struct is a **fragment**, stored in the world as its
own resource so that change detection is per element: moving one slider
re-runs that element's layer and nothing above it. The **aggregate** is the
whole set as one plain value, for everything that has to hold one: a Python
call, a headless build, the panel's staged copy.

`generator!` declares the aggregate from its field list, once:

```rust
misina_lab::generator! {
    /// Every knob of the box row.
    pub struct BoxesParams as PyBoxesParams("BoxesParams") for Boxes {
        pub scene: SceneParams,
        pub terrain: misina_lab::terrain::TerrainParams as core,
        #[reflect(@Label("The boxes"))]
        pub boxes: BoxParams,
    }
}
```

It emits the struct; its `Params` implementation, `apply` (split into the
resources, leaving an unchanged fragment alone) and `read` (the inverse); and
under the caller's `python` feature the Python side below. A `@Label` on a
field titles its panel section. A fragment this crate declares,
`TerrainParams`, is marked `as core`: it carries its own Python constructor,
and PyO3 accepts a `#[pymethods]` block only in the crate that declared the
type.

A generator's `SceneParams` is its own. The framework has no opinion on what
is scene-wide beyond the seed every element draws from
([randomness.md](randomness.md)).

## Stage sets

Everything that authors the scene runs in `PreUpdate`, inside the `Build`
set. A generator chains its own sets inside it, one per layer, in build
order: the terrain first, then whatever places things on it, then what hangs
off those. The chain is what makes the pipeline a pipeline; each set has
finished spawning before the next one queries ([elements.md](elements.md)).
The panel commits an edit before `Build`, and the perf marks bracket it.

## What the framework builds from it

| Surface | Entry point |
| --- | --- |
| the interactive viewer, with the panel, the footer and the save key | `viewer::app::<G>()` |
| one headless build cycle, and the scene document | `generate::scene::<G>(&params)` |
| the parameter panel | `ui::plugin::<G>` |
| the **Copy Isaac Lab cfg** snippet | `snippet::cfg::<G>(&params)` |
| the generated stub, cfg classes, re-exports and parameters page | `codegen::targets::<G>(docs)`, and `codegen::stale` for the test |
| a built scene to assert against in a test | `testing::grown::<G>(params)` |

The generator's `main` is `viewer::app::<G>()` plus whatever viewer-only
plugins it wants, run. Its `lib.rs` holds the `Generator` impl and, under its
`python` feature, the `#[pymodule] fn _core` that calls the `module` function
the macro wrote.

## The Python module

Under the `python` feature the macro also emits the extension module's
contents. The aggregate becomes a `#[pyclass]` under the quoted name, holding
one `Py<T>` per fragment. `Py<T>` matters: a plain field would make the
getter clone, so `params.boxes.count = 5` would mutate a temporary while the
scene kept the old value. For the same reason fragments are
`skip_from_py_object`; they are live shared objects, and extracting one by
value would hand back a copy.

Every fragment gets a keyword-only constructor that sets fields by name
through reflection, so the Rust side has no signature to keep in step with
the struct; the typed signature tooling sees is the generated `_core.pyi`. A
`@Choices` field is validated where it enters from Python, with a
`ValueError` for a name off the list.

The aggregate has two entry points. `write_usd(path)` runs one headless
build, serializes the document and hands it to `misina_lab.usd.build_usd`,
which is why `usd-core` is the Python package's dependency and not the
crate's. `generate_scene_json()` returns the document instead, for a caller
that wants the bytes. Both release the GIL for the Rust work: the params are
copied out first, so nothing inside touches Python objects, and other threads
in a host such as Isaac Sim keep running.

`module(m)` registers every class and `__version__`, and sets each class's
`__module__` to the extension module's name, which PyO3 would otherwise leave
as `builtins`. The Isaac Lab spawner reads it to find the extension the
params came from ([isaac-lab.md](isaac-lab.md)).

## What a generator's tests run

- `testing::check_params::<P>()` panics on the first field that does not
  declare what the panel, the snippet and the docs need: a doc comment, a
  slider or a choice list, a default in range.
- `testing::nudged::<P>()` is a set with every field moved off its default,
  for the round trip through `apply` and `read`.
- A test over `codegen::stale::<G>(root, docs)` fails while a generated file
  is behind the structs; its ignored twin, `regen_params`, rewrites them
  ([editing-parameters.md](editing-parameters.md)).

## Where it lives

- [`src/lib.rs`](../src/lib.rs): `Generator`, `Params`, `Build`
- [`src/params.rs`](../src/params.rs): `generator!`, `fragment_python!`, and the walk over a params struct
- [`src/python.rs`](../src/python.rs): the keyword constructor, the choice check, the two entry points
- [`src/generate.rs`](../src/generate.rs): the headless app
- [`src/testing.rs`](../src/testing.rs): what a generator's tests get, and the `Boxes` fixture this crate tests itself with
