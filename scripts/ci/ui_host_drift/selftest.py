"""`--selftest`: run every detector control and report which ones fail.
"""

import re

from ui_host_drift.controls import (
    SELFTEST_CALL_FORM,
    SELFTEST_CONSTANTS,
    SELFTEST_CONTENT,
    SELFTEST_DIAG,
    SELFTEST_ENV_READS,
    SELFTEST_FRAME,
    SELFTEST_OWNERSHIP,
    SELFTEST_PAGE_KEYS,
    SELFTEST_PATH_STRINGS,
    SELFTEST_RENDER,
    SELFTEST_SCREENS,
    SELFTEST_SIGNATURES,
    SELFTEST_SIM,
    SELFTEST_TRANSFORMS,
    SELFTEST_VARIANT_USE,
    SELFTEST_WORDS,
)
from ui_host_drift.path_check import is_path_string
from ui_host_drift.rows.frame_paths import FRAME_PATHS
from ui_host_drift.rows.page_keys import NON_PAD_CODES
from ui_host_drift.rust_text import (
    brace_block,
    signature_end,
    strip_all_comments,
    word_set,
)
from ui_host_drift.tiers.constant_pairs import normalise_value
from ui_host_drift.tiers.diag_gates import initialiser_is_off
from ui_host_drift.tiers.env_reads import env_read_violations
from ui_host_drift.tiers.frame_content import CONTENT_FN_RE, kernel_engine_calls
from ui_host_drift.tiers.frame_paths import frame_path_scan
from ui_host_drift.tiers.owned_types import owns_type
from ui_host_drift.tiers.page_keys import (
    EVENT_CODE_LITERAL_RE,
    EVENT_KEY_RE,
    KEY_SET_LITERAL_RE,
    bindable_dom_codes,
    controller_table_hits,
)
from ui_host_drift.tiers.render_kernels import rule_findings
from ui_host_drift.tiers.screens import is_screen_signature
from ui_host_drift.tiers.sim_pairs import splice_native_steps


def _selftest_page_key_case(src: str) -> bool:
    """Run the tier-5 detectors over one synthetic source; True when flagged."""
    if EVENT_KEY_RE.search(src):
        return True
    if any(m.group(1) not in NON_PAD_CODES for m in EVENT_CODE_LITERAL_RE.finditer(src)):
        return True
    if controller_table_hits(src, False):
        return True
    codes = bindable_dom_codes()
    return any(m.group(1) in codes for m in KEY_SET_LITERAL_RE.finditer(src))


def _selftest_sim_case(mode: str, a: str, b: str, extra: object) -> bool:
    """Run one synthetic sim-pair comparison; True when it diverges."""
    hosts = ["native", "web"]
    bodies = {"native": a, "web": b}
    diffs: list[str] = []
    if mode in ("symbols_all", "symbols_same"):
        for sym in extra:  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                diffs.append(sym)
            elif mode == "symbols_same" and seen["native"] != seen["web"]:
                diffs.append(sym)
    else:
        pat = re.compile(str(extra))
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        if found["native"] != found["web"]:
            diffs.append("pattern")
    return bool(diffs)


