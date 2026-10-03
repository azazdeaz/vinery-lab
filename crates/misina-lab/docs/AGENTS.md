# Writing the docs

How a page in this folder is written, and how it stays true as the code
changes under it. It is for anyone editing the docs, person or agent; the
`CLAUDE.md` beside it loads it for Claude Code whenever a page here is
opened. `tests/test_docs.py`, at the repository root, checks what a regex
can, and this guide covers the rest.

## Who reads a page

Someone who knows Rust, Bevy and the basics of USD, but not this code. A
person reads a page from the top and stops once they have what they came
for; an agent greps for a name and reads the paragraph it lands in. Both are
served by the same order: the definition first, then one example, then the
detail, with the source one link away.

## What goes where

| Place | What it holds |
| --- | --- |
| a module doc or a docstring | that module's contract: what it is, what it promises, the rules for using it |
| a page in this folder | how modules fit together, and why they are built that way |
| [`../README.md`](../README.md) | the front door: what the crate is, the smallest generator, and the table of pages |
| [`../AGENTS.md`](../AGENTS.md) | the map: where things are, and the rules that bite |

Each fact is written once and linked from everywhere else. A sentence that
would fit both a page and a module doc goes in the module doc, where it is
read beside the code it describes and changes with it.

## Writing a page

1. Title it with a noun naming the idea, `# Quantization`. A how-to is
   titled with what it does, `# Editing parameters`.
2. Define the idea in the first paragraph, in words that stand on their own.
3. Show one example. Code is the row of boxes from the README, or a generic
   plant, leaf or stem, trimmed with `/* ... */` to what the point needs.
4. Give each further section a heading that names what it explains, so a
   link can land on it.
5. End with `## Where it lives`: the files, each with the symbols a reader
   would open it for. A how-to has numbered steps in place of the sections,
   and links each file where a step uses it.
6. Add a row to the README's table. The test fails while a page is missing
   from it.

One idea per page. Split a page when it takes on a second idea or passes
about 200 lines.

## Prose

- Write for someone who has only the code in front of them. State a fact
  about the code or a rule to follow, not how it was found, and not the
  answer to a question only ever asked in a chat. If a detail doesn't change
  what the reader does next, cut it.
- Give the reason for a rule that isn't obvious. A rule without one is
  broken by the first reader who thinks they know better.
- Say what a measured number was measured on.
- Name no generator. The framework's examples are its own, and the test
  fails on a generator's name anywhere in this crate's Markdown.

## Links

- Link the file and name the symbol: [`src/rng.rs`](../src/rng.rs), `salt`.
  Never link a line number. Lines move with every edit and nothing notices,
  while a symbol survives and can be grepped. The test fails on a line link.
- Link a heading, `elements.md#rules`, to land a reader on one section. The
  test checks that the heading exists.
- Keep links relative and inside the repository. The test fails on one that
  resolves to nothing.

## Markdown that renders everywhere

A page is read on GitHub, in an editor and, later, on a rendered site; the
README is read through rustdoc as well. Write what all of them agree on: one
H1, fenced code with a language tag, tables, Mermaid for diagrams. No raw
HTML, and no renderer-specific callouts such as `!!! note` or `> [!NOTE]`.

## Keeping a page current

A page goes stale when the code changes under it, and no test reads prose.
So the change that renames a public item, or changes what one does, also
fixes the pages that name it:

1. Grep this folder and the README for the old name.
2. Fix what each hit says, not only the name.
3. Delete a page whose idea is gone, with its row in the README's table.

A renamed heading breaks the links into it, and the test names them. The
README's Rust block is a doctest, so `cargo test` catches it going stale.
Nothing else here is compiled: a code block on a page is a sketch, read
against the code it sketches.
