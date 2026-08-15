#!/usr/bin/env python3
"""Join a replay's runtime coverage against the port catalog.

Every other reachability number in this repo is computed by the engine about
the engine: `port-catalog.py --live` walks a *static* call graph from the host
roots and asks whether a `// PORT:`-tagged Rust symbol is reachable in
principle. That graph is deliberately permissive (see the two-graph split in
`docs/tooling/port-catalog.md`), so "live" is an upper bound - it says
reachable, not reached.

This script supplies the missing denominator: what a **pad-driven run actually
executes**. It consumes `cargo llvm-cov` output for one or more replay ladders
and joins the per-function execution counts against the catalog's
address -> (file, line) anchors, reporting three sets:

  inert-entered   an address the static graph calls NOT reachable from any host
                  root, whose anchor ran anyway. The graph was wrong, or the
                  tag is on the wrong symbol. Each one is a finding.

  disclosed-entered
                  an anchor carrying a `NOT WIRED:` disclosure that ran anyway.
                  These are the dangerous ones: a passing oracle traversed code
                  the source says nothing reaches, so the oracle may be
                  certifying a stub.

  live-unentered  an address the static graph calls live that no run entered.
                  Not a defect - this is the prioritisation list, and it is only
                  meaningful relative to how far the ladders get.

## A const anchor has no lines to execute

Line coverage answers "did this code run", and a `// PORT:` tag above a
`const` / `static` / `type` alias anchors to an item with **no code**. The
naive resolution - read the verdict of the next function in the file - turns
the neighbour's coverage into the const's, which both *accused* honestly
disclosed `NOT WIRED` consts of executing (the sibling ran) and *credited*
never-referenced consts as entered. So item anchors take their executed
verdict from attributable REFERENCES instead, mirroring the catalog's strict
item-liveness rule (`port-catalog.py: item_reference_patterns` /
`item_reference_hit` - one definition, both consumers): the item is executed
iff some executed non-test `fn` names it bare from the item's own file or
qualified (`module::NAME` / `Type::NAME`) from anywhere. If no attributable
referencing function executed, the verdict is **not observable (const)** - a
fourth bucket, deliberately neither "entered" nor "never entered", because a
worklist row that no line of coverage can ever convert is not work in this
report's sense. Type anchors (struct / enum / trait) resolve the same way the
catalog does: through the executed methods of the type's own `impl` blocks,
falling back to the file when the file gives the type no `impl`.

## The denominator is a union of ladders, not one session

`--json` is **repeatable**, and the difference matters more than it looks. The
repo drives several pad-only ladders, each its own test binary and each at full
score: `critical_path_replay` (the world spine), `menu_replay` (the pause menu
and save UI), `minigame_replay` (the five minigame doors), `v0_1_playthrough`
(the cold-boot field anchor), and `play_compose_ladder` (the browser play
page's draw composition - see below). A join over only the
first reports the menu and minigame subsystems as never-entered - and those two
are the largest clusters in the live-unentered list, so a worklist ordered off
a single-binary run is ordered against a measurement that structurally excluded
its own top rows.

## One ladder is a rendering host

The four `engine-shell` ladders drive the headless `BootSession`, which builds
no draw list - so `engine-ui` reported zero executed regions across their whole
union, and the largest NO-LADDER cluster in `docs/tooling/reach-triage.md` was
invisible to this report by construction. `play_compose_ladder`
(`crates/web-viewer/tests/`) closes that shape: the browser play page's
composition is *library* code, so the ladder feeds pad words per tick and calls
the page's per-frame read surface (`play_overlay_draws_json`, the menu / battle
/ fishing / dev-menu overlays, the screen-prim route, the battle 3D + FX
exports). It is still pad-only - nothing is seated or poked - but its frames
are composed, which is exactly the half of a rendering host the native window
keeps locked in its `bin/` target.

So the headline number is the **union** across every `--json` given, and the
per-source table below it reports what each ladder contributed and how much of
that no other ladder reached. A union across separate sessions is not one
continuous playthrough and the report says so; what it does measure honestly is
"code some pad-driven run entered" versus "code no pad-driven run entered".

## The union is the default, and a short join says so

With no `--json`, the script discovers `target/cov-*.json` and joins **all** of
them, so the bare invocation is the union rather than one binary. It also knows
the canonical ladder set ([`CANONICAL_LADDERS`]) and names any member whose
export is missing, in the report and on stdout - because a union over three of
the four is not a smaller version of the number, it is a different number, and
the earlier single-binary default understated the denominator by 1.7x without
saying anything at all.

The cost is why this stays a manual step rather than a pre-commit gate: each
export is an instrumented build plus a full disc-gated ladder run, and the
ladders are minutes each. (Instrumented, NOT `--release` - the recipe below
passes no `--release` anywhere, because the line table a release build emits
does not survive the span join.) `--fail-on-disclosed` is the gateable part, and
it is the part that needs no complete union to be meaningful.

Usage:
    # produce one export per ladder (slow: instrumented build of the engine
    # crates). One export each rather than one multi---test run: the
    # per-ladder table is only possible when the exports are separate, and
    # that table is what makes dropping a ladder a visible cost. The package
    # differs per ladder ([`CANONICAL_LADDERS`] carries it): the engine-shell
    # ladders and the web-viewer composition ladder are different crates, and
    # `--test` links each crate's LIBRARY - which is why the web-viewer route
    # can execute composition code the engine-shell `bin/` never exposes.
    # Clean first: the reader collapses duplicate spans keyed on the exact
    # (line_start, line_end), so a stale sibling binary left in `target/` can
    # shadow a fresh executed record with its own zero.
    cargo llvm-cov clean --workspace

    # NO `--release` anywhere - see "a release export loses executed code"
    # below. The whole set (`--list-ladders`) takes tens of minutes on the default
    # profile (`menu_replay` is a 4.7s run), so the optimised build buys
    # nothing here and costs executed code.
    # `--list-ladders` prints `<test> <package>` for every canonical entry, so
    # the recipe cannot drift from the list the report checks against.
    #
    # TWO STEPS PER LADDER, and the split is load-bearing. `-p <pkg> --json`
    # scopes the *report* to that package's own sources, not just the build -
    # so a one-shot export of `menu_replay` carries 42 files, all under
    # `crates/engine-shell/`, while everything the ladder drives is in
    # `engine-core` / `engine-vm` / `engine-ui` and is silently absent. Build
    # scoped (`-p` picks the ladder), then report UNSCOPED over the profiles.
    #
    # `--profraw-only` between ladders keeps the per-ladder table honest: it
    # drops the profile data so each export is that ladder alone, while leaving
    # the build artifacts in place so this is not 26 rebuilds.
    scripts/ci/replay-port-coverage.py --list-ladders | while read -r t pkg; do
        cargo llvm-cov clean --profraw-only
        cargo llvm-cov -p "$pkg" --test "$t" --no-report
        cargo llvm-cov report --json --output-path "target/cov-$t.json"
    done

    # then join them - no arguments needed, the union is the default
    scripts/ci/replay-port-coverage.py

## A release export loses executed code, silently

`-C instrument-coverage` counters are emitted per function, but an optimised
build inlines the small ones away and the out-of-line copy's record is left at
**zero** - indistinguishable, in this join, from a function no run called. It is
not a rare shape: on one `w1a_fmv_ladder` run, `advance_slice` (40 calls),
`slice_word_count` (39) and `mdec_output_control` / `skip_requested` /
`mdec_control_flags` (1-3 each) all reported `count = 0` under `--release` and
their real counts under the default profile, from the same ladder over the same
disc. Two of them are `mdec` rows this report had listed as *live but never
entered*, which was an artifact of the export rather than a gap in the ladders.

So a `--release` row in the never-entered list is a **hypothesis**, not a
measurement: re-export that ladder without `--release` before reading it as
work. The flag survives above only because the five exports it produced are what
the existing numbers came from; re-taking them is the fix, and it changes the
headline.

Skips (exit 0) when every coverage JSON is absent, so a disc-free or
coverage-toolless CI run is a pass, matching the `LEGAIA_DISC_BIN` convention.
"""

