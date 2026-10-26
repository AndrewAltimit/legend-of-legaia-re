#!/usr/bin/env python3
"""Capture coverage: which parts of the game the retail comparison corpus shows.

The retail comparison corpus (`docs/tooling/retail-compare.md`) scores the
engine at every captured moment; its median is only as broad as the moments it
holds. This report takes the denominator from the disc, the way
`disc-coverage.py` does for code, and asks per item whether at least one
retail-disc corpus state covers it:

    scenes          every town / field / world-map block (data/gamedata/scenes.toml)
    monsters        every populated monster record (PROT 0867)
    enemy attacks   every (monster, attack) pair: its physical attack plus
                    each named magic attack its record carries
    player casts    every named summon-band spell (0x81..=0xA0)
    cast modules    every slot-B cast / summon image (PROT 0903..0966)
    mode classes    every corpus class, and the pause-menu screens

"Covered" has two tiers: `held` (a retail-disc state shows it) and `scored`
(that state is in the committed baseline with an image score). States made on a
patched disc (`resident_patch`) never cover anything.

The per-state facts come from `legaia-engine retail-compare --census`
(`crates/parity/src/retail_coverage.rs`), which reads each state's RAM with no
engine run, plus the disc's monster / spell / formation lists. The report and
the census carry names read off the disc, so both live under the gitignored
`captures/` directory. See `docs/tooling/capture-coverage.md`.

    scripts/ci/capture-coverage.py               # build, census, report
    scripts/ci/capture-coverage.py --no-build    # reuse target/release-test/legaia-engine
    scripts/ci/capture-coverage.py --census F    # report from an existing census JSON
    scripts/ci/capture-coverage.py --summary     # headline lines only

Exit 0 and a `[skip]` line when the library or the extracted disc is missing.
"""

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / "scripts" / "ci" / "retail-compare-baseline.json"
SCENES = REPO / "data" / "gamedata" / "scenes.toml"
PROFILE = "release-test"

# Scene categories a state can stand in (the rest are cutscene, system,
# audio and battle-stage blocks).
SCENE_CATEGORIES = ("town", "field", "world_map")
# The pause menu's screens (`DAT_801E46A4`, docs/tooling/retail-compare.md
# "Menu states") plus the card-boot / title family (word clear) and the
# script-entered prize exchange.
MENU_SCREENS = {
    0x00: "title / card boot",
    0x01: "pause root",
    0x05: "Items",
    0x0E: "Magic",
    0x12: "Equip character pick",
    0x13: "Equip slot browse",
    0x14: "Equip candidates",
    0x15: "Status",
    0x17: "Options",
    0x18: "Load",
    0x19: "Save",
    0x20: "prize exchange",
}
CLASSES = ("field", "world_map", "battle", "menu", "field_init", "minigame", "cutscene", "other")
# The summon band the player casts (`FUN_8003EC70(id - 0x79)`, PROT 0903..0934).
PLAYER_CASTS = range(0x81, 0xA1)
CAST_MODULES = range(903, 967)
PHYSICAL = "physical"


def _load_retail_compare_driver():
    spec = importlib.util.spec_from_file_location(
        "retail_compare_driver", REPO / "scripts" / "ci" / "retail-compare.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run_census(out: Path, build: bool) -> Path | None:
    rc = _load_retail_compare_driver()
    library = rc.resolve("LEGAIA_SAVES_LIBRARY", "saves/library", Path.is_dir)
    extracted = rc.resolve(
        "LEGAIA_EXTRACTED_DIR", "extracted",
        lambda p: (p / "PROT.DAT").exists() and (p / "CDNAME.TXT").exists(),
    )
    if library is None or extracted is None:
        print("[skip] save library or extracted disc not found")
        return None
    if build:
        subprocess.run(
            ["cargo", "build", "-p", "legaia-engine-shell", "--profile", PROFILE,
             "--features", "oracles", "--bin", "legaia-engine"],
            cwd=REPO, check=True,
        )
    out.mkdir(parents=True, exist_ok=True)
    census = out / "census.json"
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "error")
    subprocess.run(
        [str(REPO / "target" / PROFILE / "legaia-engine"), "retail-compare",
         "--library", str(library), "--extracted-root", str(extracted),
         "--manifest", str(REPO / "scripts" / "scenarios.toml"),
         "--out", str(out), "--census", str(census)],
        cwd=REPO, env=env, check=True,
    )
    return census


