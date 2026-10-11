"""Tier 11 analysis: does an early exit skip a kernel the draw still reads?

Rows: `ui_host_drift.rows.frame_paths`.
"""

import re

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

from ui_host_drift.roots import REPO, WAIVERS
from ui_host_drift.rows.frame_paths import (
    FRAME_PATHS,
    FRAME_PATH_NON_KERNELS,
    WEB_PAGE_DRAW_CALL,
    WEB_PAGE_FRAME,
    WEB_PAGE_FRAME_FN,
    WEB_PAGE_TICK_CALL,
)
from ui_host_drift.rust_text import brace_block, strip_all_comments, strip_comments
from ui_host_drift.tiers.sim_pairs import splice_native_steps

SELF_CALL_RE = re.compile(r"self\.([a-z_][a-z_0-9]*)\s*\(")
EXIT_WORD_RE = re.compile(r"\b(continue|return)\b")
# An arm's header starts at the line-start keyword that opens it. Scanning
# back to the nearest `;`/`{`/`}` instead is not enough: `if let
# SceneTickEvent::SceneEntered { name } = event {` carries braces inside its
# own pattern, and the nearest-brace rule reports that arm as `= event`.
ARM_HEAD_RE = re.compile(
    r"(?m)^[ \t]*\}?\s*"
    r"((?:else\s+if\s+let|else\s+if|else|if\s+let|if|while\s+let|while|let|match)\b)"
)
# How far back an arm header may reasonably start. Past this the nearest-
# boundary rule is used instead, so a pathological span cannot swallow the
# statement above it.
ARM_HEAD_WINDOW = 600


def normalise_condition(src: str) -> str:
    """Collapse an arm header to one line, for matching and for printing."""
    return " ".join(src.split())


def frame_path_scan(host: str) -> tuple[list[tuple[str, int]], list[dict]]:
    """`(kernels, arms)` for one host's frame path.

    `kernels` is the fall-through list: every `self.<name>(` call at the
    body's own nesting depth, in source order. `arms` is one record per early
    exit nested inside it, carrying the arm's header text, the calls made
    within the arm's block, and the fall-through kernels it therefore skips.
    """
    spec = FRAME_PATHS[host]
    text = spec.get("_source")
    if text is None:
        text = strip_comments((REPO / spec["path"]).read_text(encoding="utf-8"))
    body_start, body_end = brace_block(text, text.index(spec["anchor"]))
    body = text[body_start:body_end]
    if spec.get("splice"):
        body = splice_native_steps(body)
    base_line = text.count("\n", 0, body_start) + 1

    # One pass: nesting depth, the stack of open-brace offsets, every call
    # site and every exit word.
    depth = 0
    stack: list[int] = []
    line = base_line
    kernels: list[tuple[str, int]] = []
    calls: list[tuple[int, str]] = []  # (offset, name)
    exits: list[tuple[int, int]] = []  # (enclosing block `{` offset, line)
    i = 0
    while i < len(body):
        ch = body[i]
        if ch == "\n":
            line += 1
            i += 1
            continue
        if ch == "{":
            depth += 1
            stack.append(i)
            i += 1
            continue
        if ch == "}":
            depth -= 1
            if stack:
                stack.pop()
            i += 1
            continue
        m = SELF_CALL_RE.match(body, i)
        if m:
            name = m.group(1)
            if name not in FRAME_PATH_NON_KERNELS:
                calls.append((i, name))
                if depth == 0:
                    kernels.append((name, line))
            i = m.end() - 1
            continue
        m2 = EXIT_WORD_RE.match(body, i)
        if m2:
            prev = body[i - 1] if i else " "
            if m2.group(1) == spec["exit"] and not (prev.isalnum() or prev == "_"):
                # Only an exit nested in an arm can skip anything; one at the
                # body's own depth is the path's end, not a short circuit.
                if depth >= 1 and stack:
                    exits.append((stack[-1], line))
            i = m2.end()
            continue
        i += 1

    arms: list[dict] = []
    seen_blocks: set[int] = set()
    for block_open, exit_line in exits:
        if block_open in seen_blocks:
            continue
        seen_blocks.add(block_open)
        # The arm header is the source from the end of the previous statement
        # or block up to this block's `{` - `if <cond>`, `if let <pat> = <e>`,
        # `let <pat> = <e> else`, `else`.
        head_from = max(
            body.rfind(";", 0, block_open),
            body.rfind("{", 0, block_open),
            body.rfind("}", 0, block_open),
        )
        window = max(0, block_open - ARM_HEAD_WINDOW)
        # The last line-start keyword whose span up to the arm's `{` is
        # brace-balanced. Balance is what separates a pattern's own braces
        # (`SceneEntered { name }`, balanced) from an enclosing block that
        # merely happens to start with a keyword (`match m {`, unbalanced).
        keyword = None
        for m in ARM_HEAD_RE.finditer(body, window, block_open):
            # From the KEYWORD, not the match: the optional `}` prefix that
            # lets `} else {` be found would otherwise unbalance every span.
            span = body[m.start(1) : block_open]
            if span.count("{") == span.count("}"):
                keyword = m.start(1)
        if keyword is not None:
            head_from = keyword - 1
        header = normalise_condition(body[head_from + 1 : block_open])
        block_end = brace_block(body, block_open)[1]
        inside = {n for off, n in calls if block_open < off < block_end}
        # A kernel the fall-through path runs after this arm's exit, and the
        # arm does not run itself, is skipped for as long as the arm is taken.
        skipped = [n for n, kline in kernels if kline > exit_line and n not in inside]
        arms.append(
            {
                "header": header,
                "line": exit_line,
                "inside": inside,
                "skips": sorted(dict.fromkeys(skipped)),
            }
        )
    return kernels, arms


