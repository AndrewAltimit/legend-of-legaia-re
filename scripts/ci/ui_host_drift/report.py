"""Command line and report: run the built-in controls, every tier, then print.
"""

import argparse
import re
import sys

from ui_host_drift.controls import (
    SELFTEST_BOOT_INSTALL,
    SELFTEST_CALL_FORM,
    SELFTEST_CONSTANTS,
    SELFTEST_CONTENT,
    SELFTEST_DIAG,
    SELFTEST_ENV_READS,
    SELFTEST_OWNERSHIP,
    SELFTEST_PAGE_KEYS,
    SELFTEST_PATH_STRINGS,
    SELFTEST_RENDER,
    SELFTEST_SAVE_IO,
    SELFTEST_SCREENS,
    SELFTEST_SIGNATURES,
    SELFTEST_SIM,
    SELFTEST_TRANSFORMS,
    SELFTEST_VARIANT_USE,
    SELFTEST_WORDS,
)
from ui_host_drift.path_check import check_declared_paths, is_path_string
from ui_host_drift.roots import HOSTS, REPO, WAIVERS
from ui_host_drift.rows.constant_pairs import CONSTANT_PAIRS
from ui_host_drift.rows.diag_gates import DIAG_GATES
from ui_host_drift.rows.enum_coverage import ENUM_COVERAGE
from ui_host_drift.rows.owned_types import OWNED_TYPES
from ui_host_drift.rows.render_kernels import RENDER_KERNEL_RULES
from ui_host_drift.rows.sim_pairs import SIM_PAIRS
from ui_host_drift.rust_text import signature_end, word_set
from ui_host_drift.selftest import (
    _selftest_content_case,
    _selftest_page_key_case,
    _selftest_render_case,
    _selftest_sim_case,
    run_selftest,
)
from ui_host_drift.tiers.boot_installs import boot_install_calls, check_boot_installs
from ui_host_drift.tiers.constant_pairs import check_constant_pairs, normalise_value
from ui_host_drift.tiers.diag_gates import check_diag_gates, initialiser_is_off
from ui_host_drift.tiers.env_reads import check_env_reads, env_read_violations
from ui_host_drift.tiers.entry_symmetry import check_entry_symmetry
from ui_host_drift.tiers.enum_coverage import check_enum_coverage
from ui_host_drift.tiers.frame_content import check_frame_content
from ui_host_drift.tiers.frame_paths import check_frame_paths
from ui_host_drift.tiers.owned_types import (
    check_owned_types,
    host_shipped_sources,
    owns_type,
)
from ui_host_drift.tiers.page_keys import check_page_key_tables
from ui_host_drift.tiers.render_kernels import check_render_kernels, render_sources
from ui_host_drift.tiers.save_io import check_save_io_routes, save_io_calls
from ui_host_drift.tiers.screens import (
    collect_builders,
    collect_call_graph,
    collect_fn_names,
    collect_uses,
    is_screen_signature,
    load_waivers,
    seed_transitively,
)
from ui_host_drift.tiers.sim_pairs import check_sim_pairs