class Coverage:
    """One dimension: items in disc order, each with the states that cover it."""

    def __init__(self, name: str):
        self.name = name
        self.items: dict = {}  # key -> label
        self.held: dict = defaultdict(list)
        self.scored: dict = defaultdict(list)
        self.rank: dict = {}  # key -> sort weight (higher = more valuable)
        self.hint: dict = {}  # key -> where to capture it

    def add(self, key, label, rank=0, hint=""):
        self.items[key] = label
        self.rank[key] = rank
        self.hint[key] = hint

    def cover(self, key, state_label, scored):
        if key not in self.items:
            return
        self.held[key].append(state_label)
        if scored:
            self.scored[key].append(state_label)

    def counts(self):
        n = len(self.items)
        held = sum(1 for k in self.items if self.held[k])
        scored = sum(1 for k in self.items if self.scored[k])
        return n, held, scored

    def uncovered(self):
        keys = [k for k in self.items if not self.held[k]]
        return sorted(keys, key=lambda k: (-self.rank[k], list(self.items).index(k)))


def build(census: dict, baseline: dict) -> dict:
    scenes_toml = tomllib.loads(SCENES.read_text())["scene"]
    disc = census["disc"]
    base_states = baseline.get("states", {})

    def scored(label):
        return "image" in base_states.get(label, {})

    retail = [s for s in census["states"] if not s["resident_patch"] and not s["error"]]

    # Where each monster is met: scene label -> formation rows naming it.
    met = defaultdict(lambda: defaultdict(int))
    for sc in disc["scenes"]:
        for row in sc["formations"]:
            for mid in set(row):
                met[mid][sc["label"]] += 1

    def where(mid):
        scenes = sorted(met[mid].items(), key=lambda kv: -kv[1])
        return ", ".join(s for s, _ in scenes[:4]) or "no MAN formation row (scripted / battle-id fight)"

    spell_name = {s["id"]: s["name"] for s in disc["spells"]}
    spell_module = {s["id"]: s["module_prot"] for s in disc["spells"]}

    dims = {}
    sc = dims["scenes"] = Coverage("scenes")
    for row in scenes_toml:
        if row["category"] in SCENE_CATEGORIES:
            sc.add(row["id"], f'{row["id"]} ({row["name"]}, {row["category"]})',
                   rank={"town": 2, "field": 1}.get(row["category"], 0))
    walk = dims["scene_walk"] = Coverage("scenes walked (field / world-map state)")
    for k, v in sc.items.items():
        walk.add(k, v, sc.rank[k])

    mo = dims["monsters"] = Coverage("monsters")
    for m in disc["monsters"]:
        rows = sum(met[m["id"]].values())
        mo.add(m["id"], f'{m["id"]:3} {m["name"]}', rank=rows, hint=where(m["id"]))

    ea = dims["enemy_attacks"] = Coverage("enemy attacks (monster, attack)")
    es = dims["enemy_spells"] = Coverage("enemy magic attacks (distinct spell id)")
    users = defaultdict(list)
    for m in disc["monsters"]:
        rows = sum(met[m["id"]].values())
        ea.add((m["id"], PHYSICAL), f'{m["id"]:3} {m["name"]}: physical', rank=rows, hint=where(m["id"]))
        for sid in m["magic_attacks"]:
            ea.add((m["id"], sid), f'{m["id"]:3} {m["name"]}: 0x{sid:02X} {spell_name.get(sid)}',
                   rank=rows, hint=where(m["id"]))
            users[sid].append(m["id"])
    for sid in sorted(users):
        es.add(sid, f"0x{sid:02X} {spell_name.get(sid)}", rank=len(users[sid]),
               hint="cast by " + ", ".join(str(u) for u in users[sid][:6]))

    pc = dims["player_casts"] = Coverage("player summon-band casts")
    for sid in PLAYER_CASTS:
        if spell_name.get(sid):
            pc.add(sid, f"0x{sid:02X} {spell_name[sid]}")

    cm = dims["cast_modules"] = Coverage("slot-B cast modules")
    module_spells = defaultdict(list)
    for s in disc["spells"]:
        if s["module_prot"] and s["name"]:
            module_spells[s["module_prot"]].append(f'0x{s["id"]:02X} {s["name"]}')
    for prot in CAST_MODULES:
        cm.add(prot, f"{prot:04} " + "; ".join(module_spells[prot][:3]),
               rank=len(module_spells[prot]))

    mc = dims["mode_classes"] = Coverage("game-mode classes")
    for c in CLASSES:
        mc.add(c, c)
    ms = dims["menu_screens"] = Coverage("menu screens")
    for k, v in MENU_SCREENS.items():
        ms.add(k, f"0x{k:02X} {v}")

    for s in retail:
        lab, sco, cls = s["label"], scored(s["label"]), s["class"]
        sc.cover(s["scene"], lab, sco)
        if cls in ("field", "world_map"):
            walk.cover(s["scene"], lab, sco)
        mc.cover(cls, lab, sco)
        if cls == "menu":
            ms.cover(s["menu_subscreen"] if s["menu_subscreen"] is not None else 0x00, lab, sco)
        b = s["battle"]
        if not b:
            continue
        for mid in set(b["monster_ids"]):
            mo.cover(mid, lab, sco)
        acting = b["band"] in ("attack", "magic", "summon", "module")
        if not acting:
            continue
        q = b["queued_action"]
        if b["active_monster"] is not None:
            m = b["active_monster"]
            if b["category"] == 3 and b["band"] == "attack":
                ea.cover((m, PHYSICAL), lab, sco)
            elif b["category"] == 2:
                # A boss's scripted cast (the picker's per-monster switch)
                # names an id its record does not carry: the census is the
                # only witness, so it joins the denominator when seen.
                if (m, q) not in ea.items:
                    name = next((x["name"] for x in disc["monsters"] if x["id"] == m), "?")
                    ea.add((m, q), f"{m:3} {name}: 0x{q:02X} {spell_name.get(q)} (scripted)")
                if q not in es.items:
                    es.add(q, f"0x{q:02X} {spell_name.get(q)} (scripted)")
                ea.cover((m, q), lab, sco)
                es.cover(q, lab, sco)
                cm.cover(spell_module.get(q), lab, sco)
        elif b["category"] in (1, 2):
            pc.cover(q, lab, sco)
            cm.cover(spell_module.get(q), lab, sco)

    return {"dims": dims, "retail_states": len(retail), "states": len(census["states"])}


