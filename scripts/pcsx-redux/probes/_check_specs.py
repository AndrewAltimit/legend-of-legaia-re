#!/usr/bin/env python3
"""Validator + Lua cross-check for scripts/pcsx-redux/probes/*.probe.toml.

What it does
============

For every ``*.probe.toml`` file in this directory:

1. Parses with Python's ``tomllib`` (ground truth).
2. Validates that the spec matches the schema enforced by
   ``scripts/pcsx-redux/lib/probe/spec.lua`` (required fields, mutually
   exclusive sections, capture-column vocabulary, breakpoint shape).
3. If ``lua5.1`` is on $PATH, also runs the file through
   ``scripts/pcsx-redux/lib/probe/toml.lua`` and asserts the structural
   shape matches Python's tomllib output. This catches divergence between
   our minimal Lua TOML reader and the canonical TOML spec.

Exit code is 0 on success, non-zero on any failure. Intended to run as a
manual / CI gate; no probe is launched.

Usage
=====

    python3 scripts/pcsx-redux/probes/_check_specs.py
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

PROBE_DIR = Path(__file__).resolve().parent
REPO_ROOT = PROBE_DIR.parents[2]
LIB_DIR = REPO_ROOT / "scripts" / "pcsx-redux" / "lib"

# Built-in capture-columns vocab. Must match COLUMN_BUILDERS in
# scripts/pcsx-redux/lib/probe/spec.lua.
CAPTURE_COLUMNS = {
    "tick", "addr", "offset", "pc", "ra", "sp", "width",
    "value_u8", "value_u16", "value_u32",
}

# Valid breakpoint kinds. Mirrors PCSX-Redux's PCSX.addBreakpoint API.
BP_KINDS = {"Exec", "Read", "Write"}


class SpecError(Exception):
    """Raised when a .probe.toml fails schema validation."""


def validate(spec: dict[str, Any], name: str) -> list[str]:
    """Return a list of warning strings; raise SpecError on schema errors."""
    warnings: list[str] = []

    def fail(msg: str) -> None:
        raise SpecError(f"{name}: {msg}")

    has_dump = "dump" in spec
    has_bp_list = bool(spec.get("breakpoint"))
    has_bp_range = bool(spec.get("breakpoint_range"))
    has_bps = has_bp_list or has_bp_range

    if has_dump and has_bps:
        fail("[dump] is mutually exclusive with [[breakpoint]]/[[breakpoint_range]]")
    if not has_dump and not has_bps:
        fail("no [dump], [[breakpoint]], or [[breakpoint_range]] section")

    if has_dump:
        d = spec["dump"]
        if not isinstance(d.get("addr"), int):
            fail("[dump].addr must be an integer")
        if not d.get("size_ram") and not isinstance(d.get("size"), int):
            fail("[dump] requires either size_ram = true or size = <int>")

    if has_bps:
        cols = spec.get("capture_columns") or []
        if not isinstance(cols, list):
            fail("capture_columns must be an array of strings")
        for c in cols:
            if c not in CAPTURE_COLUMNS:
                fail(f"unknown capture column '{c}' (vocab: {sorted(CAPTURE_COLUMNS)})")

    for i, b in enumerate(spec.get("breakpoint") or []):
        if not isinstance(b.get("addr"), int):
            fail(f"[[breakpoint]][{i}].addr must be an integer")
        kind = b.get("kind", "Exec")
        if kind not in BP_KINDS:
            fail(f"[[breakpoint]][{i}].kind '{kind}' invalid (allowed: {sorted(BP_KINDS)})")
        w = b.get("width", 4)
        if w not in (1, 2, 4):
            fail(f"[[breakpoint]][{i}].width must be 1/2/4")

    for i, r in enumerate(spec.get("breakpoint_range") or []):
        if not isinstance(r.get("base"), int):
            fail(f"[[breakpoint_range]][{i}].base must be an integer")
        if not isinstance(r.get("length"), int):
            fail(f"[[breakpoint_range]][{i}].length must be an integer")
        kind = r.get("kind", "Read")
        if kind not in BP_KINDS:
            fail(f"[[breakpoint_range]][{i}].kind '{kind}' invalid (allowed: {sorted(BP_KINDS)})")
        stride = r.get("stride", 4)
        if stride not in (1, 2, 4):
            fail(f"[[breakpoint_range]][{i}].stride must be 1/2/4")

    if "capture_frames" in spec and not isinstance(spec["capture_frames"], int):
        fail("capture_frames must be an integer")

    detail = spec.get("detail")
    if detail is not None:
        if not isinstance(detail.get("hits"), int) or detail["hits"] < 0:
            fail("[detail].hits must be a non-negative integer")

    # Scenario field is optional + informational. Cross-check against
    # scripts/scenarios.toml for a warning (not a hard error).
    if "scenario" in spec:
        scenarios_path = REPO_ROOT / "scripts" / "scenarios.toml"
        if scenarios_path.exists():
            with open(scenarios_path, "rb") as f:
                manifest = tomllib.load(f)
            labels = {s.get("label") for s in manifest.get("scenarios", [])}
            if spec["scenario"] not in labels:
                warnings.append(
                    f"scenario '{spec['scenario']}' not in scripts/scenarios.toml; "
                    f"users must pass --sstate or --scenario explicitly"
                )

    return warnings


def lua_roundtrip(path: Path) -> dict[str, Any] | None:
    """Parse via lib/probe/toml.lua and return the JSON-ish dump.

    Returns None when lua5.1 isn't available (skips the cross-check).
    """
    lua = shutil.which("lua5.1") or shutil.which("luajit")
    if lua is None:
        return None

    # arg[] isn't populated when lua is invoked with -e <code>; pass via env.
    script = r"""
        local lib_dir   = os.getenv("LEGAIA_LIB_DIR") or error("LEGAIA_LIB_DIR not set")
        local spec_path = os.getenv("LEGAIA_SPEC_PATH") or error("LEGAIA_SPEC_PATH not set")
        package.path = package.path .. ";" .. lib_dir .. "/?.lua"
        local toml = require("probe.toml")
        local t = toml.parse_file(spec_path)
        -- Emit JSON-ish output using a hand-rolled emitter (no cjson dep).
        local function emit(x)
            if type(x) == "table" then
                local is_array = (x[1] ~= nil) or (next(x) == nil)
                if is_array then
                    io.write("[")
                    for i, v in ipairs(x) do
                        if i > 1 then io.write(",") end
                        emit(v)
                    end
                    io.write("]")
                else
                    local keys = {}
                    for k in pairs(x) do keys[#keys+1] = k end
                    table.sort(keys, function(a,b) return tostring(a) < tostring(b) end)
                    io.write("{")
                    for i, k in ipairs(keys) do
                        if i > 1 then io.write(",") end
                        io.write(string.format("%q", k) .. ":")
                        emit(x[k])
                    end
                    io.write("}")
                end
            elseif type(x) == "string" then
                io.write(string.format("%q", x))
            elseif type(x) == "boolean" then
                io.write(tostring(x))
            elseif type(x) == "number" then
                -- Emit as integer if exact int; else float.
                if x == math.floor(x) and math.abs(x) < 1e16 then
                    io.write(string.format("%d", x))
                else
                    io.write(tostring(x))
                end
            else
                io.write("null")
            end
        end
        emit(t)
    """
    proc = subprocess.run(
        [lua, "-e", script],
        env={
            **__import__("os").environ,
            "LEGAIA_LIB_DIR": str(LIB_DIR),
            "LEGAIA_SPEC_PATH": str(path),
        },
        capture_output=True, text=True, check=False,
    )
    if proc.returncode != 0:
        raise SpecError(
            f"{path.name}: lua probe.toml.parse_file failed:\n{proc.stderr}"
        )
    return json.loads(proc.stdout)


def normalize_for_compare(x: Any) -> Any:
    """Normalise Python dicts/lists so they compare clean with Lua's output.

    The Lua emitter sorts keys alphabetically and emits JSON; Python's
    tomllib uses insertion order. After json.dumps(sort_keys=True),
    both should serialise identically -- but we still convert tuples and
    other non-JSON types here for safety.
    """
    if isinstance(x, dict):
        return {k: normalize_for_compare(v) for k, v in x.items()}
    if isinstance(x, list):
        return [normalize_for_compare(v) for v in x]
    return x


def main() -> int:
    specs = sorted(p for p in PROBE_DIR.glob("*.probe.toml"))
    if not specs:
        print(f"no .probe.toml files found under {PROBE_DIR}", file=sys.stderr)
        return 1

    failures: list[str] = []
    warnings_out: list[str] = []
    for path in specs:
        try:
            with open(path, "rb") as f:
                py = tomllib.load(f)
        except tomllib.TOMLDecodeError as e:
            failures.append(f"{path.name}: tomllib parse failed: {e}")
            continue

        try:
            warnings = validate(py, path.name)
            for w in warnings:
                warnings_out.append(f"{path.name}: {w}")
        except SpecError as e:
            failures.append(str(e))
            continue

        try:
            lua = lua_roundtrip(path)
        except SpecError as e:
            failures.append(str(e))
            continue

        if lua is not None:
            py_norm = json.loads(json.dumps(normalize_for_compare(py), sort_keys=True))
            lua_norm = json.loads(json.dumps(lua, sort_keys=True))
            if py_norm != lua_norm:
                failures.append(
                    f"{path.name}: lua/python TOML output diverges\n"
                    f"  python: {json.dumps(py_norm)}\n"
                    f"  lua:    {json.dumps(lua_norm)}"
                )
                continue

        suffix = " (lua cross-checked)" if lua is not None else " (lua unavailable)"
        print(f"OK  {path.name}{suffix}")

    for w in warnings_out:
        print(f"WARN {w}", file=sys.stderr)
    for f in failures:
        print(f"FAIL {f}", file=sys.stderr)

    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
