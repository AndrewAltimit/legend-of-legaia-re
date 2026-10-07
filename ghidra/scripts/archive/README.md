# Archived dump scripts

One-off `dump_*` scripts written for a single investigation pass (a worklist
round, a gap sweep) and referenced by no doc or script. Their output is the
`funcs/` dumps they wrote; the reusable pattern is `dump_funcs.py` and the
per-overlay dumpers in `ghidra/scripts/` ([`docs/tooling/ghidra.md`](../../../docs/tooling/ghidra.md)).
They still run from here inside the container (`-postScript /scripts/archive/<name>.py`).
