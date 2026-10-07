# Archived Ghidra scripts

One-off scripts written for a single investigation pass (a worklist round, a
gap sweep, one global's writer hunt) and referenced by no doc or script: the
`dump_*` target lists, and the `find_*` / `xref_*` / `analyze_*` searches
each written to answer one question. The dumpers' output is the `funcs/`
dumps they wrote; the reusable pattern is `dump_funcs.py` and the
per-overlay dumpers in `ghidra/scripts/` ([`docs/tooling/ghidra.md`](../../../docs/tooling/ghidra.md)).
They still run from here inside the container (`-postScript /scripts/archive/<name>.py`).
