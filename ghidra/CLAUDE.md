# ghidra/ - working notes

Loaded when you work under `ghidra/`. The script catalogue and per-query invocations live in [`docs/tooling/ghidra.md`](../docs/tooling/ghidra.md); the bucket map in [`scripts/README.md`](scripts/README.md).

## The container

`docker-compose.yml` defines a single `ghidra` service, built from `docker/ghidra.Dockerfile` - a UID/GID-matched wrapper over `blacktop/ghidra` (pinned by digest) so dumps come back owned by the host user:

- `./extracted:/data:ro` - disc-extracted files (read-only into Ghidra).
- `./ghidra/projects:/projects` - Ghidra project DB (gitignored; local only).
- `./ghidra/scripts:/scripts` - analysis scripts (read-write so dumps land back on host).

Workflow: `docker compose up -d ghidra` once, then `docker compose exec ghidra /ghidra/support/analyzeHeadless ...` per query. Don't restart the service per command.

## Adding dumps

To add a new function dump, edit the `TARGETS` list in `scripts/dump/dump_funcs.py` and run the post-script - output lands in `scripts/funcs/<addr>.txt` (gitignored: dumps are Sony-derived). Then update [`docs/reference/functions.md`](../docs/reference/functions.md) if the entry point is notable.

For overlay-specific dumps use per-overlay scripts (e.g. `dump_shop_overlay.py`, `dump_levelup_overlay.py`, `dump_cutscene_overlay.py`, `dump_str_fmv_overlay.py`): a header, a `TARGETS` list and one `dump_targets(currentProgram, TARGETS)` call into the shared `lib/lib_dump.py`, which skips addresses not in the current program and names output `overlay_<label>_<addr>.txt`. Run with `-process overlay_<label>.bin -noanalysis -postScript /scripts/dump/dump_<label>_overlay.py`. One-off investigation scripts live in `scripts/archive/`.

## Traps

- Jython 2.7 (Ghidra-bundled) chokes on Unicode in source unless an encoding declaration is added - keep `scripts/**/*.py` ASCII-only.
- LUI+ADDIU pairs are not auto-resolved by the reference manager: use `scripts/find/find_lui_writers.py` (edit `LO`/`HI`).
- A dump's printed addresses are a property of its load base; only the header tag is evidence the base is right ([`dump-corpus-integrity.md`](../docs/tooling/dump-corpus-integrity.md)).
- Port and document from the disassembly, not the decompiled C ([`ghidra.md`](../docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)).