# `--help` prints the package docstring: the gate's whole description.
from ui_host_drift import __doc__


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quiet", action="store_true", help="findings only")
    ap.add_argument("--list", action="store_true", help="print the full surface table")
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the screen-vs-transform control suite and exit",
    )
    args = ap.parse_args()

    if args.selftest:
        print("check-ui-host-drift self-test")
        return run_selftest()

    # The surface is only meaningful if the classifier demonstrably separates
    # the two shapes. Run the control every time: a "0 orphans" verdict from a
    # classifier that counts everything, or nothing, is not a measurement.
    for _label, text, name, want in SELFTEST_WORDS:
        if (name in word_set(text)) != want:
            print(
                "ERROR: built-in word-set control failed; the reachability pass "
                "cannot tell a whole-name reference from a substring, so every "
                "host label below is unreliable. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _name, sig in SELFTEST_SCREENS:
        if not is_screen_signature(sig):
            print(
                "ERROR: built-in screen control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(
                "ERROR: built-in transform control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, a, b, want in SELFTEST_CONSTANTS:
        if (normalise_value(a) == normalise_value(b)) != want:
            print(
                "ERROR: built-in constant-pair control failed; a normaliser that "
                "cannot tell a value change from formatting proves nothing about "
                "the pairs below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) != want:
            print(
                "ERROR: built-in signature control failed; the call graph cannot "
                "find function bodies, so every reachability verdict below is "
                "meaningless. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) != want:
            print(
                "ERROR: built-in sim-pair control failed; a comparator that cannot "
                "tell agreement from divergence proves nothing about the pairs "
                "below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, init, want in SELFTEST_DIAG:
        if initialiser_is_off(init) != want:
            print(
                "ERROR: built-in diag-toggle control failed; a detector that "
                "cannot tell `new(false)` from `new(true)` proves nothing "
                "about whether a debug draw is off on the browser. Run "
                "--selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, want in SELFTEST_PAGE_KEYS:
        if _selftest_page_key_case(src) != want:
            print(
                "ERROR: built-in page-key control failed; a detector that cannot "
                "tell a page-side keyboard table from an ordinary object field "
                "proves nothing about the pages below. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, rule, src, want in SELFTEST_RENDER:
        if _selftest_render_case(rule, src) != want:
            print(
                "ERROR: built-in render-kernel control failed; a rule engine "
                "that matches nothing reports every surface clean, which is "
                "exactly the silence this tier exists to break. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, name, want in SELFTEST_OWNERSHIP:
        if owns_type(src, name) != want:
            print(
                "ERROR: built-in ownership control failed; a detector that "
                "cannot tell holding a type from naming it would have passed "
                "the play page's Camera-shaped absence. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, enum_name, variant, want in SELFTEST_VARIANT_USE:
        if bool(re.search(rf"\b{enum_name}::{variant}\b", src)) != want:
            print(
                "ERROR: built-in variant-use control failed; a rule that "
                "accepts an unqualified mention reports every host as "
                "answering every variant. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, kernel, src, api, want in SELFTEST_CONTENT:
        if _selftest_content_case(kernel, src, api) != want:
            print(
                "ERROR: built-in frame-content control failed; a scan that "
                "cannot tell an empty kernel from one that calls the engine "
                "reports every pair identical, which is the silence tier 12 "
                "exists to break. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, src, name, want in SELFTEST_CALL_FORM:
        if bool(re.search(rf"[.:]\s*{name}\s*\(", src)) != want:
            print(
                "ERROR: built-in call-form control failed; a scan that counts "
                "a `pub fn` as its own caller makes the entry-symmetry tier "
                "vacuous. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    for _label, src, want in SELFTEST_ENV_READS:
        if env_read_violations(src) != want:
            print(
                "ERROR: built-in env-read control failed; a scan that cannot "
                "find `env::var_os(\"X\")` cannot say whether an engine crate "
                "reads a switch the browser does not have. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    for _label, src, want in SELFTEST_SAVE_IO:
        if save_io_calls(src, {"write_session_into_card"}) != want:
            print(
                "ERROR: built-in save-route control failed; a scan that cannot "
                "attribute a call to the fn it sits in, or counts a test, cannot "
                "say whether a host saves around the retail screen. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    for _label, src, want in SELFTEST_BOOT_INSTALL:
        if boot_install_calls(src) != want:
            print(
                "ERROR: built-in boot-install control failed; a scan that "
                "misses a split-line call or counts a definition reports the "
                "two boots as matching when they are not. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    for _label, value, want in SELFTEST_PATH_STRINGS:
        if is_path_string(value) != want:
            print(
                "ERROR: built-in path-shape control failed; a walk that cannot "
                "tell a path from a pattern either misses the rows' files or "
                "reports a regex as a missing one. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    builders = collect_builders()
    if not builders:
        print("[ui-drift] no draw builders found - is crates/engine-ui/src present?", file=sys.stderr)
        return 1
    # Seed and propagate over the whole engine-ui fn graph, then classify only
    # the builders. Non-builder nodes exist so a composition that runs through
    # a method is not mistaken for an unused screen.
    fn_names = collect_fn_names()
    uses = collect_uses(fn_names)
    seed_transitively(uses, collect_call_graph(fn_names))
    waivers = load_waivers()

    drift: list[str] = []
    orphan: list[str] = []
    web_ahead: list[str] = []
    both: list[str] = []
    for name in sorted(builders):
        hosts = uses[name]
        if hosts == {"native", "web"}:
            both.append(name)
        elif hosts == {"native"}:
            drift.append(name)
        elif hosts == {"web"}:
            web_ahead.append(name)
        else:
            orphan.append(name)

    if args.list:
        for name in sorted(builders):
            hosts = uses[name] or {"-"}
            mark = "W" if name in waivers else " "
            print(f"{mark} {name:<40} {','.join(sorted(hosts)):<12} {builders[name]}")

    problems: list[str] = []

    # The floor under every tier below: a row that names a file which is
    # not on disk is a rule over nothing, and most tiers would read that
    # as a clean surface.
    path_problems, paths_checked = check_declared_paths()
    problems.extend(path_problems)

    # Unwaived drift / orphans.
    for name in drift:
        if name in waivers:
            if waivers[name].get("kind") != "web_missing":
                problems.append(
                    f"{name}: waiver kind is "
                    f"'{waivers[name].get('kind')}' but the builder is native-only "
                    f"(expected kind = \"web_missing\")"
                )
            continue
        problems.append(
            f"DRIFT {name} ({builders[name]}): wired in the native window, "
            f"not in the browser play page. Wire it into crates/web-play, or "
            f"add a waiver with a reason to {WAIVERS.relative_to(REPO)}."
        )
    for name in orphan:
        if name in waivers:
            if waivers[name].get("kind") != "orphan":
                problems.append(
                    f"{name}: waiver kind is '{waivers[name].get('kind')}' but "
                    f"no host calls the builder (expected kind = \"orphan\")"
                )
            continue
        problems.append(
            f"ORPHAN {name} ({builders[name]}): no host calls this builder. "
            f"Wire it, delete it, or waive it in {WAIVERS.relative_to(REPO)}."
        )

    # Stale waivers - the half that stops this file decaying into fiction.
    for name, entry in sorted(waivers.items()):
        if name not in builders:
            problems.append(
                f"STALE WAIVER {name}: no such engine-ui draw builder "
                f"(renamed or deleted?). Drop the waiver."
            )
            continue
        if name in both:
            problems.append(
                f"STALE WAIVER {name}: now wired on BOTH hosts - the gap is "
                f"closed. Drop the waiver."
            )
        if name in web_ahead:
            problems.append(
                f"STALE WAIVER {name}: web calls it and native does not, so "
                f"this is not a web gap. Drop the waiver."
            )
        if not str(entry.get("reason", "")).strip():
            problems.append(f"WAIVER {name}: needs a non-empty `reason`.")

    # The model half: paired host constants must carry equal values.
    problems.extend(check_constant_pairs())

    # The simulation half: paired injection sites must name the same kernels.
    sim_problems, sim_pending = check_sim_pairs()
    problems.extend(sim_problems)

    # The input half: no page may carry its own keyboard table.
    key_problems, key_files = check_page_key_tables()
    problems.extend(key_problems)

    # The diagnostic half: an additive debug draw must be off by default on
    # BOTH hosts, or one host paints it in normal play.
    diag_problems, diag_additive = check_diag_gates()
    problems.extend(diag_problems)

    # The render half: every surface that assembles a given draw list must run
    # the kernel that makes it correct. The surface is derived, so a new one
    # joins the measurement by existing.
    rk_problems, rk_pending, rk_counts = check_render_kernels()
    problems.extend(rk_problems)

    # The ownership half: a host that reaches an engine type's outputs without
    # holding the type re-implements it, invisibly to every tier above.
    shipped = host_shipped_sources()
    own_problems, own_pending = check_owned_types(shipped)
    problems.extend(own_problems)

    # The enum half: a variant both hosts can enter and one can answer.
    enum_problems, enum_pending, enum_checked = check_enum_coverage(shipped)
    problems.extend(enum_problems)

    # The entry half: a World phase whose only caller is a debug key press.
    entry_problems, entry_pending, entry_methods = check_entry_symmetry()
    problems.extend(entry_problems)

    # The frame half: an arm that short-circuits the frame path without
    # short-circuiting the draw that reads what it skipped.
    frame_problems, frame_notes, frame_counts = check_frame_paths()
    problems.extend(frame_problems)

    # The content half: a pair of frame kernels that reach different engine
    # calls is a simulation the two hosts do not share, however well their
    # names pair.
    content_problems, content_notes, content_pairs = check_frame_content()
    problems.extend(content_problems)

    # The boot half: a World table the native boot installs and the page's
    # disc load does not - invisible to every tier above because each
    # consumer falls back quietly.
    boot_problems, boot_notes, boot_checked = check_boot_installs()
    problems.extend(boot_problems)

    # The save half: a host that writes or reads its save rack from anywhere
    # but the save screen's commit applier has a save path around the retail
    # screen.
    save_io_problems, save_io_checked = check_save_io_routes()
    problems.extend(save_io_problems)

    # The environment half: a switch an engine crate reads from the process
    # environment is one a wasm32 host, which has none, can never throw.
    env_problems, env_scanned = check_env_reads()
    problems.extend(env_problems)

    if not args.quiet:
        print(
            f"[ui-drift] engine-ui draw builders: {len(builders)} "
            f"({len(both)} on both hosts, {len(drift)} native-only, "
            f"{len(web_ahead)} web-only, {len(orphan)} unused)"
        )
        print(
            f"[ui-drift] paths the declared rows name, each checked on disk: "
            f"{paths_checked}"
        )
        print(
            f"[ui-drift] paired host geometry constants: {len(CONSTANT_PAIRS)} "
            f"(value equality only - see the module docstring for what this "
            f"does not prove)"
        )
        print(
            f"[ui-drift] paired simulation injection sites: {len(SIM_PAIRS)} "
            f"({len(sim_pending)} disclosed as blocked)"
        )
        print(
            f"[ui-drift] pad-driving site sources scanned for page-side "
            f"keyboard tables: {key_files}"
        )
        print(
            f"[ui-drift] LEGAIA_DIAG_* gates declared: {len(DIAG_GATES)} "
            f"({diag_additive} additive - i.e. draw something retail does not "
            f"and so need a default-off twin on both hosts)"
        )
        print(
            f"[ui-drift] render kernels checked across "
            f"{len(render_sources())} render-surface sources: "
            f"{len(RENDER_KERNEL_RULES)} rules"
        )
        # Name every row of the kernel x surface matrix. A bare rule count
        # cannot tell "three surfaces assemble this list" from "one does and
        # two were renamed out of the trigger", and the second is how a
        # derived surface quietly shrinks to nothing.
        for kernel, running, blocked_n, exempt_n in rk_counts:
            print(
                f"[ui-drift] render kernel `{kernel}`: {running} surface(s) "
                f"run it, {blocked_n} disclosed as blocked, {exempt_n} exempt"
            )
        for note in rk_pending:
            print(f"[ui-drift] render kernel blocked: {note}")
        print(
            f"[ui-drift] engine types both hosts must own: {len(OWNED_TYPES)} "
            f"({len(own_pending)} disclosed as blocked)"
        )
        for note in own_pending:
            print(f"[ui-drift] unowned type blocked: {note}")
        print(
            f"[ui-drift] shared-enum variant answers checked: {enum_checked} "
            f"across {len(ENUM_COVERAGE)} enum(s) x {len(HOSTS)} hosts "
            f"({len(enum_pending)} waived)"
        )
        for note in enum_pending:
            print(f"[ui-drift] variant waived: {note}")
        print(
            f"[ui-drift] World methods reached from the native hotkey arms: "
            f"{entry_methods} ({len(entry_pending)} reached from nowhere else, "
            f"each disclosed)"
        )
        for note in entry_pending:
            print(f"[ui-drift] hotkey-only: {note}")
        print(
            "[ui-drift] per-frame kernels on each host's frame path: "
            + ", ".join(f"{h} {n}" for h, n in sorted(frame_counts.items()))
        )
        # Name every disclosure, for the reason the native-only builders
        # below are named: a count cannot tell "the same arms as yesterday"
        # from "an arm gained a skip and another lost one".
        for note in frame_notes:
            print(f"[ui-drift] frame path: {note}")
        print(
            f"[ui-drift] paired frame kernels compared by CONTENT: "
            f"{content_pairs} (engine call sets; see the tier-12 note for "
            f"what a name join cannot see)"
        )
        for note in content_notes:
            print(f"[ui-drift] {note}")
        print(
            f"[ui-drift] native boot world installs checked against the page: "
            f"{boot_checked} ({len(boot_notes)} waived)"
        )
        for note in boot_notes:
            print(f"[ui-drift] boot install waived: {note}")
        print(
            f"[ui-drift] save-rack I/O calls pinned to the save screen's commit "
            f"appliers: {save_io_checked}"
        )
        print(
            f"[ui-drift] engine sources scanned for environment reads: "
            f"{env_scanned}"
        )
        if web_ahead:
            print(f"[ui-drift] web-ahead (informational): {', '.join(web_ahead)}")
        # Name every native-only builder, waived or not, for the same reason
        # the orphans below are named. A waived row still prints nothing but
        # its contribution to a count, so "2 native-only" is indistinguishable
        # from "the same 2 as yesterday plus one that lost its web caller and
        # one that gained one" - the arithmetic is stable while the membership
        # is not. Naming is what makes a waiver auditable from the output.
        for name in drift:
            mark = "waived" if name in waivers else "UNWAIVED"
            print(f"[ui-drift] native-only ({mark}): {name}  {builders[name]}")
        # Name every orphan, waived or not. A bare count cannot distinguish
        # "the same six as yesterday" from "a builder's last caller was
        # deleted this morning", which is exactly how window 25's painter
        # chain lost its consumer without a line of output changing.
        for name in orphan:
            mark = "waived" if name in waivers else "UNWAIVED"
            print(f"[ui-drift] orphan ({mark}): {name}  {builders[name]}")
        for note in sim_pending:
            print(f"[ui-drift] sim pair blocked: {note}")

    if problems:
        print(f"\n[ui-drift] {len(problems)} problem(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1

    if not args.quiet:
        print("[ui-drift] ok - every shared screen reaches both hosts or is waived")
    return 0
