"""The repo's Markdown, held to the rules a regex can check.

Every relative link resolves, down to the heading it names, and no link
points at a line number: lines move with every edit and nothing notices.
Each docs folder's pages are all listed in its index, and nothing in the
framework, docs or code, names a generator or speaks its vocabulary, since it
leaves the repo with the crate. What a regex cannot read is prose, which
`crates/misina-lab/docs/AGENTS.md` covers.
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent

FENCE = re.compile(r"^ *```.*?^ *```", re.DOTALL | re.MULTILINE)
SPAN = re.compile(r"`[^`\n]*`")
LINK = re.compile(r"\]\(([^)\s#]*)(#[^)\s]*)?\)")
"""An inline link or image: its target, empty for one into its own page, and
its anchor."""
HEADING = re.compile(r"^#{1,6} +(.+?) *$", re.MULTILINE)
URL = re.compile(r"[a-z][a-z0-9+.-]*:")
LINE = re.compile(r"#L\d+|:\d+$")
"""A line anchor, `#L42`, or a line suffix, `lib.rs:42`."""
GENERATOR = re.compile(r"\b(vine\w*|grape\w*|canes?|cordons?|spurs?|swards?)\b", re.IGNORECASE)
"""What the generators in this repo go by: the vineyard's names, and the parts
of a vine it is built from. A generator that joins the repo adds its own."""
INDEXES = {"docs": "AGENTS.md", "crates/misina-lab/docs": "crates/misina-lab/README.md"}
"""Each docs folder, and the file that lists every page in it."""
GUIDES = {"AGENTS.md", "CLAUDE.md"}
"""What a docs folder holds for whoever edits it, rather than as a page."""


def listed(*pathspecs: str) -> list[Path]:
    """Every file git tracks or would track under `pathspecs`. What it
    ignores, the build trees, the caches and `*.local.md` notes, is not
    checked."""
    paths = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", *pathspecs],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.split("\0")
    return sorted(path for path in map(ROOT.joinpath, filter(None, paths)) if path.exists())


MARKDOWN = listed("*.md")
FRAMEWORK = listed("crates/misina-lab", "python/misina-lab")
"""Everything that leaves the repo with the framework: its crate and its
Python package."""


def name(page: Path) -> str:
    return str(page.relative_to(ROOT))


def links(page: Path) -> list[tuple[str, str]]:
    """Every link outside code, where a bracket is not Markdown."""
    return LINK.findall(SPAN.sub("", FENCE.sub("", page.read_text(encoding="utf-8"))))


def anchors(page: Path) -> set[str]:
    """The anchors GitHub gives a page's headings: lowercased, punctuation
    dropped, each space a hyphen."""
    headings = HEADING.findall(FENCE.sub("", page.read_text(encoding="utf-8")))
    return {re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-") for heading in headings}


@pytest.mark.parametrize("page", MARKDOWN, ids=name)
def test_relative_links_resolve(page: Path) -> None:
    broken = []
    for target, anchor in links(page):
        if URL.match(target):
            continue
        path = page.parent / target if target else page
        if not path.exists() or (
            anchor and path.suffix == ".md" and anchor[1:] not in anchors(path)
        ):
            broken.append(target + anchor)
    assert not broken, f"{name(page)}: {broken}"


@pytest.mark.parametrize("page", MARKDOWN, ids=name)
def test_no_link_points_at_a_line(page: Path) -> None:
    """A line number moves with every edit above it; link the file and name
    the symbol instead."""
    lines = [
        target + anchor
        for target, anchor in links(page)
        if LINE.search(anchor) or (not URL.match(target) and LINE.search(target))
    ]
    assert not lines, f"{name(page)}: {lines}"


@pytest.mark.parametrize(("folder", "index"), INDEXES.items())
def test_every_page_is_indexed(folder: str, index: str) -> None:
    home = (ROOT / index).parent
    linked = {(home / target).resolve() for target, _ in links(ROOT / index)}
    missing = [
        name(page)
        for page in MARKDOWN
        if page.parent == ROOT / folder and page.name not in GUIDES and page.resolve() not in linked
    ]
    assert not missing, f"{index} does not link {missing}"


@pytest.mark.parametrize("path", FRAMEWORK, ids=name)
def test_the_framework_names_no_generator(path: Path) -> None:
    """Its examples are the row of boxes and a generic plant."""
    lines = path.read_text(encoding="utf-8").splitlines()
    hits = [f"{n}: {line.strip()}" for n, line in enumerate(lines, 1) if GENERATOR.search(line)]
    assert not hits, f"{name(path)}: {hits}"
