## Isaac Lab revision

`isaaclab` comes from a pinned Git commit rather than a release wheel — the
releases lag the features this demo uses. `tools/wheel_builder` in that repo is
a PEP 517 backend that builds the same aggregate package from any revision, so
the dependency stays a plain `isaaclab[isaacsim,all]` and needs no checkout.

To move to a newer revision:

1. Set `rev` in `[tool.uv.sources]` to the commit hash. A branch name also
   works, but then any re-lock silently picks up a new Isaac Lab.
2. Copy `tools/wheel_builder/uv-overrides.txt` from that revision into
   `override-dependencies`. These are Isaac Lab's own resolver pins, which the
   wheel it builds does not carry; without them the `isaacsim` extra pulls the
   stack back to versions Isaac Lab needs newer.
3. Re-lock and sync:

       uv lock --upgrade-package isaaclab
       uv sync

Extras can also move between revisions.