from __future__ import annotations

import argparse
import bisect
import importlib.util
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
CATALOG = REPO / "scripts" / "ci" / "port-catalog.py"
COV_DIR = REPO / "target"
COV_GLOB = "cov-*.json"
LEGACY_JSON = COV_DIR / "replay-cov.json"
DEFAULT_OUT = REPO / "target" / "port-catalog" / "replay-port-entry.md"

# The pad-only ladders that make up the canonical denominator, in the order
# the usage block exports them, as `(test_name, cargo_package)`. Membership is
# "drives the engine with `set_pad` and nothing else" - a ladder that seats the
# player is measuring a different thing (see `critical_path_replay`'s module
# docs on the spine oracle). `play_compose_ladder` additionally *composes* a
# frame per tick from the browser play page's builders, which is what makes
# the union see a rendering host at all (see the module docstring).
#
# This list exists so a *partial* union is visible. A union missing a ladder
# is not a conservative version of the full number; it is a number about the
# ladders that ran, and without naming the absentees it reads exactly like the
# complete one. `--list-ladders` prints the current membership - prefer it to
# any count written down elsewhere, which is what goes stale as ladders land.
CANONICAL_LADDERS = [
    ("critical_path_replay", "legaia-engine-shell"),
    ("menu_replay", "legaia-engine-shell"),
    ("minigame_replay", "legaia-engine-shell"),
    ("v0_1_playthrough", "legaia-engine-shell"),
    ("play_compose_ladder", "legaia-web-viewer"),
    # --- lane W1-A ---
    # Two ladders for content no pad-only *engine-shell* ladder can reach.
    #
    # `w1a_fmv_ladder` plays every retail `fmv_id` through the retail STR chain
    # (dispatch slot -> file seek -> `StrPlayer` -> `MdecDecoder` -> pad skip).
    # No other union member plays a movie at all: the headless ladders have no
    # cutscene rung and the browser play page's FMV arm auto-skips, so the whole
    # `mdec` crate reported zero executed regions - a fact about the harness,
    # not the port. Its fourth rung additionally *spawns* `CARGO_BIN_EXE_mdec`,
    # which is how the MDEC-hardware half (whose only host is the `mdec
    # str-plan` subcommand, in a `bin/` target) runs under coverage at all.
    #
    # That rung also settles a structural claim this report has been read
    # against: "a `bin/` target is unreachable from any `#[test]`" is true of
    # *calls* and false of *coverage*. `LLVM_PROFILE_FILE` is inherited, so a
    # spawned `CARGO_BIN_EXE_*` writes its own profile and it is merged in -
    # measured: `mdec::cmd_str_plan` reports 40 executions from a test that
    # only spawned it. Every `bin/`-resident row on this report's never-entered
    # list is therefore reachable by a ladder that runs the subcommand.
    #
    # `w1a_narration_ladder` drives the two text presenters a pad run reaches
    # and no ladder did: the opening-prologue subtitle roller and the inline
    # field-VM conversation path (as opposed to the pre-decoded dialog panel
    # every other ladder drives).
    #
    # Both are disc-gated and skip-pass without `LEGAIA_DISC_BIN`, exactly like
    # the ladders above; a disc-free export therefore contributes no executed
    # regions rather than failing, which is the same shape the rest of the union
    # already has.
    #
    # Both are exported WITHOUT `--release`, and that is not a detail: see the
    # module docstring's "a release export loses executed code". These two
    # ladders are where that was measured.
    ("w1a_fmv_ladder", "legaia-mdec"),
    ("w1a_narration_ladder", "legaia-engine-core"),
    # --- lane W1-C ---
    ("w1c_battle_render_ladder", "legaia-web-viewer"),
    ("w1c_arts_swing_ladder", "legaia-engine-shell"),
    # --- lane W1-B ---
    # Three pad-driven ladders that live in `engine-core` because what they
    # drive is the world tick rather than a host: the op-0x49 submode screens
    # opened from a field-VM instruction, a Baka duel played through its
    # cabinet intro to the end-of-match tally, and a Muscle Dome leg played to
    # its between-leg tally. Each reaches content no engine-shell or
    # web-viewer ladder does, so leaving them out is a different number rather
    # than a smaller one.
    ("w1b_hub_ladder", "legaia-engine-core"),
    ("w1b_baka_duel_ladder", "legaia-engine-core"),
    ("w1b_dome_leg_ladder", "legaia-engine-core"),
    # --- lanes W1-D / W1-E / W1-F, plus four older ladders ---
    # These existed and ran green while being absent from this list, which is
    # the export *recipe* - so nobody exported them, no `cov-*.json` was ever
    # written for them, and every row they reach kept reading *live but never
    # entered*. A ladder written to convert a row cannot convert it until it is
    # named here. When a lane adds a ladder, it belongs in this list in the same
    # commit.
    ("w1d_world_map_render_ladder", "legaia-engine-core"),
    ("w1d_dev_menu_equip_ladder", "legaia-web-viewer"),
    ("w1e_audio_session_ladder", "legaia-engine-audio"),
    ("w1e_scene_bgm_transition_ladder", "legaia-engine-core"),
    ("w1f1_battle_tutorial_ladder", "legaia-engine-core"),
    ("w1f1_fishing_pond_ladder", "legaia-engine-core"),
    ("w1f1_fishing_banner_ladder", "legaia-web-viewer"),
    ("w1f1_pause_special_use_ladder", "legaia-engine-core"),
    ("w1f2_menu_depth_ladder", "legaia-web-viewer"),
    ("battle_depth_replay", "legaia-engine-shell"),
    ("battle_flee_ladder", "legaia-engine-core"),
    ("battle_full_playthrough", "legaia-engine-core"),
    ("seru_cast_magic_xp_ladder", "legaia-engine-core"),
    # --- lane L5 ---
    # The native window's own composition layer, reached by SPAWNING it.
    #
    # `w5_native_minigame_ladder` runs `CARGO_BIN_EXE_legaia-engine
    # play-window` once per minigame and lets the inherited `LLVM_PROFILE_FILE`
    # merge each child's profile into this export. That is the same route the
    # `mdec` ladder's fourth rung takes, and it is why "no `#[test]` can call
    # into a `bin/` target" bounds *calls* and not coverage: the whole
    # `crates/engine-shell/src/bin/legaia-engine/` tree - the HUD builders, the
    # minigame side-channel tick, the field render pass - executes here and
    # nowhere else in the union.
    #
    # What made the cluster reachable at all is a CLI channel rather than a
    # file move: `--pad-script` writes a pad word and the window's keyboard
    # handler never runs, while every native minigame entry lives in that
    # handler, so no pad-only run could open one. `--key-script` delivers keys
    # through the same arms a player's keyboard does, and the two scripts
    # compose - keys open the surface, the pad plays it.
    #
    # Two gates beyond the disc: the rungs need a display (`play-window` needs
    # a real wgpu surface even for its offscreen capture) and they assert on
    # the captured FRAME, not on the exit status - a HUD builder that emits an
    # empty draw list would pass "did it run" and fails here.
    #
    # EXPORT THIS ONE IN TWO STEPS. `-p <pkg>` scopes the *report* to that
    # package's sources, not only the build, and this ladder's whole yield is
    # in other crates: a one-shot `-p legaia-engine-shell --json` export of it
    # carries 42 files, all under `crates/engine-shell/`, while the dance HUD,
    # the fishing chrome, the Baka number drawers and the casino counter are
    # `engine-core` and the builders under them are `engine-ui`. Reporting
    # without `-p` over the same profiles carries 652 files across fifteen
    # crates - including `engine-render`, which this page records as
    # structurally unreachable because the browser composition ladder cannot
    # carry a wgpu link. The native window is that link.
    #
    #     cargo llvm-cov clean --workspace
    #     cargo llvm-cov -p legaia-engine-shell \
    #         --test w5_native_minigame_ladder --no-report -- --test-threads=1
    #     cargo llvm-cov report --json \
    #         --output-path target/cov-w5_native_minigame_ladder.json
    #
    # The same scoping applies to every `-p`-exported ladder above, so a row
    # that reads never-entered may be a row whose only driver exported it out
    # of scope. That is worth re-measuring before spending a fixture.
    ("w5_native_minigame_ladder", "legaia-engine-shell"),
    # --- lane L4 ---
    # Three ladders for content the union is blind to *by construction* rather
    # than by depth.
    #
    # `w1l4_page_compose_ladder` drives the browser surfaces `play_compose_ladder`
    # opens but cannot populate. Its Load rung already reaches the card rack -
    # with no card inserted, so `card_block_snapshots` takes its `None` early
    # return and the retail directory walk behind it never runs. This one inserts
    # real card images, prices their free blocks, commits a save through the
    # filename classifier, and walks the info panel's three modes. It also drives
    # the two standalone minigame pages and the Items target panel, whose
    # preview word needs a bag the page has no affordance to fill (it arrives on
    # a resumed save, as a player's would).
    #
    # `w1l4_slot_bonus_marquee_ladder` plays the casino machine into a **bonus
    # round** and composes the dot-matrix marquee off that round's own live
    # state. The schedule is solved rather than rolled for: the machine is
    # deterministic and `SlotMachine` is `Clone`, so each candidate stop frame is
    # probed on a copy and the RNG is untouched by the search.
    #
    # `w1l4_slot_bonus_marquee` is the disc-data half of the same gate, and the
    # one that can spawn `CARGO_BIN_EXE_asset` - `asset slot-scene` is the only
    # non-test caller the marquee kernels have, and a spawned bin inherits
    # `LLVM_PROFILE_FILE` (the same mechanism `w1a_fmv_ladder` uses for `mdec`).
    #
    # All three are disc-gated and skip-pass without `LEGAIA_DISC_BIN`, and all
    # three are exported WITHOUT `--release` - see the module docstring's "a
    # release export loses executed code".
    ("w1l4_page_compose_ladder", "legaia-web-viewer"),
    ("w1l4_slot_bonus_marquee_ladder", "legaia-web-viewer"),
    ("w1l4_slot_bonus_marquee", "legaia-asset"),
    # --- lane L1 ---
    # The browser play page driven into a battle with retail's own one-shot
    # arm flag raised, so the tutorial box the page composes actually exists.
    ("battle_tutorial_page", "legaia-web-viewer"),
    # --- lane L2 ---
    # The pause menu's three data-driven kernels, driven to their OUTPUT (bag
    # count, picked weapon id, destination rows) rather than to their call.
    ("l2_menu_data_wiring", "legaia-engine-core"),
    ("l2_menu_data_wiring_disc", "legaia-engine-core"),
    # --- lane L3 ---
    # Five fixtures for the GATED-(b) rows: content behind a story flag, a
    # scene, or a battle state no pad stream reaches from a cold boot.
    #
    # These are **gate-seeded**, not pad-only, and the distinction has to stay
    # visible in this list or the denominator quietly changes meaning. Each one
    # writes exactly the one piece of state the gate is (a system flag, a
    # status effect, a visited-map record) and then drives the ordinary engine
    # path - `World::tick`, `World::step_field`, the scene loader - from there.
    # Three take their bytecode from the disc corpus rather than inventing it,
    # so they skip-pass without `LEGAIA_DISC_BIN` like the rest of the union.
    #
    # `field_ledge_hop_disc` is not new: it already drove the ledge-hop rows
    # (`801d2404` / `801d2298`) off a real `town01` ledge and was simply never
    # in the union, which is why those rows read as never-entered.
    #
    # Export them WITHOUT `--release` (see "a release export loses executed
    # code" above) - several of the kernels here are small enough to inline.
    ("l3_scripted_scene_program_gate", "legaia-engine-core"),
    ("l3_gated_field_arms_disc", "legaia-engine-core"),
    ("l3_confused_monster_target_gate", "legaia-engine-core"),
    ("l3_travel_art_visited_gate", "legaia-engine-core"),
    ("field_ledge_hop_disc", "legaia-engine-core"),
    # --- lane W2C ---
    # Five existing tests that were already the honest drivers for
    # never-entered rows and were simply absent from this list (the W1-D/E/F
    # shape: a ladder converts nothing until it is named here), plus two new
    # ladders.
    #
    # `pause_menu_compose` composes every pause screen through the shared
    # session-path builders on both chrome passes: the Items special-Use
    # confirm (801d1dac / 801d1f10), its hand quad (801e3ff0), and the three
    # painter-gated windows 5 / 6 / 7 (801d61b0 / 801d6360 / 801dccb4).
    # Disc-free.
    #
    # `dome_ladder_and_hub_real` builds the Muscle Dome hub screens off the
    # disc's recovered draw lists - the score tally's label strips are the
    # corner-anchored emitter (801d08ec). `character_pack_real` applies the
    # equipment group-swap patch to every active-party slot (8001ebec).
    # `boot_overlay_disc` resolves the boot-time slot-B overlay choice and,
    # with this lane's rungs, the effect-data side-band branch and the
    # sector-count rounding against the real TOC (80025ba0 / 8003e360 /
    # 8001eef0). `cdname_retail_parse_disc` runs the lossy retail CDNAME
    # reader over the real file (8001d8fc). All four disc-gated skip-pass.
    #
    # `w2c_battle_fx_ladder` walks the move-FX streak counter through the
    # production dispatcher so the chained ribbon (801e1d98) is reached the
    # way a host reaches it - by the counter falling below 0x201, not by a
    # direct call. Disc-free.
    #
    # `w2c_card_inventory_ladder` runs the retail item-window accessor family
    # over a real memory card's SC block (select / find / consume / normalize
    # / add, 8004313c and friends). Keys on ~/.mednafen/sav like
    # real_card_roundtrip, not on the disc; skip-passes without a card.
    ("pause_menu_compose", "legaia-engine-ui"),
    ("w2c_battle_fx_ladder", "legaia-engine-ui"),
    ("dome_ladder_and_hub_real", "legaia-web-viewer"),
    ("character_pack_real", "legaia-asset"),
    ("boot_overlay_disc", "legaia-asset"),
    ("cdname_retail_parse_disc", "legaia-prot"),
    ("w2c_card_inventory_ladder", "legaia-save"),
    # --- lane W2B ---
    # The two remaining NO-LADDER rows whose blocker was scene content.
    #
    # `w2b_dialog_picker_ladder` drives a real multi-choice conversation (the
    # `koin4` merchant's two-price offer) through the session's field-interact
    # path into `OwnedDialogPanel::confirm_menu` (`80038050`) - the native
    # keyboard handler's own contract, previously reached by no ladder because
    # a menu needs a picker-bearing NPC record, not a harness entry point.
    #
    # `w2b_fmv_handoff_ladder` executes each of the eight trigger scenes' own
    # `4C E2` op through the session field VM (record bytes sliced at the op -
    # the surrounding story choreography spins on actor-motion waits no
    # headless world satisfies; see the ladder's module doc) and runs the
    # post-play hand-off map (`fmv_post_play_handoff`, `801cea3c`) to the
    # hand-off scene, completing one full Field -> Cutscene -> hand-off-scene
    # transfer.
    #
    # Both are disc-gated and skip-pass without `LEGAIA_DISC_BIN`; export both
    # WITHOUT `--release` (see "a release export loses executed code" above).
    ("w2b_dialog_picker_ladder", "legaia-engine-core"),
    ("w2b_fmv_handoff_ladder", "legaia-engine-core"),
    # --- lane L4 (playable-ch1) ---------------------------------------- <L4>
    # `chapter1_frontier_ladder` is the only union member denominated in
    # SCENES rather than in a route: it takes the BFS closure of `town01` over
    # the disc's own `0x3F` destination tables (27 scenes, bounded at the
    # `jiji -> map02` kingdom handoff) and drives every member through load ->
    # enter -> settle -> pad-walk -> exit.
    #
    # What that reaches and no route ladder does: the entry script, walk-on
    # dispatch, collision grid and exit-record path of twenty-two interiors
    # the pad ladders never visit - the Drake Castle chain (`jouina` ..
    # `jouine`), the Uru Mais rooms, `garmel`, `bylon`, `izumi`, `town0b`.
    # `critical_path_replay` walks five scenes well; this one walks
    # twenty-seven once each, so the two are different numbers rather than
    # different sizes of the same one.
    #
    # Disc-gated and skip-passes without `LEGAIA_DISC_BIN`; export WITHOUT
    # `--release` (see "a release export loses executed code" above).
    ("chapter1_frontier_ladder", "legaia-engine-core"),
    # ------------------------------------------------------------------ </L4>
]
CANONICAL_LADDER_NAMES = [name for name, _pkg in CANONICAL_LADDERS]


