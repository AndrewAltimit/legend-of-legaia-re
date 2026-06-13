# Doc legibility-density checker

`scripts/ci/check-doc-density.py` keeps the committed documentation read-optimized
by flagging the two patterns that make long-lived docs hard to skim:

- **Over-long lines** — any line wider than `--max-line` characters (default
  `800`). Usually a run-on sentence or an over-stuffed table row.
- **Over-budget table cells** — any single markdown table cell holding more than
  `--max-cell-words` words (default `150`). The fix is to move the cell body into
  a dedicated section (same page) or a sub-page and leave a one-line summary +
  link in the cell.

It is the durable guard behind the one-fact-per-cell / no-wall-of-prose
convention: same information, more navigable structure.

## Scope

- Every `docs/**/*.md`.
- Every top-level `crates/<name>/README.md`.
- The generated `crates/web-viewer/pkg/README.md` is skipped.
- Lines inside fenced code blocks (```` ``` ````) are skipped — CLI examples and
  code are allowed to be wide.

Cells are split naively on `|`; a pipe inside an inline code span only ever
splits a cell into smaller fragments, so the word count can under-report but
never false-positive — the safe direction for a commit gate (it never wrongly
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
