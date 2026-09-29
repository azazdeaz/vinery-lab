"""Every relative link in the repo's Markdown resolves to a file that exists.

The doc pages navigate by file link, and a link that rots does so silently
on GitHub and in an editor alike. Only relative links are checked; a URL is
someone else's to keep alive. `*.local.md` files are private notes and
skipped.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"\]\(([^)\s#]+)(?:#[^)]*)?\)")
"""The target of an inline link or image, without its anchor."""
SKIPPED_DIRS = {".claude", ".git", ".venv", "node_modules", "site", "target"}


def pages() -> list[Path]:
    return sorted(
        page
        for page in ROOT.rglob("*.md")
        if not SKIPPED_DIRS & set(page.relative_to(ROOT).parts)
        and not page.name.endswith(".local.md")
    )


@pytest.mark.parametrize("page", pages(), ids=lambda page: str(page.relative_to(ROOT)))
def test_relative_links_resolve(page: Path) -> None:
    targets = LINK.findall(page.read_text(encoding="utf-8"))
    broken = [
        target
        for target in targets
        if not re.match(r"[a-z][a-z0-9+.-]*:", target) and not (page.parent / target).exists()
    ]
    assert not broken, f"{page.relative_to(ROOT)}: {broken}"