def discover_inputs() -> tuple[list[Path], list[str]]:
    """`(paths, missing_canonical_labels)` for a bare invocation.

    Every `target/cov-*.json` is joined, not just the canonical set, so a
    ladder added later is picked up without editing this file. The legacy
    single-file path is honoured only when the glob finds nothing at all.
    """
    found = sorted(COV_DIR.glob(COV_GLOB))
    if not found and LEGACY_JSON.is_file():
        # The pre-per-ladder path: one merged export whose contents cannot be
        # attributed to a ladder. Every canonical name is reported unjoined,
        # which is the honest reading - the per-ladder table cannot be built
        # from it, so nothing here can say which ladders it covers.
        return [LEGACY_JSON], list(CANONICAL_LADDER_NAMES)
    labels = {p.stem.removeprefix("cov-") for p in found}
    missing = [name for name in CANONICAL_LADDER_NAMES if name not in labels]
    return found, missing


def load_catalog_module():
    """Import `port-catalog.py` (hyphenated, so not a normal import)."""
    spec = importlib.util.spec_from_file_location("port_catalog", CATALOG)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {CATALOG}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def catalog_address_sets() -> tuple[set[str], set[str]]:
    """`(live, not_live)` address sets, from the catalog's own liveness pass.

    Shelled out rather than reimplemented: the verdict logic (module-scope and
    type-scope anchors, test-file exclusion, the receiver-gated sibling graph)
    is subtle enough that a second copy would drift, and the whole point of
    this script is to check the catalog rather than to restate it.
    """

    def run(flag: str) -> set[str]:
        proc = subprocess.run(
            [sys.executable, str(CATALOG), flag, "--no-write"],
            capture_output=True,
            text=True,
            cwd=REPO,
        )
        if proc.returncode != 0:
            raise SystemExit(f"port-catalog.py {flag} failed:\n{proc.stderr}")
        out = set()
        for line in proc.stdout.splitlines():
            token = line.split(None, 1)[0] if line.split() else ""
            if len(token) == 8 and all(c in "0123456789abcdef" for c in token):
                out.add(token.lower())
        return out

    return run("--live-only"), run("--not-live")