def page_frame_gates() -> tuple[list[str], bool]:
    """`(guards, draw_call_found)` for the page's frame function.

    `guards` is every `if (...)` condition the WASM per-frame call sits
    inside and the overlay draw does not. An empty list means the page ticks
    and draws under the same conditions, which is the shape that needs no
    disclosure.
    """
    text = strip_all_comments((REPO / WEB_PAGE_FRAME).read_text(encoding="utf-8"))
    body_start, body_end = brace_block(text, text.index(WEB_PAGE_FRAME_FN))
    body = text[body_start:body_end]
    tick_at = body.index(WEB_PAGE_TICK_CALL)
    draw_at = body.rfind(WEB_PAGE_DRAW_CALL)

    # The stack of open blocks at a given offset, as (block_open, header).
    def open_blocks(at: int) -> list[tuple[int, str]]:
        stack: list[int] = []
        for i, ch in enumerate(body[:at]):
            if ch == "{":
                stack.append(i)
            elif ch == "}" and stack:
                stack.pop()
        out = []
        for open_at in stack:
            head_from = max(
                body.rfind(";", 0, open_at),
                body.rfind("{", 0, open_at),
                body.rfind("}", 0, open_at),
            )
            out.append((open_at, normalise_condition(body[head_from + 1 : open_at])))
        return out

    tick_stack = open_blocks(tick_at)
    draw_stack = {b for b, _ in open_blocks(draw_at)} if draw_at >= 0 else set()
    guards = [
        head
        for block_open, head in tick_stack
        if block_open not in draw_stack and head.startswith("if")
    ]
    return guards, draw_at >= 0


def load_frame_waivers() -> tuple[list[dict], list[dict]]:
    if not WAIVERS.is_file():
        return [], []
    data = tomllib.loads(WAIVERS.read_text(encoding="utf-8"))
    return data.get("frame_arm", []), data.get("frame_kernel", [])


