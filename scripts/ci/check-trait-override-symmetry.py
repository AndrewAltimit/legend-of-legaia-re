#!/usr/bin/env python3
"""Trait-override symmetry: do the hosts implement the same set of hooks?

`engine-core` hands the hosts their behaviour through traits, and several of
those traits give **every** method a default body. That is a deliberate
convenience - a test stub can implement `BgmDirector` with an empty block and
compile - and it is also a silent failure mode with no diff to look at:

    impl BgmDirector for WebBgmDirector<'_> {
        fn start(&mut self, id: u16, seq: &[u8]) { ... }
        // start_owned_vab never written -> every global-pool track,
        // which is every real music cue, silently plays nothing.
    }

Nothing about that is a compile error, a clippy lint, or a visible diff. It
is a method that was never typed. The native host has the same shape available
to it, and the same silence.

This gate is the syntactic check that closes it: **for every `engine-core`
trait with default method bodies that more than one host implements, the set
of overridden methods must match across the implementers.** No call graph, no
runtime, no model of what a method does - just "both hosts wrote the same
hooks, or a recorded reason says why not".

## What it does and does not prove

* it DOES prove that a defaulted hook one implementer overrides is overridden
  by every other implementer of the same trait, so a hook cannot be added to
  one host and forgotten on the other;
* it DOES notice a hook that is added to the trait and then adopted by only
  some implementers;
* it does NOT prove the two overrides *do* the same thing, that either is
  correct, or that a hook nobody overrides should be overridden. A method both
  hosts leave defaulted is invisible here, and deliberately so - that is a
  host-identical gap, not drift.

The last point is the honest limit. `BgmDirector::reattach_volume` is
defaulted on both hosts today; this gate says nothing about it, because
nothing about the two hosts differs. Reachability of that hook is the port
catalog's question, not this one.

## Scope

Traits: every `pub trait` under `crates/engine-core/src` that declares at
least one method **with a body**. A trait whose methods are all required
(`fn f(&self);`) cannot exhibit this failure - the compiler already enforces
symmetry - so it is not measured.

Implementers: `impl Trait for Type` blocks in the shipped host sources.
`engine-render` counts as native, as in `check-ui-host-drift.py`, and
`tests/` paths are excluded: a test stub implementing three of eight methods
is the intended use of the defaults, not a gap between hosts.

Comparison runs **pairwise over every implementer**, not host-against-host,
and both directions of divergence are reported. Intra-host asymmetry is a
finding in its own right, which is what surfaced the `audio-trace` oracle's
missing owned-VAB hooks: two directors in one crate, one of them silently
modelling a strictly smaller sequencer.

## Waivers

`scripts/ci/trait-override-waivers.toml`, one entry per implementer that may
legitimately carry a smaller override set, each with a non-empty `reason`.
Validated in both directions, exactly as the UI-drift waivers are:

* a waiver naming a type that implements nothing   -> fail (stale)
* a waiver for a type that now matches its peers   -> fail (close it out)
* an empty `reason`                                -> fail

A waiver is not a claim that the gap is harmless. It is a record of what
diverges and what would have to exist to close it, which is the only form of
exemption this repo's gates accept.

Usage:

    python3 scripts/ci/check-trait-override-symmetry.py            # check
    python3 scripts/ci/check-trait-override-symmetry.py --quiet    # findings only
    python3 scripts/ci/check-trait-override-symmetry.py --list     # full table
    python3 scripts/ci/check-trait-override-symmetry.py --selftest # controls

Exit status: 0 = symmetric or waived, 1 = asymmetry / stale waiver,
2 = self-test failed.
"""

import argparse
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

REPO = Path(__file__).resolve().parent.parent.parent
CORE_SRC = REPO / "crates" / "engine-core" / "src"
WAIVERS = Path(__file__).resolve().parent / "trait-override-waivers.toml"

