"""Turn a generated scene document into a USD stage.

    python -m vinerylab.usd scene.json scene.usd

The viewer's save key writes `scene.json`; this is the other half. The normal
path from Python does not go through here -- `vinerylab.isaaclab` calls the
generator and the builder in one process -- but having a command means a
document can be inspected, edited and rebuilt without running Rust at all.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

from .build import build_usd


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m vinerylab.usd",
        description="Build a USD stage from a vinerylab scene document.",
    )
    parser.add_argument(
        "document", type=pathlib.Path, help="scene document, as written by the generator"
    )
    parser.add_argument(
        "output",
        type=pathlib.Path,
        help="where to write the stage; the extension picks the format "
        "(.usd/.usdc for the binary crate form, .usda for text)",
    )
    parser.add_argument(
        "-f", "--force", action="store_true", help="overwrite an existing output file"
    )
    args = parser.parse_args(argv)

    if args.output.exists():
        if not args.force:
            parser.error(f"{args.output} already exists; pass --force to overwrite it")
        # `Usd.Stage.CreateNew` refuses an existing path rather than truncating.
        args.output.unlink()

    build_usd(json.loads(args.document.read_text()), str(args.output))
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
