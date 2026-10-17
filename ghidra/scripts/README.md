# Ghidra-side scripts

Analysis scripts that run inside the `ghidra` compose service, which mounts this
directory at `/scripts`. Run one by its absolute container path, e.g.
`-postScript /scripts/dump/dump_funcs.py`; the invocation rules (why the path is
absolute, how `lib/` is imported, the `-scriptPath` alternative) and the
per-script catalogue are in [`docs/tooling/ghidra.md`](../../docs/tooling/ghidra.md#script-layout-and-invocation).
Keep every `.py` here ASCII-only (Jython 2.7).

| Directory | Holds |
|---|---|
| `dump/` | Function dumpers: `dump_funcs.py` for SCUS, the per-overlay `dump_<label>_overlay.py` family, gap / worklist dumpers, `DumpEd710.java`. |
| `find/` | Searches: callers, xrefs, LUI+ADDIU materializers, `jal` sweeps, subsystem-targeted scanners, single-function traces. |
| `inventory/` | Project and program listings: `list_*`, program bases, per-overlay function inventories, the SCUS `explore.json` report. |
| `repair/` | Scripts that fix the Ghidra database or the dump corpus: truncated-dump rebuilds, forced disassembly, flow repair. |
| `symbols/` | `apply_known_symbols.py` and the committed `symbols.json` / `symbols.lua` name maps the PCSX-Redux probes load. |
| `import/` | Overlay capture and import: the PCSX-Redux `dump_overlay.lua` RAM grab and `import_overlay.sh`. |
| `lib/` | Modules imported by other scripts (`lib_dump.py`, `known_symbols.py`); not run on their own. |
| `archive/` | One-off investigation scripts, kept for reference ([`archive/README.md`](archive/README.md)). |
| `funcs/` | The per-function dump corpus the docs cite. Gitignored (Sony-derived); generated, never committed. |

Outputs (`funcs/`, `inventory_*.csv`, `explore.json`, the `redump_*.txt` and
`force_disasm_targets.txt` worklists) land at this top level and are gitignored.