def check_frame_paths() -> tuple[list[str], list[str], dict[str, int]]:
    problems: list[str] = []
    notes: list[str] = []
    arm_waivers, kernel_waivers = load_frame_waivers()
    scans = {host: frame_path_scan(host) for host in FRAME_PATHS}
    counts = {host: len(scans[host][0]) for host in FRAME_PATHS}
    # The page's JS gate is an arm of the browser host like any other; it
    # skips the WHOLE Rust frame path, so its skip list is that path.
    page_guards, page_draw_found = page_frame_gates()
    if page_guards:
        scans["page"] = (
            [],
            [
                {
                    "header": guard,
                    "line": 0,
                    "inside": set(),
                    "skips": sorted({n for n, _ in scans["web"][0]}),
                }
                for guard in page_guards
            ],
        )
        counts["page (guards over the whole web path)"] = len(page_guards)
    elif not page_draw_found:
        problems.append(
            f"FRAME PAGE {WEB_PAGE_FRAME}: could not locate "
            f"`{WEB_PAGE_DRAW_CALL}` in `{WEB_PAGE_FRAME_FN}` - the gate "
            f"cannot tell whether the page's tick and draw share a guard, "
            f"and a detector that finds nothing reports every host clean."
        )

    # --- half 1: no arm silently drops a fall-through step ----------------
    used_arm_waivers: set[int] = set()
    for host, (_kernels, arms) in scans.items():
        for arm in arms:
            matched = [
                (n, w)
                for n, w in enumerate(arm_waivers)
                if w.get("host") == host and str(w.get("arm", "")) in arm["header"]
            ]
            if len(matched) > 1:
                problems.append(
                    f"FRAME ARM {host} `{arm['header']}`: {len(matched)} waivers "
                    f"match this arm - make each `arm` key name one arm only."
                )
                continue
            waived: set[str] = set()
            if matched:
                idx, entry = matched[0]
                used_arm_waivers.add(idx)
                if not str(entry.get("reason", "")).strip():
                    problems.append(
                        f"FRAME ARM {host} `{arm['header']}`: needs a "
                        f"non-empty `reason`."
                    )
                waived = set(entry.get("skips", []))
                stale = sorted(waived - set(arm["skips"]))
                if stale:
                    problems.append(
                        f"STALE FRAME-ARM WAIVER {host} `{entry.get('arm')}`: "
                        f"{', '.join(stale)} - not skipped by this arm any "
                        f"more (the arm runs it, or it left the frame path). "
                        f"Drop it from `skips`."
                    )
            missing = [k for k in arm["skips"] if k not in waived]
            if missing:
                problems.append(
                    f"FRAME ARM {host} `{arm['header']}` (line {arm['line']}) "
                    f"skips {len(missing)} fall-through kernel(s) the frame "
                    f"still draws after: {', '.join(missing)}. Either call "
                    f"them on this arm, or list them in a `[[frame_arm]]` "
                    f"waiver with a reason."
                )
            else:
                notes.append(
                    f"{host} arm `{arm['header']}`: "
                    f"{len(arm['skips'])} kernel(s) skipped, all disclosed"
                )
    for n, entry in enumerate(arm_waivers):
        if n not in used_arm_waivers:
            problems.append(
                f"STALE FRAME-ARM WAIVER {entry.get('host')} "
                f"`{entry.get('arm')}`: matches no arm of that host's frame "
                f"path (renamed, merged or deleted?). Drop the waiver."
            )

    # --- half 2: neither host's per-frame list has an unpaired member -----
    native = {n for n, _ in scans["native"][0]}
    web = {n for n, _ in scans["web"][0]}
    aliases: dict[str, str] = {}
    solo: dict[tuple[str, str], dict] = {}
    for entry in kernel_waivers:
        if entry.get("host_only"):
            solo[(str(entry.get("host")), str(entry.get("kernel")))] = entry
        else:
            aliases[str(entry.get("native"))] = str(entry.get("web"))
    for host, mine, theirs in (("native", native, web), ("web", web, native)):
        for name in sorted(mine):
            if name in theirs:
                continue
            if host == "native":
                twin = aliases.get(name)
            else:
                twin = next((n for n, w in aliases.items() if w == name), None)
            if twin is not None:
                if twin in theirs:
                    continue
                problems.append(
                    f"STALE FRAME-KERNEL ALIAS `{name}` <-> `{twin}`: the "
                    f"other host's frame path no longer calls its half."
                )
                continue
            entry = solo.get((host, name))
            if entry is None:
                problems.append(
                    f"FRAME KERNEL {host}-only: `{name}` is ticked every frame "
                    f"by the {host} host and by no arm of the other host's "
                    f"frame path. Tick it there too, or declare it with a "
                    f"`[[frame_kernel]]` row (`host_only`, or an alias for a "
                    f"differently-named twin)."
                )
            elif not str(entry.get("reason", "")).strip():
                problems.append(
                    f"FRAME KERNEL {host}-only `{name}`: needs a non-empty "
                    f"`reason`."
                )
            else:
                notes.append(f"{host}-only kernel: {name} - {entry['reason']}")
    for (host, name), entry in solo.items():
        mine = native if host == "native" else web
        theirs = web if host == "native" else native
        if name not in mine:
            problems.append(
                f"STALE FRAME-KERNEL WAIVER {host} `{name}`: not on that "
                f"host's frame path any more. Drop the waiver."
            )
        elif name in theirs:
            problems.append(
                f"STALE FRAME-KERNEL WAIVER {host} `{name}`: both hosts tick "
                f"it now - the gap is closed. Drop the waiver."
            )
    for a, b in aliases.items():
        if a not in native or b not in web:
            problems.append(
                f"STALE FRAME-KERNEL ALIAS `{a}` <-> `{b}`: one side is no "
                f"longer on its host's frame path. Drop or re-point the row."
            )
    return problems, notes, counts