# Same host split as check-ui-host-drift.py: engine-render re-exports engine-ui
# and hosts native-side adapters, so an impl there is the native window's.
HOSTS = {
    "native": [
        REPO / "crates" / "engine-shell" / "src",
        REPO / "crates" / "engine-render" / "src",
    ],
    "web": [REPO / "crates" / "web-viewer" / "src"],
}

LINE_COMMENT_RE = re.compile(r"//.*$", re.MULTILINE)
TRAIT_RE = re.compile(r"\bpub trait\s+(?P<name>[A-Z][A-Za-z0-9_]*)")
FN_RE = re.compile(r"\bfn\s+(?P<name>[a-z0-9_]+)\s*[<(]")

# `impl [<generics>] [path::]Trait[<args>] for Type {`. The trait path is
# allowed to be qualified because the web host writes
# `impl legaia_engine_core::scene::BgmDirector for WebBgmDirector<'_>`, and a
# pattern that only matched the bare name would have silently dropped exactly
# the host this gate exists to compare.
IMPL_RE = re.compile(
    r"\bimpl\s*(?:<[^>]*>)?\s*"
    r"(?:[A-Za-z0-9_]+\s*::\s*)*"
    r"(?P<trait>[A-Z][A-Za-z0-9_]*)"
    r"(?:<[^>{]*>)?"
    r"\s+for\s+(?P<type>[^{]+?)\s*\{"
)


def strip_line_comments(text: str) -> str:
    """Drop `//` comments so a doc line naming a method is not an override."""
    return LINE_COMMENT_RE.sub("", text)


