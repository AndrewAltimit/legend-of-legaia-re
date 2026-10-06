#!/usr/bin/env python3
"""Refresh the committed progress metrics the site landing page renders.

The site is built by `site/_gen.py` on a machine that has **no disc data** -
`extracted/` and the Ghidra dump corpus are both gitignored. So the landing
page cannot compute its own numbers at deploy time. This script is the local
refresh step: run it on a machine that has the disc, commit the resulting JSON,
and the deployment pipeline just renders what is committed.

Sources, and what each denominator actually is - the labels matter more than the
numbers, because the two families are not comparable:

  DISC-DENOMINATED (`scripts/ci/disc-coverage.py`)
    Measured against the game's own bytes. These can fall as well as rise and
    are the only figures that can say how much of the game is left.

  CORPUS-DENOMINATED (`scripts/ci/port-catalog.py`)
    Measured against the set of addresses this project has identified. Useful
    for steering work; structurally unable to see a subsystem nobody has cited.
    Never present one of these as "percent of the game".

    The wiring track's denominator is narrower still: it excludes the ports
    the engine replaces by construction (`REPLACED-BY:`), because a routine
    no host can ever call is not a wiring gap in either direction.

Usage:
    python3 scripts/ci/update-progress-metrics.py          # refresh + write
    python3 scripts/ci/update-progress-metrics.py --print  # show, don't write
    python3 scripts/ci/update-progress-metrics.py \
        --funcs /path/to/checkout/ghidra/scripts/funcs \
        --extracted /path/to/checkout/extracted

Both corpora are gitignored, so outside the checkout that holds them this
script has nothing to read and says SKIPPED. `--funcs` / `--extracted` point it
at another checkout's copies, which is the supported way to compute the figures
from somewhere else - copying either corpus in would stage Sony bytes.

One warning about doing that, because the failure is silent: the disc-coverage
half pairs the live corpus with **this tree's** committed
`dump-extent-attribution.csv`. Pointing at a corpus while the tree's CSV is a
different vintage publishes numbers that describe neither. Refresh from a tree
whose CSV matches the corpus, and prefer the checkout that holds both.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "scripts", "ci", "progress-metrics.json")
DISC_COVERAGE = os.path.join(REPO, "scripts", "ci", "disc-coverage.py")
PORT_CATALOG = os.path.join(REPO, "scripts", "ci", "port-catalog.py")


def load_disc_coverage(funcs=None, extracted=None):
    spec = importlib.util.spec_from_file_location("disc_coverage", DISC_COVERAGE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    funcs = funcs or mod.DEFAULT_FUNCS
    extracted = extracted or mod.DEFAULT_EXTRACTED
    extents, _unparsed = mod.read_dump_extents(funcs)
    if not extents:
        return None, None
    scus = mod.scus_report(extracted, extents)
    data = mod.data_report(extracted)
    return scus, data


# ---------------------------------------------------------------------------
# Per-image port status (`image-port-status.json`)
#
# The four tiles above are whole-project figures and three of them sit at
# 100%, so they no longer move. The question still open is per runtime code
# image: of the functions the disc puts in the field overlay, the battle
# overlay, each minigame, how many carry a port? This builds that table.
#
#   denominator  the function entries the dump corpus places in the image,
#                with `disc-coverage.py`'s own placement rule (its `floor`:
#                extents the byte attribution names for this image, plus the
#                ones no other image's span reaches). Inherited tails are cut
#                there too, so a sibling's code never inflates a row.
#   numerator    entries carrying a `// PORT:` tag, then the ignore-list rows
#                split by what the row claims (below), then `open`.
#   companion    the image's code bytes inside a placed dump. A function list
#                cannot see a routine nobody dumped, so a row's "100%" is only
#                as good as this figure, and the table prints both.
# ---------------------------------------------------------------------------

IMAGE_STATUS_OUT = os.path.join(REPO, "scripts", "ci", "image-port-status.json")
STATIC_OVERLAYS = os.path.join(REPO, "crates", "asset", "data", "static-overlays.toml")
IGNORE_TOML = os.path.join(REPO, "scripts", "ci", "port-catalog-ignore.toml")
# Per-image verdicts for VA-aliased addresses (see `load_scoped_verdicts`).
SCOPED_TOML = os.path.join(REPO, "scripts", "ci", "image-scoped-verdicts.toml")
ATTRIBUTION_CSV = os.path.join(
    REPO, "scripts", "ghidra-analysis", "dump-extent-attribution.csv")

# Ignore-list sections whose claim is "no routine begins at this VA" (a label,
# an interior fragment, a phantom print). A dump at such an address is not a
# function, so it leaves the image's denominator rather than padding it.
NOT_A_FUNCTION = {
    "field_vm_labels", "overlay_epilogue_labels", "ghidra_phantoms",
    "worklist_interior", "worklist_shared_tail", "worklist_duplicate",
    "worklist_phantom", "worklist_data", "worklist_uncertain",
    "worklist_misbased_print", "worklist_foreign_build_scus",
}
# Sections that excuse a bare VA because several images hold DIFFERENT code
# there. That is a statement about the address, not about any one image's
# routine, so per image it excuses nothing: the routine stays open unless a
# tag ports it **in that image** - a stem `overlay_<label>_<addr>` whose label
# resolves to the image (`stem_names_image`), never a bare `FUN_<addr>`, which
# cannot say which of the images it implements - or `image-scoped-verdicts.toml`
# classifies that image's routine.
NOT_AN_EXCUSE = {"va_aliased_overlay_local", "worklist_va_aliased"}
# Real routines with no retail behaviour to carry: never reached on the disc,
# dev-gated, or empty bodies.
NO_BEHAVIOUR = {
    "unreferenced", "unreferenced_slot_a_orphans",
    "unreferenced_transport_and_runtime", "dev_gated_scus", "dev_tool_overlays",
    "jp_options_status_overlay", "noop_stubs", "noop_frame", "identity_thunks",
}
# Every other section is a scope claim: the routine is real and the port does
# its job natively (PsyQ libraries, BIOS, libgte, the GPU packet pipeline, CD
# transport, plumbing subsumed by a ported caller).

# Game role per image label. The JSON carries the role so the site renders
# names a player recognises; an unlisted label falls back to itself.
#   kind  game     - shipped game logic
#         dev      - developer harness the retail game never enters
#         unloaded - an image this build never loads at all
IMAGE_ROLES = {
    "SCUS_942.54": ("Main executable", "Engine core, script VMs, PsyQ libraries", "game"),
    "field": ("Field / event VM", "Towns, dungeons, dialogue, the world map", "game"),
    "battle_action": ("Battle", "Battle state machine, damage, effects", "game"),
    "menu": ("Menu, shop + save", "Pause menu, equipment, shops, save screen", "game"),
    "summon_render": ("Field render library", "Slot-B ground + decoration renderer for field scenes", "game"),
    "world_map_render": ("World map renderer", "Kingdom-map terrain companion", "game"),
    "cutscene_str": ("FMV player", "STR / MDEC movie playback", "game"),
    "field_battle_intro": ("Battle intro", "Field-to-battle transition", "game"),
    "battle_tutorial": ("Battle tutorial", "Tetsu's sparring prompts", "game"),
    "battle_slot_b_0968": ("Boss stage A", "Scripted boss-stage module", "game"),
    "battle_slot_b_0969": ("Boss stage B", "Scripted boss-stage module", "game"),
    "fishing": ("Fishing", "Minigame", "game"),
    "slot_machine": ("Slot machine", "Casino minigame", "game"),
    "baka_fighter": ("Baka Fighter", "Duel minigame", "game"),
    "dance": ("Dance", "Noa's rhythm minigame", "game"),
    "arena_init": ("Muscle Dome", "Arena roster + contest hub", "game"),
    "field_back_read": ("Field back-read", "Staged background loader", "game"),
    "boot_init_pak": ("Boot", "Publisher logos + init.pak", "game"),
    "gameover": ("Game over", "Unreachable dev harness", "dev"),
    "debug_menu": ("Debug menu", "Developer mode select", "dev"),
    "other2_dev": ("Dev module OTHER2", "Primitive / clip test harness", "dev"),
    "other3_dev": ("Dev module OTHER3", "Selection / depth test harness", "dev"),
    "monster_test": ("World-map top view", "Debug camera image", "dev"),
    "jp_options_status": ("JP options + status", "Foreign-build image, never loaded", "unloaded"),
}
# PROT 0903..0966: the Seru magic, summon and enemy-cast modules all load at
# one slot-B base. Sixty-four images of a few functions each read better as
# one band row (with how many of them are complete) than as a wall of rows.
CAST_BAND = ("cast_band", "Seru magic + cast modules",
             "One module per Seru spell or enemy cast", "game")


def _sha(*chunks):
    import hashlib
    h = hashlib.sha256()
    for c in chunks:
        h.update(c if isinstance(c, bytes) else c.encode())
    return h.hexdigest()[:16]


def image_status_inputs(ports, ignore):
    """Digest of every COMMITTED input the per-image counts depend on.

    The dump corpus and `extracted/` are gitignored, so they cannot be
    digested where the freshness check runs. Everything else can: the ported
    address set (from `crates/`), the ignore list, the byte attribution CSV and
    the overlay map. `check-progress-metrics-freshness.py` recomputes this from
    the tree and warns when it no longer matches the committed JSON.
    """
    def file_bytes(p):
        try:
            return open(p, "rb").read()
        except OSError:
            return b""
    stems = collect_port_stems()
    return {
        "ports": _sha(",".join(sorted(ports))),
        "stems": _sha(",".join("%s=%s" % (a, "+".join(sorted(stems[a]))) for a in sorted(stems))),
        "scoped": _sha(file_bytes(SCOPED_TOML)),
        "ignore": _sha(",".join("%s=%s" % (a, ignore[a][0]) for a in sorted(ignore))),
        "attribution": _sha(file_bytes(ATTRIBUTION_CSV)),
        "overlay_map": _sha(file_bytes(STATIC_OVERLAYS)),
    }


def stem_names_image(stem_label, image_label, prot_index):
    """Does a dump-stem label (`0897`, `0897_xxx_dat`, `cast_steal_0941`,
    `fishing`) name this image? A stem carrying a four-digit PROT token names
    that entry; one without names the image whose label it equals. Capture
    labels that are no image (`cutscene_dialogue`, `muscle_dome`) name none."""
    digits = [t for t in stem_label.split("_") if len(t) == 4 and t.isdigit()]
    if digits:
        return prot_index is not None and int(digits[0]) == prot_index
    return stem_label == image_label.lower()


# A `PROT NNNN` token in a tag's own text ("FUN_801F69D8 (PROT 0905; ...)"),
# the form the slot-B cast-module ports use to say which of the sixty-four
# images sharing their VA they implement.
PROT_TOKEN_RE = re.compile(r"\bPROT\s+(\d{3,4})\b")


def collect_port_stems():
    """`{addr: {stem label}}` over every `// PORT:` tag in `crates/`.

    Besides each `overlay_<label>_<addr>` stem, a `PROT NNNN` token anywhere in
    the tag's text names an image for every address in that tag, recorded as
    the pseudo-stem `prot_NNNN` (which `stem_names_image` resolves through its
    four-digit token like any stem). Stems are consulted only where an address
    is shared between images, so prose naming an unrelated entry credits
    nothing."""
    sys.path.insert(0, os.path.dirname(PORT_CATALOG))
    import port_tag_reader
    out = {}
    for root, _dirs, files in os.walk(os.path.join(REPO, "crates")):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            try:
                text = open(os.path.join(root, fn), errors="ignore").read()
            except OSError:
                continue
            for _ln, kind, tail in port_tag_reader.iter_markers(text):
                if kind != "PORT":
                    continue
                for a, labels in port_tag_reader.stem_labels(tail).items():
                    out.setdefault(a, set()).update(labels)
                prots = {"prot_%04d" % int(n) for n in PROT_TOKEN_RE.findall(tail)}
                if prots:
                    for a in port_tag_reader.addresses(tail):
                        out.setdefault(a, set()).update(prots)
    return out


def load_scoped_verdicts():
    """`{(image label, addr): (category, reason)}` from `image-scoped-verdicts.toml`.

    The ignore list keys a verdict by bare VA, so it cannot say "in the field
    overlay this VA is an interior fragment, in fishing it is a real routine".
    This file can: one table per image label, rows `addr = [category, reason]`
    with `category` drawn from the ignore list's sections. A row applies to its
    image only and overrides that image's bare-VA category.
    """
    import tomllib
    try:
        with open(SCOPED_TOML, "rb") as fh:
            data = tomllib.load(fh)
    except OSError:
        return {}
    out = {}
    for label, body in data.items():
        if not isinstance(body, dict):
            continue
        for addr, val in body.items():
            cat, reason = (val[0], val[1]) if isinstance(val, list) else (str(val), "")
            out[(label, addr.lower())] = (cat, reason)
    return out


def load_port_catalog(funcs=None):
    spec = importlib.util.spec_from_file_location("port_catalog", PORT_CATALOG)
    mod = importlib.util.module_from_spec(spec)
    sys.path.insert(0, os.path.dirname(PORT_CATALOG))
    spec.loader.exec_module(mod)
    if funcs:
        from pathlib import Path
        mod.FUNCS_DIR = Path(funcs).expanduser()
    return mod


def replaced_addresses(pc):
    """Ported addresses no host is owed for (`REPLACED-BY:`), as the catalog's
    summary counts them: not live, and carrying the tag."""
    srcs = pc.load_rust_sources()
    fns, edges = pc.build_rust_graph(srcs)
    roots = pc.collect_roots(srcs)
    reach = pc.reachable_fns(fns, edges, roots)
    live = pc.compute_live(pc.collect_port_anchors(srcs), srcs, fns, reach)
    return {a for a, v in live.items() if not v["live"] and v["replaced_tag"]}


# `--list-open` collects `(image, addr, category)` for every open entry here.
OPEN_SINK = None


def shared_entries(rows):
    """Addresses the floor places in more than one image.

    The attribution places one VA in two images only when each image's own
    bytes start a routine there (`divergent`, or `unique` extents of different
    lengths) - different code linked at the same address. A bare
    `FUN_<addr>` tag cannot say which of them it implements, exactly as at an
    ignore-listed aliased VA, so it is treated the same way. Without this, one
    bare tag on a slot-B entry point credited every image that starts a
    routine there (twenty-three of them at `0x801F69D8`)."""
    seen, shared = set(), set()
    for r in rows:
        for a in {"%08x" % va for va, _b in r["floor_extents"]}:
            (shared if a in seen else seen).add(a)
    return shared


def _image_row(row, ports, ignore, replaced, stems=None, scoped=None,
               prot_index=None, shared=frozenset()):
    """Counts for one covered image (a `cover_image` result)."""
    stems = stems or {}
    scoped = scoped or {}
    label = row["name"]
    entries = sorted({a for a, _b in row["floor_extents"]})
    unplaced = sorted({a for a, _b in row["unattributed"]} - set(entries))
    c = {"functions": 0, "ported": 0, "replaced": 0, "native": 0,
         "no_behaviour": 0, "open": 0, "not_a_function": 0,
         "unplaced": len(unplaced), "unplaced_open": 0}
    for va in entries:
        a = "%08x" % va
        cat = ignore.get(a, (None,))[0]
        aliased = cat in NOT_AN_EXCUSE or a in shared
        # At an aliased VA - ignore-listed as one, or placed in several images
        # by the floor - a bare tag cannot say which image it ports; only a
        # stem (or a `PROT NNNN` token) naming this image credits it.
        here = any(stem_names_image(s, label, prot_index) for s in stems.get(a, ()))
        if (a in ports and not aliased) or here:
            c["functions"] += 1
            c["ported"] += 1
            c["replaced"] += a in replaced
            continue
        if a in shared and cat is not None:
            # The ignore list keys a verdict by bare VA, and its reason
            # describes one image's routine; where the floor places a
            # different routine in each of several images it cannot speak
            # for the others. Only a per-image verdict classifies them.
            cat = "shared_va(%s)" % cat
        cat = scoped.get((label, a), (cat,))[0]
        if cat in NOT_A_FUNCTION:
            c["not_a_function"] += 1
            continue
        c["functions"] += 1
        if cat is None or cat in NOT_AN_EXCUSE or cat.startswith("shared_va("):
            c["open"] += 1
            if OPEN_SINK is not None:
                OPEN_SINK.append((label, a, cat))
        elif cat in NO_BEHAVIOUR:
            c["no_behaviour"] += 1
        else:
            c["native"] += 1
    for va in unplaced:
        a = "%08x" % va
        if a not in ports and a not in ignore:
            c["unplaced_open"] += 1
    c["code_bytes"] = row["code_denominator"]
    c["identified_bytes"] = row["covered_attributed"]
    c["undumped_code_bytes"] = row["code_gap"]
    return c


def _finish(rec):
    """Derived figures + the `fully_ported` verdict, shared by rows and bands."""
    fn = rec["functions"]
    done = rec["ported"] + rec["native"] + rec["no_behaviour"]
    rec["pct_accounted"] = round(100.0 * done / fn, 1) if fn else 0.0
    cb = rec["code_bytes"]
    rec["pct_identified"] = round(100.0 * rec["identified_bytes"] / cb, 1) if cb else 0.0
    # Complete means: every function the disc places here is ported or
    # excused, AND the function list can be trusted to be the whole image -
    # no un-dumped code, and under 1% of the code bytes resting only on a
    # dump the attribution could not place. The 1% admits the 4-to-36-byte
    # Ghidra fragments every overlay carries; a real unplaced routine is
    # larger than that budget on every image measured.
    rec["fully_ported"] = bool(
        fn and rec["open"] == 0 and rec["undumped_code_bytes"] == 0
        and rec["identified_bytes"] >= 0.99 * cb)
    return rec


def build_image_status(funcs=None, extracted=None, with_replaced=True):
    dc_spec = importlib.util.spec_from_file_location("disc_coverage", DISC_COVERAGE)
    dc = importlib.util.module_from_spec(dc_spec)
    dc_spec.loader.exec_module(dc)
    funcs = funcs or dc.DEFAULT_FUNCS
    extracted = extracted or dc.DEFAULT_EXTRACTED
    extents, _ = dc.read_dump_extents(funcs)
    if not extents:
        return None
    attrib = dc.read_attribution()
    scus = dc.scus_report(extracted, extents)
    overlays, _totals, _unamb = dc.overlay_reports(extracted, extents, attrib)
    if scus is None or not overlays:
        return None

    pc = load_port_catalog(funcs)
    ports = set(pc.collect_ports())
    ignore = pc.load_ignore()
    replaced = replaced_addresses(pc) if with_replaced else set()
    stems = collect_port_stems()
    scoped = load_scoped_verdicts()

    prot = {}
    import tomllib
    with open(STATIC_OVERLAYS, "rb") as fh:
        for r in tomllib.load(fh).get("overlays", []):
            prot[r.get("label")] = r.get("prot_index")

    rows, band = [], None
    shared = shared_entries([scus] + overlays)
    for r in [scus] + overlays:
        label = r["name"]
        p = prot.get(label)
        counts = _image_row(r, ports, ignore, replaced, stems, scoped, p, shared)
        if p is not None and 903 <= p <= 966:
            if band is None:
                key, name, desc, kind = CAST_BAND
                band = {"key": key, "name": name, "desc": desc, "kind": kind,
                        "prot": "0903-0966", "images": 0, "images_complete": 0,
                        "images_no_code": 0}
                for k in counts:
                    band[k] = 0
            if not counts["code_bytes"]:
                # An image whose own content is all inherited tail (PROT 0926
                # is one sector: a bare `jr ra` stub and a pointer table) has
                # no code to port. Counted, so the band's total is still 64.
                band["images_no_code"] += 1
            band["images"] += 1
            for k, v in counts.items():
                band[k] += v
            band["images_complete"] += _finish(dict(counts))["fully_ported"]
            continue
        name, desc, kind = IMAGE_ROLES.get(label, (label, "", "game"))
        rows.append(_finish({
            "key": label, "name": name, "desc": desc, "kind": kind,
            "prot": None if label == "SCUS_942.54" else "%04d" % p,
            **counts}))
    if band:
        # The band is complete only when every module with code is.
        rows.append(_finish(band))
        band["fully_ported"] = band["fully_ported"] and (
            band["images_complete"] + band["images_no_code"] == band["images"])

    order = {"game": 0, "dev": 1, "unloaded": 2}
    rows.sort(key=lambda r: (order.get(r["kind"], 9), r["key"] != "SCUS_942.54",
                             -r["code_bytes"]))
    return {
        "_comment": "Committed build input for site/_gen.py: per runtime code "
                    "image, how many of the functions the disc places there "
                    "carry a port. Counts and labels only. Refresh with "
                    "scripts/ci/update-progress-metrics.py on a machine with "
                    "the disc and the dump corpus; "
                    "scripts/ci/check-progress-metrics-freshness.py warns when "
                    "`inputs` no longer matches the tree.",
        "inputs": image_status_inputs(ports, ignore),
        "replaced_measured": with_replaced,
        "images": rows,
    }


def run_port_catalog(funcs=None):
    """Parse the catalog's own summary block rather than re-deriving it."""
    cmd = [sys.executable, PORT_CATALOG, "--live-audit"]
    if funcs:
        cmd += ["--funcs", funcs]
    try:
        proc = subprocess.run(
            cmd, cwd=REPO, capture_output=True, text=True, timeout=3600)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = proc.stdout + proc.stderr

    def grab(pattern):
        m = re.search(pattern, text)
        return int(m.group(1)) if m else None

    return {
        "ported": grab(r"ported \(// PORT: tag\)\s*:\s*(\d+)"),
        "worklist": grab(r"remaining port worklist\s*:\s*(\d+)"),
        "live": grab(r"ported \+ live.*?:\s*(\d+)"),
        "inert": grab(r"ported, NOT live \(inert\)\s*:\s*(\d+)"),
        # Ports whose job the engine does by construction (CD DMA, card BU
        # I/O, MDEC channel sync, GPU packet queues, retail node pools). They
        # can never have a host, so counting them against the wiring track
        # states a gap that will never close. See
        # docs/tooling/port-catalog.md#replaced-by.
        "replaced": grab(r"of which infra-replaced.*?:\s*(\d+)"),
        "documented_gap": grab(r"ported but NOT documented \(provenance gap\)\s*:\s*(\d+)"),
        "dump_worklist": grab(r"cited but NOT dumped\s+\(dump worklist\)\s*:\s*(\d+)"),
    }


