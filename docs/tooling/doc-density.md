# Doc legibility-density checker

`scripts/ci/check-doc-density.py` keeps the committed documentation read-optimized
by flagging the two patterns that make long-lived docs hard to skim:

- **Over-long lines** - any line wider than `--max-line` characters (default
  `800`). Usually a run-on sentence or an over-stuffed table row.
- **Over-budget table cells** - any single markdown table cell holding more than
  `--max-cell-words` words (default `150`). The fix is to move the cell body into
  a dedicated section (same page) or a sub-page and leave a one-line summary +
  link in the cell.

It is the durable guard behind the one-fact-per-cell / no-wall-of-prose
convention: same information, more navigable structure.

## Scope

- Every `docs/**/*.md`.
- Every top-level `crates/<name>/README.md`.
- Every top-level `*.md` - `CLAUDE.md`, `README.md`, `CONTRIBUTING.md`.
- The generated `crates/web-viewer/pkg/README.md` is skipped.
- Lines inside fenced code blocks (```` ``` ````) are skipped - CLI examples and
  code are allowed to be wide.

The top-level files are in scope deliberately. They were exempt for a long time,
and `CLAUDE.md` - the map every contributor and agent reads first - decayed the
furthest of any file in the repo, reaching a single 5,814-character line and
table cells holding whole specifications. An unlinted doc is where density goes
to hide.

## What a passing run does and does not mean

This checker is a floor, not a definition of legibility. A line under the cap can
still be an unreadable run-on; a lookup table's rows are *supposed* to be dense,
and breaking them up would destroy the grep-ability that is the point of a
reference table (see [`reference/functions.md`](../reference/functions.md)).
Read a green run as "nothing here is egregious", not as "this reads well".

The corollary matters more: **do not write up to the cap.** Prose that lands at
799 characters was written to satisfy this script rather than a reader. The cap
exists to catch the extreme; the target is a paragraph that carries one idea.

Cells are split naively on `|`; a pipe inside an inline code span only ever
splits a cell into smaller fragments, so the word count can under-report but
never false-positive - the safe direction for a commit gate (it never wrongly
blocks a within-budget cell).

## Usage

```bash
scripts/ci/check-doc-density.py                 # scan the whole corpus
scripts/ci/check-doc-density.py --staged        # only staged md files (hook mode)
scripts/ci/check-doc-density.py --quiet         # suppress the success summary line
scripts/ci/check-doc-density.py --max-cell-words 120 --max-line 700
```

Output is one `path:line: message` per violation. The checker **exits non-zero
when it finds violations**, so it can gate CI if wanted.

## Pre-commit wiring (hard gate)

The pre-commit hook (`scripts/git-hooks/pre-commit`, installed once per clone by
`scripts/ci/install-hooks.sh`) runs it on the staged doc set and aborts the commit
on a violation:

```bash
python3 scripts/ci/check-doc-density.py --staged --quiet || exit 1
```

Unlike the warn-only [`check-port-tags.py`](port-catalog.md), a density
violation blocks the commit. The check runs before the hook's Rust-only
early-exit so docs-only commits are covered too. Set `LEGAIA_SKIP_PRECOMMIT=1`
to bypass the whole hook in an emergency.

Pure standard library; ASCII-only; no external dependencies.

## Sibling gate: Markdown link + anchor checker

`scripts/ci/check-md-links.py` covers the same corpus as this checker and asks a
different question: does every relative link and `#anchor` actually resolve? It
slugs headings the way GitHub does (inline markup stripped, lowercased,
non-alphanumerics dropped, spaces to hyphens, duplicates suffixed `-1`, `-2`)
and matches link fragments against that set, plus explicit `<a name>` / `id`
anchors. External URLs are out of scope - checking them needs the network and
makes a gate flaky.

It exists because of a failure mode that review cannot catch: **a dead fragment
is not an error.** `page.md#no-such-heading` renders as a jump to the top of the
page, so a broken anchor looks like a working link and survives indefinitely.
The overwhelmingly common cause is one slug rule: a heading's `" - "` becomes
three hyphens (`---`), and links get written with two.

When a link and a heading disagree, **fix the link**. A heading is an anchor
target that other pages, and the site's hand-mirrored HTML, may already point at;
renaming it to satisfy one link breaks every other inbound reference.

```bash
python3 scripts/ci/check-md-links.py            # whole corpus
python3 scripts/ci/check-md-links.py --staged   # what the hook runs
```

## Sibling gate: site internal-link checker

`scripts/ci/check-site-links.py` is the same idea aimed at the static site:
it scans every generated page under `site/` (skipping the `_content/`
fragments) and fails on a relative `href`/`src` whose target file doesn't
exist or a fragment link (`page.html#anchor`, bare `#anchor`) whose element
id is absent from the target page. External URLs are out of scope.

It runs in two places:

- `python3 site/_gen.py` invokes it after regenerating, so a deploy with a
  broken internal link fails the build;
- the pre-commit hook runs it when staged changes touch `site/`.

Both exit non-zero on violations; fix the `_content/` fragment hrefs (the
generated pages mirror them) and regenerate. Pure standard library,
ASCII-only.
