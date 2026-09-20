# Editing parameters

A parameter is declared once, on a field of a `*Params` struct in Rust.
Everything else reads that declaration:

| Surface | How it gets there |
|---|---|
| Viewer panel: caption, slider range, tooltip | `src/ui.rs` walks the structs through Bevy reflection |
| **Copy Isaac Lab cfg** snippet | `src/snippet.rs`, the same walk |
| Python constructor `PoleParams(radius=0.05)` | `src/python.rs`, keyword arguments set fields by name |
| Type stub `python/vinerylab/_core.pyi` | generated |
| Isaac Lab `PoleCfg` in `python/vinerylab/isaaclab/vineyard_cfg.py` | generated |
| Re-exports in both `__init__.py` files | generated |
| Reference page [`docs/parameters.md`](parameters.md) | generated |

`src/params.rs` is the walk; `src/codegen.rs` is the generator.

## Change or add a parameter

1. Edit the field on its struct, in the element's file under `src/elements/`.
   A field needs a doc comment, a slider or a choice list, and a default:

   ```rust
   /// Post radius, in meters. The default is the 8 cm round softwood post
   /// that is the commonest thing in a European vineyard.
   ///
   /// Anything after the first paragraph is for a reader of the Rust.
   #[reflect(@Slider { min: 0.01, max: 0.1, step: 0.005 })]
   pub radius: f32,
   ```

   and its value in the struct's `impl Default`.

2. Regenerate the Python and the docs:

   ```bash
   cargo test regen_params -- --ignored
   ```

3. Run `cargo test`. It fails while a generated file is stale, and the
   `params` test says what a field is missing.

That is the whole change. Do not edit the marked regions in the Python files
by hand; the next regeneration overwrites them.

## What a field declares

**The first paragraph of the doc comment is user-facing.** It is the tooltip,
the Python docstring and the row in `docs/parameters.md`, so write it to stand
on its own, with no rustdoc link syntax (`[`like this`]`). Backticks are fine;
the tooltip drops them. Later paragraphs are for the Rust reader and may say
what they like. A struct's doc comment is user-facing in full: it becomes the
class docstring, so it should say what the element is and how its fields
relate.

**A numeric field carries a `@Slider`** with the range the viewer offers and
the step it moves in. The displayed precision follows from the step. The
range is the slider's, not a limit: Python takes any value the generator can
build, so a builder clamps what it must (see `PoleConfig::new`).

**A string field carries a `@Choices`** pointing at the element's fixed list of
names, `@Choices(&Kind::NAMES)`. The viewer offers them as a dropdown and
Python rejects any other name with a `ValueError`.

**A `bool` carries nothing** and gets a checkbox.

**`@Label("...")` overrides the caption**, which is otherwise the field name
with underscores as spaces. Use it for units, `@Label("Feature size (m)")`,
or where the name is not the wording wanted on screen, `@Label("Cordons per
vine")` on `arms`. On a fragment field of `VineyardParams` it titles the
panel section.

Field types are `f32`, `u32`, `u64`, `bool` or `String`. Declaration order is
the order in the panel, the stub and the docs, so keep the shape knobs first
and the budget knobs, `variations` and `detail`, last.

## Add a fragment

A new element's params struct is a fragment. Beyond the element file itself:

1. Derive `Reflect` on the struct and declare its fields as above.
2. Add it to `VineyardParams` in `src/elements/mod.rs`, and to `apply` and
   `from_world` below it. A test moves every field through both and fails on
   a missed line.
3. Add the type to the `py_params!` list in `src/python.rs` and a `Py<T>`
   field to `PyVineyardParams` beside it.
4. Regenerate, as above. The stub, the cfg class, the `FRAGMENTS` table and
   every re-export come out of the walk.

## Where the text goes

Tooltips and the generated docstrings come from the struct at run time, through
Bevy's `reflect_documentation` feature, so a doc comment is never copied
anywhere. If a description reads wrong in the viewer or in an IDE hover, fix
it on the field.