def build(scus, data, cat):
    tracks = []

    if scus:
        tracks.append({
            "key": "decompilation",
            "label": "Ghidra tracing",
            "pct": round(scus["pct"], 1),
            "headline": "%.1f%% of SCUS_942.54's code" % scus["pct"],
            "detail": "Share of the main executable's code bytes that sit inside a "
                      "Ghidra-dumped function. Overlay images are excluded - they "
                      "alias in address space, so address attribution alone cannot "
                      "support a figure.",
            "denominator": "disc bytes",
            "href": "tooling/disc-coverage.html",
        })

    if data:
        tracks.append({
            "key": "formats",
            "label": "Asset formats",
            "pct": round(data["pct_content_parsed"], 1),
            "headline": "%.1f%% of PROT.DAT content" % data["pct_content_parsed"],
            "detail": "Share of disc asset bytes whose container resolves to a "
                      "documented format. This is format *recognition*, not a "
                      "byte-for-byte parse, so read it as an upper bound. "
                      "%.1f%% remains unexplained. %d entries (%s bytes) of "
                      "reserved dev filler are excluded from the denominator: "
                      "each is a documented one-sector placeholder with nothing "
                      "in it to parse."
                      % (data["pct_unexplained"], data["filler_entries"],
                         "{:,}".format(data["filler"])),
            "denominator": "disc bytes",
            "href": "formats/index.html",
        })

    if cat and cat.get("ported") is not None:
        ported, worklist = cat["ported"], cat.get("worklist") or 0
        identified = ported + worklist
        tracks.append({
            "key": "port",
            "label": "Engine port",
            "pct": round(100.0 * ported / identified, 1) if identified else 0.0,
            "headline": "%d functions ported" % ported,
            "detail": "Of the retail functions identified as port sites, the share "
                      "carrying a from-scratch Rust implementation. %d remain on the "
                      "worklist. Measured against what the project has identified, "
                      "not against the whole game." % worklist,
            "denominator": "identified port sites",
            "href": "subsystems/engine.html",
        })

        live, inert = cat.get("live"), cat.get("inert")
        # A port the engine replaces by construction leaves BOTH sides of this
        # ratio: it is not wired and it never will be, so it is neither a
        # numerator nor a denominator. An older version of this track counted
        # every one of them as "not yet hosted", which overstated the wiring
        # worklist by everything PsyQ-shaped in the tree.
        replaced = cat.get("replaced") or 0
        owed = (inert - replaced) if inert is not None else None
        if live is not None and owed is not None and (live + owed):
            tracks.append({
                "key": "wiring",
                "label": "Port wiring",
                "pct": round(100.0 * live / (live + owed), 1),
                "headline": "%d of %d ported functions reachable" % (live, live + owed),
                "detail": "A ported function still needs a host that calls it. This "
                          "is the share reachable from a real entry point; %s %d "
                          "further ports are excluded from the denominator "
                          "entirely: the engine does their job by construction (CD "
                          "DMA, memory-card device I/O, MDEC channel sync, GPU "
                          "packet queues), so no host will ever call them and each "
                          "says which mechanism replaced it."
                          % (("every port a host is owed for is reached." if owed == 0
                              else "the remaining %d are implemented but not yet "
                                   "hosted, and each one says so in its source." % owed),
                             replaced),
                "denominator": "ported functions a host is owed for",
                "href": "subsystems/engine.html",
            })

    return {
        "_comment": "Committed build input for site/_gen.py. The site builds without "
                    "disc data, so these cannot be computed at deploy time. Refresh "
                    "locally with scripts/ci/update-progress-metrics.py and commit.",
        "tracks": tracks,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--print", dest="show", action="store_true")
    ap.add_argument("--skip-catalog", action="store_true",
                    help="skip the slow port-catalog pass and keep its committed tracks")
    ap.add_argument("--funcs", default=None,
                    help="dump corpus directory (default: this checkout's)")
    ap.add_argument("--extracted", default=None,
                    help="extracted disc directory (default: this checkout's)")
    ap.add_argument("--images-only", action="store_true",
                    help="refresh only image-port-status.json (the per-image "
                         "port table), leaving progress-metrics.json alone")
    ap.add_argument("--list-open", nargs="*", metavar="IMAGE", default=None,
                    help="print every open entry (optionally only for the named "
                         "image labels) with its function extent, and write nothing")
    args = ap.parse_args()

    if args.list_open is not None:
        global OPEN_SINK
        OPEN_SINK = []
        build_image_status(args.funcs, args.extracted, with_replaced=False)
        for label, a, cat in OPEN_SINK:
            if args.list_open and label not in args.list_open:
                continue
            print("%-28s %s  %s" % (label, a, cat or "-"))
        return 0

    scus, data = load_disc_coverage(args.funcs, args.extracted)
    if scus is None:
        print("[progress] SKIPPED - no dump corpus / extracted tree; nothing to "
              "refresh. Pass --funcs / --extracted to read another checkout's.")
        return 0

    images = build_image_status(args.funcs, args.extracted,
                                with_replaced=not args.skip_catalog)
    if images is not None:
        text = json.dumps(images, indent=2) + "\n"
        if args.show:
            sys.stdout.write(text)
        else:
            with open(IMAGE_STATUS_OUT, "w") as fh:
                fh.write(text)
            print("[progress] wrote %s" % IMAGE_STATUS_OUT)
            for r in images["images"]:
                print("  %-26s %5.1f%% of %4d functions, %3d open, code "
                      "identified %5.1f%%%s"
                      % (r["name"], r["pct_accounted"], r["functions"], r["open"],
                         r["pct_identified"],
                         "  COMPLETE" if r["fully_ported"] else ""))
    if args.images_only:
        return 0

    cat = None if args.skip_catalog else run_port_catalog(args.funcs)
    if args.skip_catalog and os.path.exists(OUT):
        prev = json.load(open(OUT))
        keep = {t["key"]: t for t in prev.get("tracks", [])}
        out = build(scus, data, None)
        have = {t["key"] for t in out["tracks"]}
        for key in ("port", "wiring"):
            if key in keep and key not in have:
                out["tracks"].append(keep[key])
    else:
        out = build(scus, data, cat)

    order = {"decompilation": 0, "formats": 1, "port": 2, "wiring": 3}
    out["tracks"].sort(key=lambda t: order.get(t["key"], 99))

    text = json.dumps(out, indent=2) + "\n"
    if args.show:
        sys.stdout.write(text)
        return 0
    with open(OUT, "w") as fh:
        fh.write(text)
    print("[progress] wrote %s" % OUT)
    for t in out["tracks"]:
        print("  %-14s %5.1f%%  (%s)" % (t["label"], t["pct"], t["denominator"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
