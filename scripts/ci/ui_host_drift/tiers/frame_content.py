"""Tier 12 analysis: do two paired frame kernels call the same engine?

Rows: `ui_host_drift.rows.frame_content`.
"""

import re

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

from ui_host_drift.roots import HOSTS, REPO, WAIVERS
from ui_host_drift.rows.frame_content import (
    CONTENT_WALK_STOPS,
    ENGINE_API_CRATES,
    STD_METHOD_NAMES,
)
from ui_host_drift.rows.frame_paths import FRAME_PATHS
from ui_host_drift.rust_text import (
    brace_block,
    is_test_source,
    signature_end,
    strip_comments,
)
from ui_host_drift.tiers.frame_paths import frame_path_scan, load_frame_waivers

CONTENT_PUB_FN_RE = re.compile(
    r"\bpub(?:\s*\([^)]*\))?\s+(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+"
    r"([a-z_][a-z_0-9]*)"
)
CONTENT_FN_RE = re.compile(r"\bfn\s+([a-z_][a-z_0-9]*)")
METHOD_CALL_RE = re.compile(r"[.:]\s*([a-z_][a-z_0-9]*)\s*\(")
SELF_METHOD_RE = re.compile(r"\bself\.([a-z_][a-z_0-9]*)\s*\(")
# A helper reached through the page's actor layer (`self.actors.drive(`): the
# delegate is the host's own code (`web-play::field_actors`, shared with the
# map viewer), so the subtree it reaches is the kernel's. Named field only - a
# general `self.<field>.<method>(` edge would follow every engine-type field
# into same-named host fns.
SELF_FIELD_METHOD_RE = re.compile(r"\bself\.actors\.([a-z_][a-z_0-9]*)\s*\(")

# How far a host helper chain is followed out of a kernel body.
CONTENT_DEPTH = 8


def engine_api_names() -> set[str]:
    """Every `pub fn` name the four shared engine crates define, minus the
    std-collision set."""
    out: set[str] = set()
    for crate in ENGINE_API_CRATES:
        root = REPO / "crates" / crate / "src"
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            text = strip_comments(path.read_text(encoding="utf-8"))
            out |= set(CONTENT_PUB_FN_RE.findall(text))
    return out - STD_METHOD_NAMES


def host_fn_bodies(host: str) -> dict[str, list[str]]:
    """Every `fn` body this host's shipped sources define, by name."""
    out: dict[str, list[str]] = {}
    for root in HOSTS[host]:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            text = strip_comments(path.read_text(encoding="utf-8"))
            for m in CONTENT_FN_RE.finditer(text):
                brace = signature_end(text, m.start())
                if brace < 0:
                    continue
                start, end = brace_block(text, brace)
                out.setdefault(m.group(1), []).append(text[start:end])
    return out


def kernel_engine_calls(
    kernel: str, bodies: dict[str, list[str]], api: set[str]
) -> tuple[set[str], int, bool]:
    """`(engine call names, host fns walked, kernel body found)`.

    Follows `self.<helper>(` edges inside the host's own sources, so a kernel
    that delegates is compared by what its whole subtree reaches.
    """
    own = set(bodies)
    seen: set[str] = set()
    calls: set[str] = set()
    stack = [(kernel, 0)]
    found = False
    while stack:
        name, depth = stack.pop()
        if name in seen or depth > CONTENT_DEPTH:
            continue
        seen.add(name)
        for body in bodies.get(name, []):
            if name == kernel:
                found = True
            for c in METHOD_CALL_RE.findall(body):
                if c in api and c not in own:
                    calls.add(c)
            for c in SELF_METHOD_RE.findall(body) + SELF_FIELD_METHOD_RE.findall(body):
                if c in own and c not in CONTENT_WALK_STOPS:
                    stack.append((c, depth + 1))
    return calls, len(seen), found