def render(result: dict, worklist_rows: int) -> str:
    dims = result["dims"]
    out = ["# Capture coverage", ""]
    out.append(f'{result["retail_states"]} retail-disc corpus states of {result["states"]} read; '
               "`held` = a retail state shows the item, `scored` = that state carries a baselined image score.")
    out += ["", "| Dimension | Items | Held | Scored |", "|---|---|---|---|"]
    for d in dims.values():
        n, h, s = d.counts()
        out.append(f"| {d.name} | {n} | {h} ({h / n:.0%}) | {s} ({s / n:.0%}) |" if n else f"| {d.name} | 0 | - | - |")
    for key in ("enemy_attacks", "monsters", "enemy_spells", "scene_walk", "cast_modules",
                "player_casts", "menu_screens", "mode_classes"):
        d = dims[key]
        un = d.uncovered()
        out += ["", f"## Uncovered {d.name} ({len(un)})", ""]
        if not un:
            out.append("none")
            continue
        out += ["| Item | Weight | Where to capture |", "|---|---|---|"]
        for k in un[:worklist_rows]:
            out.append(f"| {d.items[k]} | {d.rank[k]} | {d.hint[k]} |")
        if len(un) > worklist_rows:
            out.append(f"| ... {len(un) - worklist_rows} more | | |")
    out += ["", "## Held items and their states", ""]
    for d in dims.values():
        held = [k for k in d.items if d.held[k]]
        if not held:
            continue
        out.append(f"### {d.name}")
        out.append("")
        for k in held:
            tag = "scored" if d.scored[k] else "held"
            out.append(f"- {d.items[k]} [{tag}]: {', '.join(d.held[k][:4])}"
                       + (f" (+{len(d.held[k]) - 4})" if len(d.held[k]) > 4 else ""))
        out.append("")
    return "\n".join(out) + "\n"


def summary_json(result: dict) -> dict:
    return {k: dict(zip(("items", "held", "scored"), d.counts())) for k, d in result["dims"].items()}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--census", type=Path, help="report from this census JSON instead of running one")
    ap.add_argument("--out", type=Path, default=REPO / "captures" / "capture-coverage")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--rows", type=int, default=40, help="worklist rows per dimension")
    ap.add_argument("--summary", action="store_true", help="print the headline table only")
    a = ap.parse_args()
    census_path = a.census or run_census(a.out, not a.no_build)
    if census_path is None:
        return 0
    census = json.loads(census_path.read_text())
    baseline = json.loads(BASELINE.read_text()) if BASELINE.exists() else {}
    result = build(census, baseline)
    a.out.mkdir(parents=True, exist_ok=True)
    (a.out / "report.md").write_text(render(result, a.rows))
    (a.out / "summary.json").write_text(json.dumps(summary_json(result), indent=1) + "\n")
    for k, v in summary_json(result).items():
        n = v["items"] or 1
        print(f'{k:<14} {v["items"]:4} items  held {v["held"]:4} ({v["held"] / n:5.1%})  '
              f'scored {v["scored"]:4} ({v["scored"] / n:5.1%})')
    if not a.summary:
        print(f"report -> {a.out / 'report.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