def signature_end(text: str, start: int) -> int:
    """Index of the body's `{` for the `fn` at `text[start]`, or -1 if none.

    Bracket-depth aware, so `-> [f32; 4] {` is a body and `fn f(&self);` is
    not. This is what separates a defaulted trait method from a required one,
    so a wrong answer here silently changes what the gate measures.
    """
    i, n, depth = start, len(text), 0
    while i < n:
        c = text[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c == "{":
            return i
        elif depth == 0 and c == ";":
            return -1
        i += 1
    return -1


def block_body(text: str, brace: int) -> str:
    """Source between `text[brace]` and its matching close brace.

    String and char literals are skipped so a brace inside `format!("{}")`
    cannot unbalance the scan.
    """
    n = len(text)
    i, depth = brace, 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                term = '"' + "#" * (j - i - 1)
                end = text.find(term, j + 1)
                i = n if end < 0 else end + len(term)
                continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
        i += 1
    return text[brace + 1 :]


def is_test_source(path: Path) -> bool:
    """Path-only test detection, matching check-ui-host-drift.py.

    A test stub implementing three of eight methods is the *intended* use of
    a defaulted trait, so counting it would make this gate fire on exactly
    the case the defaults exist for.
    """
    return "tests" in path.parts or path.name == "tests.rs"


def collect_defaulted_traits() -> dict[str, tuple[set[str], str]]:
    """Map trait name -> (defaulted method names, defining `path:line`).

    Only traits with at least one defaulted method are returned; a trait whose
    methods are all required cannot drift, because the compiler enforces it.
    """
    out: dict[str, tuple[set[str], str]] = {}
    for path in sorted(CORE_SRC.rglob("*.rs")):
        if is_test_source(path):
            continue
        text = strip_line_comments(path.read_text(encoding="utf-8"))
        for m in TRAIT_RE.finditer(text):
            brace = text.find("{", m.end())
            if brace < 0:
                continue
            body = block_body(text, brace)
            defaulted = {
                fm.group("name")
                for fm in FN_RE.finditer(body)
                if signature_end(body, fm.start()) >= 0
            }
            if defaulted:
                line = text[: m.start()].count("\n") + 1
                out[m.group("name")] = (defaulted, f"{path.relative_to(REPO)}:{line}")
    return out


def collect_impls(traits: dict[str, tuple[set[str], str]]) -> dict[str, list[dict]]:
    """Map trait name -> the host impls of it, with their override sets."""
    found: dict[str, list[dict]] = {name: [] for name in traits}
    for host, roots in HOSTS.items():
        for root in roots:
            if not root.is_dir():
                continue
            for path in sorted(root.rglob("*.rs")):
                if is_test_source(path):
                    continue
                text = strip_line_comments(path.read_text(encoding="utf-8"))
                for m in IMPL_RE.finditer(text):
                    trait = m.group("trait")
                    if trait not in traits:
                        continue
                    brace = text.index("{", m.end() - 1)
                    body = block_body(text, brace)
                    overrides = {fm.group("name") for fm in FN_RE.finditer(body)}
                    line = text[: m.start()].count("\n") + 1
                    found[trait].append(
                        {
                            "host": host,
                            "type": " ".join(m.group("type").split()),
                            "where": f"{path.relative_to(REPO)}:{line}",
                            "overrides": overrides & traits[trait][0],
                        }
                    )
    return found


def load_waivers() -> dict[tuple[str, str], dict]:
    if not WAIVERS.is_file():
        return {}
    data = tomllib.loads(WAIVERS.read_text(encoding="utf-8"))
    out: dict[tuple[str, str], dict] = {}
    for entry in data.get("waiver", []):
        trait, ty = entry.get("trait"), entry.get("type")
        if trait and ty:
            out[(trait, ty)] = entry
    return out


# Detector control suite. Real shapes, so a refactor that breaks the parser
# fails loudly instead of reporting a clean tree it never read.
SELFTEST_SIGNATURES: list[tuple[str, str, bool]] = [
    ("defaulted method has a body", "fn pause(&mut self) {}", True),
    ("required method has none", "fn tick(&mut self, dt: u32);", False),
    ("array return type's `;` is not a terminator", "fn tint(&self) -> [f32; 4] { [0.0; 4] }", True),
    ("defaulted generic method", "fn any<T: Copy>(&self, v: T) -> u8 { 0 }", True),
]

SELFTEST_IMPLS: list[tuple[str, str, str | None, str | None]] = [
    ("bare trait name",
     "impl BgmDirector for AudioBgmDirector {", "BgmDirector", "AudioBgmDirector"),
    ("fully qualified trait path",
     "impl legaia_engine_core::scene::BgmDirector for WebBgmDirector<'_> {",
     "BgmDirector", "WebBgmDirector<'_>"),
    ("lifetime generics on the impl",
     "impl<'a> CdDmaHost for DiscHost<'a> {", "CdDmaHost", "DiscHost<'a>"),
    ("inherent impl is not a trait impl", "impl AudioBgmDirector {", None, None),
    ("trait definition is not a trait impl", "pub trait BgmDirector {", None, None),
]


def run_selftest() -> int:
    failures = 0
    for label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) == want:
            print(f"  ok    signature: {label}")
        else:
            print(f"  FAIL  signature: {label}")
            failures += 1
    for label, src, want_trait, want_type in SELFTEST_IMPLS:
        m = IMPL_RE.search(src)
        got_trait = m.group("trait") if m else None
        got_type = " ".join(m.group("type").split()) if m else None
        if got_trait == want_trait and got_type == want_type:
            print(f"  ok    impl: {label}")
        else:
            print(f"  FAIL  impl: {label} - parsed {got_trait!r} for {got_type!r}")
            failures += 1
    total = len(SELFTEST_SIGNATURES) + len(SELFTEST_IMPLS)
    if failures:
        print(
            f"\nself-test: {failures} of {total} case(s) failed - this gate cannot "
            f"read the shapes it measures, so a clean run means nothing"
        )
        return 2
    print(f"\nself-test: all {total} cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quiet", action="store_true", help="findings only")
    ap.add_argument("--list", action="store_true", help="print every impl and its overrides")
    ap.add_argument("--selftest", action="store_true", help="run the parser controls and exit")
    args = ap.parse_args()

    if args.selftest:
        print("check-trait-override-symmetry self-test")
        return run_selftest()

    # Run the controls every time. A "0 asymmetries" verdict from a parser that
    # matched no impls at all is not a measurement.
    for _label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) != want:
            print("ERROR: signature control failed. Run --selftest.", file=sys.stderr)
            return 2
    for _label, src, want_trait, _want_type in SELFTEST_IMPLS:
        m = IMPL_RE.search(src)
        if (m.group("trait") if m else None) != want_trait:
            print("ERROR: impl-parser control failed. Run --selftest.", file=sys.stderr)
            return 2

    traits = collect_defaulted_traits()
    impls = collect_impls(traits)
    waivers = load_waivers()

    # Only traits with more than one host implementer can drift.
    compared = {t: rows for t, rows in impls.items() if len(rows) > 1}

    problems: list[str] = []
    matched_waivers: set[tuple[str, str]] = set()

    for trait, rows in sorted(compared.items()):
        defaulted, decl = traits[trait]
        # The reference set is the largest override set among the unwaived
        # implementers: the most complete adapter is what the others owe.
        # Taking the union instead would demand hooks nobody has written.
        unwaived = [r for r in rows if (trait, r["type"]) not in waivers]
        pool = unwaived or rows
        reference = max((r["overrides"] for r in pool), key=len)
        for row in rows:
            missing = reference - row["overrides"]
            key = (trait, row["type"])
            if key in waivers:
                matched_waivers.add(key)
                if not missing:
                    problems.append(
                        f"STALE WAIVER {trait} for {row['type']}: it now overrides "
                        f"everything its peers do. Drop the waiver from "
                        f"{WAIVERS.relative_to(REPO)}."
                    )
                elif not str(waivers[key].get("reason", "")).strip():
                    problems.append(
                        f"WAIVER {trait} for {row['type']}: needs a non-empty `reason`."
                    )
                continue
            if missing:
                peers = ", ".join(
                    f"{r['type']} ({r['host']})" for r in rows if r["overrides"] >= reference
                )
                problems.append(
                    f"ASYMMETRY {trait} for {row['type']} ({row['host']}, "
                    f"{row['where']}): does not override "
                    f"{', '.join(sorted(missing))}, which {peers} does. "
                    f"The trait defaults every method ({decl}), so the hook is a "
                    f"silent no-op here - no compile error, no diff. Implement it, "
                    f"or record why not in {WAIVERS.relative_to(REPO)}."
                )

    # Stale-waiver validation: a waiver naming an impl that no longer exists.
    live = {(t, r["type"]) for t, rows in impls.items() for r in rows}
    for key in sorted(waivers):
        if key not in live:
            problems.append(
                f"STALE WAIVER {key[0]} for {key[1]}: no such host impl "
                f"(renamed or deleted?). Drop the waiver."
            )

    if args.list:
        for trait, rows in sorted(impls.items()):
            if not rows:
                continue
            print(f"{trait}  (defaulted: {', '.join(sorted(traits[trait][0]))})")
            for r in sorted(rows, key=lambda r: (r["host"], r["type"])):
                mark = "W" if (trait, r["type"]) in waivers else " "
                print(
                    f"  {mark} {r['host']:<7} {r['type']:<28} "
                    f"{','.join(sorted(r['overrides'])) or '-'}   {r['where']}"
                )

    if not args.quiet:
        n_impls = sum(len(rows) for rows in impls.values())
        print(
            f"[trait-symmetry] engine-core traits with default bodies: {len(traits)} "
            f"({len(compared)} implemented more than once by the hosts, "
            f"{n_impls} host impls in total)"
        )

    if problems:
        print(f"\n[trait-symmetry] {len(problems)} problem(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1

    if not args.quiet:
        print("[trait-symmetry] ok - every multiply-implemented hook set agrees or is waived")
    return 0


if __name__ == "__main__":
    sys.exit(main())