def load_content_waivers() -> list[dict]:
    if not WAIVERS.is_file():
        return []
    return tomllib.loads(WAIVERS.read_text(encoding="utf-8")).get("frame_content", [])


def frame_kernel_pairs() -> list[tuple[str, str]]:
    """Every paired kernel across the two frame paths: same name on both, or
    an alias row. Derived, so a new pair joins by existing."""
    native = {n for n, _ in frame_path_scan("native")[0]}
    web = {n for n, _ in frame_path_scan("web")[0]}
    pairs = [(n, n) for n in sorted(native & web)]
    for entry in load_frame_waivers()[1]:
        if entry.get("host_only"):
            continue
        a, b = str(entry.get("native")), str(entry.get("web"))
        if a in native and b in web:
            pairs.append((a, b))
    return sorted(set(pairs))


def check_frame_content() -> tuple[list[str], list[str], int]:
    """Every paired frame kernel: equal engine call sets, or a waiver."""
    problems: list[str] = []
    notes: list[str] = []
    api = engine_api_names()
    if len(api) < 100:
        problems.append(
            "FRAME CONTENT: the engine API scan found almost no `pub fn` - "
            "the comparison below would report every pair clean for the "
            "wrong reason. Check `ENGINE_API_CRATES`."
        )
        return problems, notes, 0
    bodies = {host: host_fn_bodies(host) for host in FRAME_PATHS}
    waivers = load_content_waivers()
    used: set[int] = set()
    pairs = frame_kernel_pairs()
    for a, b in pairs:
        ca, walked_a, found_a = kernel_engine_calls(a, bodies["native"], api)
        cb, walked_b, found_b = kernel_engine_calls(b, bodies["web"], api)
        for host, name, found in (("native", a, found_a), ("web", b, found_b)):
            if not found:
                problems.append(
                    f"FRAME CONTENT: the {host} frame path calls "
                    f"`{name}` but no body for it was found in that host's "
                    f"sources - the pair below cannot be compared."
                )
        only_a = sorted(ca - cb)
        only_b = sorted(cb - ca)
        if not only_a and not only_b:
            notes.append(
                f"content pair `{a}` <-> `{b}`: identical engine call set "
                f"({len(ca)} calls, {walked_a}/{walked_b} host fns walked)"
            )
            continue
        entry = None
        for n, row in enumerate(waivers):
            if str(row.get("native")) == a and str(row.get("web")) == b:
                entry, index = row, n
                break
        if entry is None:
            problems.append(
                f"FRAME CONTENT `{a}` <-> `{b}`: the two bodies reach "
                f"different engine calls - native-only {only_a}, web-only "
                f"{only_b}. Wire the missing half, or declare the difference "
                f"with a `[[frame_content]]` row naming both lists and a "
                f"reason."
            )
            continue
        used.add(index)
        want_a = sorted(str(x) for x in entry.get("native_only", []))
        want_b = sorted(str(x) for x in entry.get("web_only", []))
        if want_a != only_a or want_b != only_b:
            problems.append(
                f"STALE FRAME-CONTENT WAIVER `{a}` <-> `{b}`: the difference "
                f"moved. Now native-only {only_a}, web-only {only_b}; the row "
                f"says {want_a} / {want_b}. Re-derive it, or close the gap."
            )
        elif not str(entry.get("reason", "")).strip():
            problems.append(
                f"FRAME-CONTENT WAIVER `{a}` <-> `{b}`: needs a non-empty "
                f"`reason`."
            )
        else:
            notes.append(
                f"content pair `{a}` <-> `{b}`: "
                f"{len(only_a)} native-only / {len(only_b)} web-only, "
                f"declared - {entry['reason']}"
            )
    for n, row in enumerate(waivers):
        if n not in used:
            problems.append(
                f"STALE FRAME-CONTENT WAIVER `{row.get('native')}` <-> "
                f"`{row.get('web')}`: the pair is gone or its two bodies now "
                f"reach the same engine calls. Drop the row."
            )
    return problems, notes, len(pairs)