class FileCoverage:
    """Executed-line lookup for one source file, from llvm-cov regions.

    A function's regions are collapsed to its line span plus whether any region
    executed. An anchor is then resolved to a function the same way
    `collect_port_anchors` resolves it to a symbol: a tag inside a body belongs
    to the enclosing function, a tag above an item belongs to the next one.
    """

    def __init__(self):
        # (line_start, line_end, executed) per function in this file.
        self.spans: list[tuple[int, int, bool]] = []
        self._starts: list[int] = []

    def add_function(self, line_start: int, line_end: int, executed: bool):
        self.spans.append((line_start, line_end, executed))

    def finalize(self):
        # One llvm-cov export carries a record for every binary in the target
        # dir - `cargo llvm-cov --test X` still merges the coverage *mappings*
        # of the sibling test binaries built earlier into the same report, as
        # zero-count duplicates of the same functions. Collapse identical
        # spans with OR before sorting, or an unexecuted duplicate from a
        # binary the run never launched shadows the executed record and the
        # entered count collapses (measured: 265 -> 122 on the same run the
        # moment a second test binary existed in the target dir).
        merged: dict[tuple[int, int], bool] = {}
        for lo, hi, ex in self.spans:
            merged[(lo, hi)] = merged.get((lo, hi), False) or ex
        self.spans = [(lo, hi, ex) for (lo, hi), ex in merged.items()]
        self.spans.sort(key=lambda s: s[0])
        self._starts = [s[0] for s in self.spans]

    def any_executed(self) -> bool:
        return any(s[2] for s in self.spans)

    def verdict_at(self, line: int) -> bool | None:
        """`True`/`False` if a function covers or follows `line`, else `None`."""
        # 1. Enclosing function (tag written inside a body).
        enclosing = [s for s in self.spans if s[0] <= line <= s[1]]
        if enclosing:
            # Innermost wins: nested closures report their own spans.
            enclosing.sort(key=lambda s: s[1] - s[0])
            return enclosing[0][2]
        # 2. Otherwise the next function starting at/after the tag line.
        idx = bisect.bisect_left(self._starts, line)
        if idx < len(self.spans):
            return self.spans[idx][2]
        return None

    def executed_overlapping(self, lo: int, hi: int) -> bool:
        """Whether any executed span overlaps the line range `[lo, hi]`.

        The lookup for a *known* function span (an `fn` the catalog parsed from
        source), where `verdict_at`'s next-function approximation is both
        unnecessary and wrong - the source span and the llvm-cov span differ by
        attribute lines, so overlap is the honest join.
        """
        return any(ex for a, b, ex in self.spans if a <= hi and lo <= b)


