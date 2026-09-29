"""What a generated scene's Python side is built from.

A generator such as `vinerylab` compiles its Rust core into a `_core`
extension module whose params classes write a scene as a JSON document. This
package is the half that turns the document into something a simulator loads:
`misina_lab.usd` authors it as a USD stage, and `misina_lab.isaaclab` spawns
it in Isaac Lab, generating and caching the file on first use.

Neither subpackage is imported from here: `usd` needs `usd-core`, `isaaclab`
needs Isaac Lab, and a generator's plain `import` must keep working with
neither.
"""
