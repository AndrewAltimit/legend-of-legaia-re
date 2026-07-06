# Wave 8 — RE + engine plan (branch `re-wave-8` off main @ `28ae1045`)

## Context

Wave 7 (#308) closed the static side of the spine flag-writer hunt (corpus-wide negative — capture is provably the only closer), pinned the second motion VM, landed multi-context helper timelines, and pushed hub depth through vozz/jou — surfacing one new engine-only gap: the player-channel (`0xF8`) ExecMove/HaltAcquire handshake never completes, so driven door-cutscene hops (jou→jouina) force-complete without their trailing `0x3F`. Track 1 (preservation) is ~91% and in cleanup; the frontier is Track B engine breadth plus the capture-gated spine writers.

User decisions for this wave: **partial capture session (Zeto + 0x142; 0x482 stays parked)** + all four optional arcs. Branch off main, commit locally, **no push**.

## Mechanics

- Branch `re-wave-8` off `main` (`28ae1045`). `WAVE-8-PLAN.md` committed branch-local; delete before merge (own commit).
- Parallel-subagent recipe (validated waves 3–7): worktree per implementation arc with **disjoint file scopes**, cherry-pick back; read-only recon agents return findings; **one owner (main session)** writes shared index files (`docs/reference/open-rev-eng-threads.md`, `docs/reference/functions.md`, `site/_content/`). Worktree agents must symlink `extracted/` + `saves/` in, or disc-gated tests silently skip. Re-verify every subagent RESOLVED/PORT claim against local dumps before landing.
- Gates per land + at the end: `cargo fmt --all -- --check`, `cargo clippy --all-targets --workspace -- -D warnings`, full disc-gated suite (`~/.bashrc` exports `LEGAIA_DISC_BIN`; never pipe test output — redirect to file and check exit code). Docs changes mirror to `site/_content/` + `python3 site/_gen.py` + `check-site-links.py` + `check-doc-density.py`.

## Arc 0 — Spine flag-writer capture, partial (HUMAN session + agent pre/post)

The critical-path unlock. Harness is built: `scripts/pcsx-redux/autorun_spine_flag_writers.lua` + `run_probe.sh` + runbook `docs/tooling/spine-flag-writers-capture.md`.

- **Agent pre-work**: verify probe + card fingerprints (`scripts/manage-states.py fingerprint`; confirm by fingerprint, not slot number), stage exact launch command lines (wrapped in `timeout --kill-after`; interpreter+debugger, never `--fast`) and hand them to the user.
- **Human session** (two legs, both bracketed by `saves/library/cards/playthrough-ladder-pro00-14.mcr`):
  - Zeto: load PRO-01 (fallback PRO-05), walk into Mt. Rikuroa; write-watch `0x8007b7fc` names the PC.
  - `0x142` dolk-clear: load PRO-00, play the first dolk visit to its clear beat; exec-bp `0x8003CE08` with `a0==322`.
  - Operator also fingerprints + catalogues fresh pre-write states at each boundary (per runbook) — fixes the "no state brackets these beats" library hole.
- **Agent post-capture** (per caught `ra`): `attribute_overlay_hits.py` by containment → `asset overlay generate/verify/extract/ghidra` + `crates/asset/data/static-overlays.toml` row → dump + decode the writer → port organic progression. The Zeto port replaces the interim latch `SCRIPTED_SCENE_BOSSES` (`crates/engine-core/src/world/encounters.rs:25`, consumer `scene_entry.rs:34-62`); the `0x142` port makes the dolk→dolk2 conditional entrance organic. Update `open-rev-eng-threads.md` rows; `0x482` row stays open (explicitly parked).
- Runs **in parallel** with the agent arcs; if the session slips, everything else still lands and the post-capture work moves to wave 9.

## Arc 1 — B-player-channel: 0xF8 ExecMove/HaltAcquire completion model (engine-only)

Highest-leverage agent-doable unblock. Root cause fully mapped:
- `field_channels::resolve_target` (`crates/engine-core/src/field_channels.rs:137-144`) returns `None` for `0xF8`, so in `run_spawned_record_slice` (`crates/engine-core/src/world/narration.rs:817-868`) a `C3 F8 …` HaltAcquire falls through to the timeline's own ctx and yield-loops `pc 0x50→0x60` until the 1200-frame cap (`finish_cutscene_timeline_frame`, `narration.rs:1046-1053`) force-completes without the trailing `0x3F`.
- **Fix (minimal, faithful)**: special-case `0xF8`-targeted ops in `run_spawned_record_slice`. `A2 F8 <id>` ExecMove → drive the player via the existing plumbing (`player_actor_slot` + `move_state`; reuse the `WalkTouchEvent::PlayerMoveTo` snap model from `man_field_scripts/npc_motion.rs:197-227` / `field_movement.rs:623-635`) and mark a player-move in flight. `C3 F8 …` HaltAcquire → complete when the player-move is done (or a small park budget mirroring `CHANNEL_WAIT_PARK_TIMEOUT`, `config.rs:377`) and step **past** by width instead of taking the backward `resume_pc`. The designed seam is `field_halt_acquire_apply` (`crates/engine-vm/src/field/host.rs:2080-2088`, currently a default no-op).
- **Oracle**: extend `crates/engine-shell/tests/chapter1_hub_depth_oracle.rs` `part_j` (`:553-606`) to assert the previously-unreachable `SceneEntered("jouina")`, plus a unit test that a `0xF8` HaltAcquire no longer yield-loops. Keep the gate-both-ways assertions intact.

## Arc 2 — Hub breadth: cave01 / vell / suimon depth + jouina→jouinb (depends on Arc 1 for the jou leg)

Extend the proven oracle pattern (`chapter1_hub_sweep_oracle.rs` leg driver + `chapter1_hub_depth_oracle.rs` helpers `drive_town01_to_map01` `:87`, `find_portal_tile` `:111`, `scene_dest_names` `:155`, `p2_scene_changes` `:168`):
- Depth-drive the three ungated legs (cave01 `[1,13,18]`, vell `[8,17,13]`, suimon `[10,7,3]`): decode each interior's `0x3F` destination set, gate census (C1/C2), tile triggers; drive at least one interior transition per leg in-engine.
- `jouinb` (index 664, currently untouched): load + shape/decode assertions; after Arc 1 lands, drive jou→jouina→jouinb end-to-end.
- New/extended disc-gated test(s) in `crates/engine-shell/tests/` following the existing naming; document new gates/beat idioms in the world-map/field docs + open-threads.

## Arc 3 — op-0x49 framed window census (Track C, cheap static)

The last static shot before fully conceding the spine captures: a framed op-0x49 flag-window census with the real `field_disasm` walker (base+offset near-miss windows containing `0x142`/`0x482`), following the census pattern in `crates/engine-core/src/man_field_scripts/partitions.rs:404-418` (`motion_flag_census`) + its disc anchor test. Expected negative — record the result either way in open-threads; a negative is a real finding (do not re-run afterwards).

## Arc 4 — Shared-blocker ports (port-catalog leverage)

From `target/port-catalog/open-work.md`: `80024e80` (89 refs), `801d63b0` (43), `801d58f0` (41) dominate nearly every feature's missing list. Recon first (dump + classify each — some may be PsyQ infra → `port-catalog-ignore.toml` instead of a port), then port 1–3 with `// PORT:` tags and unit tests. Also close the two provenance gaps (`801da1b8`, `801da200` ported-but-not-dumped: add to `ghidra/scripts/dump_funcs.py` TARGETS and dump). Success = the dashboard's worklist number drops and per-feature bars lift; regenerate with `scripts/ci/port-catalog.py --dashboard`.

## Arc 5 — NPC initial facings (field fidelity)

`docs/subsystems/field-locomotion.md` open item: never-walked NPCs render unrotated because per-actor field-VM channel execution doesn't apply initial facings. RE the retail source of the initial facing (MAN placement heading vs first channel op), port into the actor spawn path in engine-core, and verify with a town01/town-scene screenshot pass (`project_engine_screenshot_capture` recipe). Visible fidelity win in every town.

## Arc 6 — scene-v12 reader dump (static half only)

Ghidra-dump the undumped `~0x800219xx` reader of `_DAT_8007b85c` (the v12 record-table consumer lead): locate precisely via `find_lui_writers.py`-style sweep over SCUS + resident code, add to `dump_funcs.py` TARGETS, dump, and decode as far as statics allow. The scene-load write-watchpoint half stays parked (not in this wave's capture session). Update `docs/formats/scene-v12-table.md` Open section with whatever the dump settles.

## Arc 7 — Inn/shop/menu residue (small closes)

From docs `## Open` sections, opportunistic and low-risk:
- `docs/subsystems/shop.md`: write up the Buy/Sell/Quit mode-select panel (`FUN_801d4868`) layout from the existing overlay dump.
- `docs/subsystems/level-up.md`: fix the `stat_cap` accessor reading `+0x11A` vs the documented `+0x120` u16 (rename/relocate in `crates/save` + tests).
- `docs/subsystems/inn.md`: static hunt for per-scene inn costs in the shop/menu overlay DATA (dump-side only; if capture-blocked, record that and stop).

## Close-out

1. Docs + site reconcile (single owner): open-threads rows (close/park per arc), functions.md for new dumps, site fragments mirrored, `_gen.py` + link/density checks.
2. Full disc-gated suite green (wave-7 baseline 4596/0); re-run after each cherry-pick fix.
3. Delete `WAVE-8-PLAN.md` (own commit). Leave branch local — no push.
4. Update agent memory: `project_re_wave_8_plan` (+ point wave-7 note at it); refresh the backlog expectations only in memory — the user's `~/Downloads/legaia-backlog.txt` is theirs to rewrite.

## Verification

- Per-arc: each arc lands with its own disc-gated oracle or unit tests (named above), plus fmt/clippy.
- End-to-end: full `cargo test --workspace --release` disc-gated run (output to file, check exit code); `chapter1_hub_depth_oracle` drives jou→jouina (and →jouinb) for real; port-catalog dashboard regenerated; screenshot pass for Arc 5.
- Capture verification: caught `ra` values attributed by containment; decoded writers must explain the exact observed flag values before replacing any interim latch.