def load_coverage(path: Path) -> dict[str, FileCoverage]:
    """Parse an `llvm-cov export` JSON into per-file function coverage."""
    raw = json.loads(path.read_text())
    files: dict[str, FileCoverage] = defaultdict(FileCoverage)
    for block in raw.get("data", []):
        for fn in block.get("functions", []):
            filenames = fn.get("filenames") or []
            regions = fn.get("regions") or []
            if not filenames or not regions:
                continue
            # regions: [line_start, col_start, line_end, col_end, count,
            #           file_id, expanded_file_id, kind]
            per_file: dict[int, list[tuple[int, int, int]]] = defaultdict(list)
            for r in regions:
                if len(r) < 6:
                    continue
                per_file[r[5]].append((r[0], r[2], r[4]))
            for file_id, rs in per_file.items():
                if file_id >= len(filenames):
                    continue
                name = filenames[file_id]
                lo = min(r[0] for r in rs)
                hi = max(r[1] for r in rs)
                executed = any(r[2] > 0 for r in rs)
                try:
                    rel = str(Path(name).resolve().relative_to(REPO))
                except ValueError:
                    rel = name
                files[rel].add_function(lo, hi, executed)
    for cov in files.values():
        cov.finalize()
    return files


def join_anchor_coverage(
    catalog,
    anchors: dict[str, list[dict]],
    srcs: dict,
    sources: list[tuple[str, dict[str, "FileCoverage"]]],
    live: set[str],
    not_live: set[str],
) -> dict:
    """Join every anchor against the coverage union; return the buckets.

    Split out of `main` so `--selftest` can drive it on a synthetic corpus.
    `catalog` is the imported `port-catalog.py` module - the item-reference
    rule comes from there (`item_reference_patterns` / `item_reference_hit`)
    so this join can never drift from the liveness verdict it is checking.
    """
    inert_entered: list[tuple[str, dict]] = []
    disclosed_entered: list[tuple[str, dict]] = []
    live_entered: set[str] = set()
    live_unentered: list[tuple[str, dict]] = []
    # An address whose every anchor file is absent from *every* source's
    # coverage was never observable - it is compiled into a different target
    # (wasm-only crates, other binaries). Counting it as "never entered" would
    # inflate the worklist with rows no run could have exercised either way,
    # and would make the zero buckets below look better-founded than they are.
    unobservable: list[tuple[str, dict]] = []
    # Item anchors (const / static / type alias) with no executed attributable
    # reference. See the module docstring: not "entered", not "never entered".
    not_observable_const: list[tuple[str, dict]] = []
    per_source_live: dict[str, set[str]] = {label: set() for label, _ in sources}

    all_files = {f for _, cov in sources for f in cov}

    def fn_executed_labels(f) -> set[str] | None:
        """Labels of sources that executed `f`; `None` if none can see it."""
        src = srcs[f.path]
        rel = str(f.path.relative_to(catalog.REPO))
        lo = f.line
        hi = src.line_of(max(f.body_end - 1, f.body_start))
        seen, labels = False, set()
        for label, cov in sources:
            fc = cov.get(rel)
            if fc is None:
                continue
            seen = True
            if fc.executed_overlapping(lo, hi):
                labels.add(label)
        return labels if seen else None

    # Pre-resolve every item-kind anchor once, across the union: the verdict
    # depends on the referencing functions' coverage, not on the anchor's own
    # file. `(executed_labels, observable)` per (path, symbol).
    item_keys: dict[tuple, str | None] = {}
    for sites in anchors.values():
        for site in sites:
            if site["kind"] == "item":
                item_keys[(site["path"], site["symbol"])] = site["type_name"]
    item_exec: dict[tuple, tuple[set[str], bool]] = {}
    for (p, n), ty in item_keys.items():
        bare, qual = catalog.item_reference_patterns(p, n, ty)
        labels: set[str] = set()
        observable = str(p.relative_to(catalog.REPO)) in all_files
        for src in srcs.values():
            for f in src.fns:
                if f.is_test:
                    continue
                body = src.stripped[f.body_start : f.body_end]
                if not catalog.item_reference_hit(f.path, body, p, bare, qual):
                    continue
                got = fn_executed_labels(f)
                if got is None:
                    continue
                observable = True
                labels |= got
        item_exec[(p, n)] = (labels, observable)

    # Per-file impl-block types and per-(file, type) non-test methods, for the
    # type-kind verdict below (mirrors the catalog's `type_scope` + its
    # no-`impl`-in-file fallback to module scope).
    impl_types: dict = {}
    methods_of: dict[tuple, list] = defaultdict(list)
    for p, src in srcs.items():
        impl_types[p] = {ty for _a, _b, ty, *_ in src.impl_spans}
        for f in src.fns:
            if not f.is_test and f.impl_type:
                methods_of[(p, f.impl_type)].append(f)

    def ran_in(cov: dict[str, "FileCoverage"], site: dict) -> bool | None:
        """`None` when this source cannot observe the site at all."""
        fc = cov.get(site["file"])
        if fc is None:
            return None
        if site["kind"] == "module":
            return fc.any_executed()
        if site["kind"] == "type":
            src = srcs[site["path"]]
            for f in methods_of[(site["path"], site["type_name"])]:
                lo = f.line
                hi = src.line_of(max(f.body_end - 1, f.body_start))
                if fc.executed_overlapping(lo, hi):
                    return True
            # A type the file never `impl`s (a plain data struct, a trait
            # whose implementations live elsewhere) has no method here to
            # observe - fall back to the file, as the liveness pass does.
            if site["type_name"] not in impl_types[site["path"]]:
                return fc.any_executed()
            return False
        return bool(fc.verdict_at(site["line"]))

    for addr, sites in sorted(anchors.items()):
        entered_site = None
        observable = False
        const_site = None
        for label, cov in sources:
            for site in sites:
                if site["kind"] == "item":
                    continue
                ran = ran_in(cov, site)
                if ran is None:
                    continue
                observable = True
                if ran:
                    if entered_site is None:
                        entered_site = site
                    if addr in live:
                        per_source_live[label].add(addr)
                    break
        if entered_site is None:
            for site in sites:
                if site["kind"] != "item":
                    continue
                labels, item_obs = item_exec[(site["path"], site["symbol"])]
                if labels:
                    entered_site = site
                    if addr in live:
                        for label in labels:
                            per_source_live[label].add(addr)
                    break
                if item_obs and const_site is None:
                    const_site = site
        if entered_site is not None:
            if addr in not_live:
                inert_entered.append((addr, entered_site))
            if entered_site.get("not_wired_tag"):
                disclosed_entered.append((addr, entered_site))
            if addr in live:
                live_entered.add(addr)
        elif observable:
            if addr in live:
                live_unentered.append((addr, sites[0]))
        elif const_site is not None:
            not_observable_const.append((addr, const_site))
        else:
            unobservable.append((addr, sites[0]))

    # Non-vacuity: the zero-valued buckets above are only meaningful if the
    # join actually looked at those addresses. Report the observable share so a
    # zero produced by a lookup miss cannot be read as a clean result. Observed
    # across the union - an address one ladder's binary cannot see may still be
    # compiled into another's. An item anchor counts through its references.
    def addr_observable(a: str) -> bool:
        for s in anchors[a]:
            if s["kind"] == "item":
                if item_exec[(s["path"], s["symbol"])][1]:
                    return True
            elif s["file"] in all_files:
                return True
        return False

    not_live_observable = sum(1 for a in not_live if a in anchors and addr_observable(a))

    return {
        "inert_entered": inert_entered,
        "disclosed_entered": disclosed_entered,
        "live_entered": live_entered,
        "live_unentered": live_unentered,
        "unobservable": unobservable,
        "not_observable_const": not_observable_const,
        "per_source_live": per_source_live,
        "not_live_observable": not_live_observable,
    }


