# docs/ - working notes

Loaded when you work under `docs/`. Public-facing technical reference, topic-first; no progress tracker, session log or status tables.

## Where a page is indexed

| Section | Index page |
|---|---|
| Formats | [`formats/overview.md`](formats/overview.md) |
| Subsystems | [`subsystems/README.md`](subsystems/README.md) |
| Tooling | [`tooling/README.md`](tooling/README.md) |
| Reference | [`reference/README.md`](reference/README.md) |
| Guides | [`guides/`](guides/) (also mirrored on the site's guides section) |

A new page gets a row in its section's index page, not in the root `CLAUDE.md`: that file keeps only the rules, the commands and the cross-cutting traps every session needs.

## Gates that bite here

- `scripts/ci/check-doc-density.py` (>800-char lines, >150-word cells) and `scripts/ci/check-md-links.py` (relative links + `#anchors`): [`tooling/doc-density.md`](tooling/doc-density.md).
- `scripts/ci/check-site-doc-mirrors.py`: some pages (the open / settled / falsified registers among them) have `site/_content/` mirrors that must claim every `##` heading and live row. Renaming a heading there breaks the mirror.
- `scripts/ci/check-register-address-cites.py` on the reference registers.

The writing rules (present tense, no dates or session markers, provenance citations, no prose in cells) are in the root `CLAUDE.md`, since they also cover crate READMEs.