def run_selftest() -> int:
    failures = 0
    # The native step splice: a step's body replaces its call (arguments
    # included), recursively, and a call to a non-step fn stays a call.
    spliced = splice_native_steps(
        "self.a(); let x = self.b(f(1), 2); self.k();",
        {"a": " self.b(); ", "b": " self.c(); y "},
    )
    if spliced == "  self.c(); y ; ; let x =  self.c(); y ; self.k();":
        print("  ok    native step splice: nested steps inline, kernels stay calls")
    else:
        print(f"  FAIL  native step splice: got {spliced!r}")
        failures += 1
    for label, text, name, want in SELFTEST_WORDS:
        got = name in word_set(text)
        ref = re.search(rf"\b{re.escape(name)}\b", text) is not None
        if got == want and ref == want:
            print(f"  ok    word set: {label}")
        else:
            print(
                f"  FAIL  word set: {label} - word_set={got}, "
                f"regex={ref}, expected {want}"
            )
            failures += 1
    for name, sig in SELFTEST_SCREENS:
        if is_screen_signature(sig):
            print(f"  ok    {name}: counted as a screen")
        else:
            print(f"  FAIL  {name}: dropped from the surface (expected a screen)")
            failures += 1
    for name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(f"  FAIL  {name}: counted as a screen (expected a transform)")
            failures += 1
        else:
            print(f"  ok    {name}: excluded as a draw-list transform")
    for label, a, b, want in SELFTEST_CONSTANTS:
        got = normalise_value(a) == normalise_value(b)
        if got == want:
            print(f"  ok    constants: {label}")
        else:
            verdict = "matched" if got else "differed"
            print(f"  FAIL  constants: {label} - normaliser {verdict}")
            failures += 1
    for label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) == want:
            print(f"  ok    signature: {label}")
        else:
            print(f"  FAIL  signature: {label}")
            failures += 1
    for label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) == want:
            print(f"  ok    sim pair: {label}")
        else:
            print(f"  FAIL  sim pair: {label}")
            failures += 1
    for label, src, want in SELFTEST_PAGE_KEYS:
        if _selftest_page_key_case(src) == want:
            print(f"  ok    page keys: {label}")
        else:
            verdict = "flagged" if not want else "passed"
            print(f"  FAIL  page keys: {label} - detector {verdict}")
            failures += 1
    for label, init, want in SELFTEST_DIAG:
        if initialiser_is_off(init) == want:
            print(f"  ok    diag toggle: {label}")
        else:
            print(f"  FAIL  diag toggle: {label}")
            failures += 1
    for label, src, want in SELFTEST_ENV_READS:
        if env_read_violations(src) == want:
            print(f"  ok    env read: {label}")
        else:
            print(f"  FAIL  env read: {label}")
            failures += 1
    for label, rule, src, want in SELFTEST_RENDER:
        if _selftest_render_case(rule, src) == want:
            print(f"  ok    render kernel: {label}")
        else:
            verdict = "stayed silent" if want else "fired"
            print(f"  FAIL  render kernel: {label} - detector {verdict}")
            failures += 1
    for label, src, name, want in SELFTEST_OWNERSHIP:
        if owns_type(src, name) == want:
            print(f"  ok    ownership: {label}")
        else:
            verdict = "claimed ownership" if not want else "saw none"
            print(f"  FAIL  ownership: {label} - detector {verdict}")
            failures += 1
    for label, src, enum_name, variant, want in SELFTEST_VARIANT_USE:
        if bool(re.search(rf"\b{enum_name}::{variant}\b", src)) == want:
            print(f"  ok    variant use: {label}")
        else:
            failures += 1
            print(f"  FAIL  variant use: {label}")
    for label, src, name, want in SELFTEST_CALL_FORM:
        if bool(re.search(rf"[.:]\s*{name}\s*\(", src)) == want:
            print(f"  ok    call form: {label}")
        else:
            failures += 1
            print(f"  FAIL  call form: {label}")
    for label, src, anchor, word, arm, want in SELFTEST_FRAME:
        got = _selftest_frame_case(src, anchor, word, arm)
        if got == want:
            print(f"  ok    frame path: {label}")
        else:
            failures += 1
            print(f"  FAIL  frame path: {label} - skips {got}, expected {want}")
    for label, kernel, src, api, want in SELFTEST_CONTENT:
        got = _selftest_content_case(kernel, src, api)
        if got == want:
            print(f"  ok    frame content: {label}")
        else:
            failures += 1
            print(f"  FAIL  frame content: {label} - reached {got}, expected {want}")
    for label, value, want in SELFTEST_PATH_STRINGS:
        if is_path_string(value) == want:
            print(f"  ok    path shape: {label}")
        else:
            failures += 1
            print(
                f"  FAIL  path shape: {label} - read as "
                f"{'a path' if not want else 'not a path'}"
            )
    total = (
        len(SELFTEST_WORDS)
        + len(SELFTEST_SCREENS)
        + len(SELFTEST_TRANSFORMS)
        + len(SELFTEST_CONSTANTS)
        + len(SELFTEST_SIGNATURES)
        + len(SELFTEST_SIM)
        + len(SELFTEST_PAGE_KEYS)
        + len(SELFTEST_DIAG)
        + len(SELFTEST_ENV_READS)
        + len(SELFTEST_RENDER)
        + len(SELFTEST_OWNERSHIP)
        + len(SELFTEST_VARIANT_USE)
        + len(SELFTEST_CALL_FORM)
        + len(SELFTEST_FRAME)
        + len(SELFTEST_CONTENT)
        + len(SELFTEST_PATH_STRINGS)
    )
    if failures:
        print(
            f"\nself-test: {failures} of {total} case(s) failed - the surface this "
            f"gate measures is not the set of screens, so its verdict means nothing"
        )
        return 2
    print(f"\nself-test: all {total} cases pass")
    return 0


def _selftest_render_case(rule: dict, src: str) -> bool:
    return bool(rule_findings(rule, strip_all_comments(src)))


def _selftest_frame_case(
    src: str, anchor: str, exit_word: str, arm_sub: str
) -> list[str]:
    """Run the arm scanner over one synthetic frame path."""
    FRAME_PATHS["__selftest__"] = {
        "path": None,
        "anchor": anchor,
        "exit": exit_word,
        "_source": src,
    }
    try:
        _kernels, arms = frame_path_scan("__selftest__")
    finally:
        FRAME_PATHS.pop("__selftest__", None)
    if not arms:
        return ["<arm not found>"]
    for arm in arms:
        if arm_sub and arm_sub in arm["header"]:
            return arm["skips"]
    return ["<arm not found>"]


def _selftest_content_case(kernel: str, src: str, api: set[str]) -> set[str]:
    bodies: dict[str, list[str]] = {}
    for m in CONTENT_FN_RE.finditer(src):
        brace = signature_end(src, m.start())
        if brace < 0:
            continue
        start, end = brace_block(src, brace)
        bodies.setdefault(m.group(1), []).append(src[start:end])
    return kernel_engine_calls(kernel, bodies, api)[0]