# ---------------------------------------------------------------------------
# Self-test (--selftest)
#
# Unit-level control for the item-aware executed-verdict, following the
# catalog's own `--selftest` convention: a synthetic in-memory corpus plus a
# synthetic llvm-cov export driven through `load_coverage` and
# `join_anchor_coverage`, no repo state touched. The cases pin exactly the
# three shapes the naive next-function verdict got wrong: an executed
# attributable reference reads as executed, an unexecuted (or missing)
# reference reads as not-observable-const rather than entered OR never-entered,
# and a `NOT WIRED` const is never accused off a neighbouring function's
# coverage.
# ---------------------------------------------------------------------------

_SELFTEST_FILES = {
    "consts.rs": """\
/// PORT: FUN_8001d110
pub const USED_BY_EXEC: u32 = 1;

/// PORT: FUN_8001d120
/// NOT WIRED: nothing references this yet.
pub const DISCLOSED_CONST: u32 = 2;

/// PORT: FUN_8001d130
pub const REF_NOT_RUN: u32 = 3;

/// PORT: FUN_8001d160
pub const REF_QUAL: u32 = 4;

/// PORT: FUN_8001d170
pub const BARE_XFILE: u32 = 5;

/// PORT: FUN_8001d180
/// NOT WIRED: disclosed, yet executed code reads it.
pub const DISCLOSED_USED: u32 = 6;

/// PORT: FUN_8001d140
pub fn exec_fn() -> u32 {
    USED_BY_EXEC + DISCLOSED_USED
}

/// PORT: FUN_8001d150
pub fn cold_fn() -> u32 {
    REF_NOT_RUN
}
""",
    "user.rs": """\
pub fn qual_user() -> u32 {
    crate::consts::REF_QUAL + BARE_XFILE
}
""",
    "types.rs": """\
/// PORT: FUN_8001d190
pub struct ColdGauge {
    pub v: u32,
}

impl ColdGauge {
    pub fn tick(&mut self) {
        self.v += 1;
    }
}

/// PORT: FUN_8001d1a0
pub struct HotMeter {
    pub v: u32,
}

impl HotMeter {
    pub fn bump(&mut self) {
        self.v += 1;
    }
}
""",
}

# Which synthetic functions the synthetic ladder "executed".
_SELFTEST_EXECUTED = {"exec_fn", "qual_user", "bump"}


def run_selftest() -> int:
    import tempfile

    print("replay-port-coverage item-verdict self-test")
    catalog = load_catalog_module()
    src_dir = catalog.CRATES_DIR / "__selftest__" / "src"
    srcs = {
        src_dir / name: catalog.RustSource(src_dir / name, "__selftest__", text)
        for name, text in _SELFTEST_FILES.items()
    }
    anchors = catalog.collect_port_anchors(srcs)

    # Synthetic llvm-cov export: one function record per parsed fn, count 1
    # for the executed set, 0 otherwise. Line spans come from the parsed
    # source so the fixture cannot drift from its own line numbers.
    functions = []
    for path, src in srcs.items():
        for f in src.fns:
            hi = src.line_of(max(f.body_end - 1, f.body_start))
            functions.append(
                {
                    "filenames": [str(path)],
                    "regions": [
                        [f.line, 1, hi, 1, 1 if f.name in _SELFTEST_EXECUTED else 0, 0, 0, 0]
                    ],
                }
            )
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as tmp:
        json.dump({"data": [{"functions": functions}]}, tmp)
        tmp_path = Path(tmp.name)
    try:
        cov = load_coverage(tmp_path)
    finally:
        tmp_path.unlink()

    live = {
        "8001d110",
        "8001d130",
        "8001d140",
        "8001d150",
        "8001d160",
        "8001d170",
        "8001d190",
        "8001d1a0",
    }
    not_live = {"8001d120", "8001d180"}
    joined = join_anchor_coverage(
        catalog, anchors, srcs, [("selftest", cov)], live, not_live
    )

    def addrs(bucket: str) -> set[str]:
        val = joined[bucket]
        return set(val) if isinstance(val, set) else {a for a, _ in val}

    failures = 0

    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok    {name}")
        else:
            print(f"  FAIL  {name}{': ' + detail if detail else ''}")
            failures += 1

    check(
        "executed fn anchor is entered",
        "8001d140" in addrs("live_entered"),
        f"live_entered={addrs('live_entered')}",
    )
    check(
        "const referenced by an executed same-file fn is entered",
        "8001d110" in addrs("live_entered"),
    )
    check(
        "const referenced module-qualified by an executed cross-file fn is entered",
        "8001d160" in addrs("live_entered"),
    )
    check(
        "const whose only reference never ran is not-observable-const",
        "8001d130" in addrs("not_observable_const")
        and "8001d130" not in addrs("live_unentered")
        and "8001d130" not in addrs("live_entered"),
        f"const={addrs('not_observable_const')} unentered={addrs('live_unentered')}",
    )
    check(
        "cross-file bare-name reference does not count (strict rule mirrored)",
        "8001d170" in addrs("not_observable_const"),
    )
    check(
        "NOT WIRED const with no executed reference is never accused",
        "8001d120" in addrs("not_observable_const")
        and "8001d120" not in addrs("disclosed_entered")
        and "8001d120" not in addrs("inert_entered"),
        f"disclosed={addrs('disclosed_entered')} inert={addrs('inert_entered')}",
    )
    check(
        "NOT WIRED const an executed fn really references IS accused",
        "8001d180" in addrs("disclosed_entered")
        and "8001d180" in addrs("inert_entered"),
        f"disclosed={addrs('disclosed_entered')}",
    )
    check(
        "unexecuted fn anchor stays on the never-entered worklist",
        "8001d150" in addrs("live_unentered"),
    )
    check(
        "type anchor with no executed method is never-entered, not const",
        "8001d190" in addrs("live_unentered")
        and "8001d190" not in addrs("not_observable_const"),
    )
    check(
        "type anchor with an executed method is entered",
        "8001d1a0" in addrs("live_entered"),
    )
    check(
        "nothing in this corpus is unobservable",
        not addrs("unobservable"),
        f"unobservable={addrs('unobservable')}",
    )

    if failures:
        print(
            f"\nself-test: {failures} case(s) failed - the item-aware verdict "
            "is not trustworthy, so its report rows mean nothing"
        )
        return 2
    print("\nself-test: all cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--json",
        type=Path,
        action="append",
        dest="jsons",
        metavar="PATH",
        help=(
            "llvm-cov export for one ladder; repeat to union several. "
            f"Default: every {COV_GLOB} under target/ (the union)"
        ),
    )
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument(
        "--fail-on-disclosed",
        action="store_true",
        help="exit non-zero if a NOT WIRED anchor was executed by the run",
    )
    ap.add_argument(
        "--list-ladders",
        action="store_true",
        help="print `<test> <package>` per canonical ladder and exit (the "
        "export recipe reads this, so it cannot drift from the list)",
    )
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the item-verdict resolver self-test on a synthetic corpus "
        "and exit (no coverage data or repo state touched)",
    )
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    if args.list_ladders:
        for name, pkg in CANONICAL_LADDERS:
            print(name, pkg)
        return 0

    if args.jsons:
        inputs = args.jsons
        given = {p.stem.removeprefix("cov-") for p in inputs}
        missing_canonical = [n for n in CANONICAL_LADDER_NAMES if n not in given]
    else:
        inputs, missing_canonical = discover_inputs()
    # A missing input is a skip, not a failure - the whole invocation is
    # optional (see the module docstring). A *present* input with no function
    # regions is also a skip, because llvm-cov emits that shape for a build
    # that produced no instrumented objects.
    sources: list[tuple[str, dict[str, FileCoverage]]] = []
    for path in inputs:
        if not path.is_file():
            print(f"[skip] {path} missing - run cargo llvm-cov first")
            continue
        cov = load_coverage(path)
        if not cov:
            print(f"[skip] {path} carries no function regions")
            continue
        sources.append((path.stem, cov))
    if not sources:
        return 0

    catalog = load_catalog_module()
    srcs = catalog.load_rust_sources()
    anchors = catalog.collect_port_anchors(srcs)
    live, not_live = catalog_address_sets()

    joined = join_anchor_coverage(catalog, anchors, srcs, sources, live, not_live)
    inert_entered = joined["inert_entered"]
    disclosed_entered = joined["disclosed_entered"]
    live_entered = joined["live_entered"]
    live_unentered = joined["live_unentered"]
    unobservable = joined["unobservable"]
    not_observable_const = joined["not_observable_const"]
    # label -> the live addresses that source entered. Drives the per-source
    # contribution table: an address several ladders reach is credited to each,
    # and the `unique` column is what would be lost by dropping that ladder.
    per_source_live = joined["per_source_live"]
    not_live_observable = joined["not_live_observable"]

    lines: list[str] = []
    w = lines.append
    w("# Replay port-entry report")
    w("")
    w(
        "Runtime join: which `// PORT:`-tagged addresses a pad-driven ladder "
        "actually executed, against the static liveness verdict. See the "
        "script header for why the static number alone is an upper bound."
    )
    w("")
    w(f"- ported addresses with anchors: **{len(anchors)}**")
    w(f"- statically live: **{len(live)}**, of which entered by a run: **{len(live_entered)}**")
    w(f"- statically not-live: **{len(not_live)}**, of which entered anyway: **{len(inert_entered)}**")
    w(f"- `NOT WIRED`-disclosed anchors executed: **{len(disclosed_entered)}**")
    w(f"- not observable (const anchors, no executed reference): **{len(not_observable_const)}**")
    w(f"- not observable in any of these binaries (excluded above): **{len(unobservable)}**")
    w("")
    w(
        f"Non-vacuity: **{not_live_observable} of {len(not_live)}** not-live addresses "
        "have at least one anchor file present in the coverage data, so the "
        "inert-entered count is a measurement over that many addresses rather "
        "than a lookup miss."
    )
    w("")

    # Per-source contribution. `unique` is what dropping that ladder would
    # cost: a single-ladder join is not a smaller version of this measurement,
    # it is a different one, and the column says by how much.
    w("## Per-ladder contribution")
    w("")
    w(
        "Union across the sources below. These are separate sessions, not one "
        "continuous playthrough - what the union measures is \"some pad-driven "
        "ladder entered this\" against \"no pad-driven ladder did\". `unique` "
        "counts live addresses only that source entered."
    )
    w("")
    w("| source | live entered | unique to it |")
    w("|---|---|---|")
    for label, _ in sources:
        mine = per_source_live[label]
        others: set[str] = set()
        for other, _ in sources:
            if other != label:
                others |= per_source_live[other]
        w(f"| `{label}` | {len(mine)} | {len(mine - others)} |")
    w("")
    if missing_canonical:
        w(
            "**Partial union.** No per-ladder export was joined for "
            + ", ".join(f"`{n}`" for n in missing_canonical)
            + ". Every number above is about the ladders listed, not about the "
            "canonical set, and the live-unentered worklist below is "
            "correspondingly long. Re-export the missing ladder(s) before "
            "reading a row as work."
        )
        w("")

    def table(title: str, rows: list[tuple[str, dict]], note: str):
        w(f"## {title}")
        w("")
        w(note)
        w("")
        if not rows:
            w("_none_")
            w("")
            return
        w("| address | crate | symbol | site |")
        w("|---|---|---|---|")
        for addr, site in rows:
            w(
                f"| `{addr}` | {site['crate']} | `{site['symbol']}` | "
                f"{site['file']}:{site['line']} |"
            )
        w("")

    table(
        "Inert ports entered",
        inert_entered,
        "The static graph reports no host root reaches these, yet a ladder "
        "executed the anchor. Either the graph is wrong or the tag sits on the "
        "wrong symbol - each row is a finding, not a metric.",
    )
    table(
        "Disclosed `NOT WIRED` anchors executed",
        disclosed_entered,
        "The source disclaims these as unreached, and a **passing** oracle ran "
        "them anyway. Highest-priority rows: an oracle that traverses "
        "undisclosed-stub code can certify behaviour nothing implements.",
    )
    table(
        "Live but never entered",
        live_unentered,
        "Statically reachable, reached by none of the ladders above. Not "
        "defects - this is the wiring worklist ordered by what a playthrough "
        "actually needs, and it shrinks as the ladders reach further. Item "
        "anchors (const / static / type alias) never appear here - an item has "
        "no lines to enter, so its rows live in the bucket below instead.",
    )
    table(
        "Not observable (const anchors)",
        not_observable_const,
        "Item anchors whose executed verdict is reference-based (see the "
        "script header) and where no attributable referencing function "
        "executed. Not \"never entered\" - there is nothing to enter - and "
        "not a worklist: converting one of these means executing a function "
        "that *references* the item, at which point the row moves on its own.",
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n")

    print(f"sources               : {', '.join(label for label, _ in sources)}")
    print(f"ported anchors        : {len(anchors)}")
    print(f"live / entered        : {len(live)} / {len(live_entered)}")
    print(f"not-live / entered    : {len(not_live)} / {len(inert_entered)}")
    print(f"  (observable         : {not_live_observable} / {len(not_live)})")
    print(f"NOT WIRED executed    : {len(disclosed_entered)}")
    print(f"live never entered    : {len(live_unentered)}")
    print(f"not observable (const): {len(not_observable_const)}")
    print(f"not observable        : {len(unobservable)}")
    for label, _ in sources:
        mine = per_source_live[label]
        others = set()
        for other, _ in sources:
            if other != label:
                others |= per_source_live[other]
        print(f"  {label:<28}: {len(mine):>4} live entered, {len(mine - others):>4} unique")
    if missing_canonical:
        print(
            "PARTIAL UNION - no per-ladder export for: "
            + ", ".join(missing_canonical),
            file=sys.stderr,
        )
    # `relative_to` raises on an --out that is not under the repo (a relative
    # path resolved against a different cwd, /tmp, ...), which would throw away
    # the whole report after writing it. Fall back to the path as given.
    try:
        shown = args.out.resolve().relative_to(REPO)
    except ValueError:
        shown = args.out
    print(f"report -> {shown}")

    if args.fail_on_disclosed and disclosed_entered:
        print(
            f"\nFAIL: {len(disclosed_entered)} NOT WIRED anchor(s) executed by a "
            "passing replay.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
