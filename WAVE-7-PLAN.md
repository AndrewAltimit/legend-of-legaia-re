# Wave 7 — RE + engine round (branch `re-wave-7` off `main`, commit-only, NO push)

## Context

Wave 6 (#307, merged; suite baseline 4574/0) left the spine flag-writer capture session pending plus a set of agent-doable arcs. Per review: **no interactive capture this wave**. The user surfaced a lead — retail's debug menu can trigger STR cutscenes which teleport the player and set the beat's event flags — and a verification sweep against the dumped corpus refined it:

- The FMV dispatch record is playback-only (`crates/asset/src/fmv_dispatch.rs:13-22`) — **no per-FMV teleport/flag table exists**; the dev-menu renderer `FUN_801EAD98` is display-only and the controllers carry no FMV/flag dispatch. The code that applies the post-STR teleport+flags is in **un-dumped** code (do not re-walk the "per-FMV event table" shape).
- Nothing in the dumped corpus loads 322 (0x142) or 1154 (0x482) into flag-setter `FUN_8003CE08` by ANY addressing mode — but the **op-7 precedent** (motion-VM `FUN_80038158` op-7 sets flag 549 inline, invisible to the MAN 0x50/0x60/0x70 census; `docs/subsystems/world-map.md:281-289`) shows a whole flag-set op family the existing census never covered.

So Wave 7 = a capture-free static hunt for the spine writers along the two honest leads (op-family census extension + targeted un-dumped-code recovery), plus the four approved engine arcs, run as parallel worktree agents with disjoint file scopes (the Wave-3 recipe).

## Arc 1 — Spine-writer static hunt v2 (headline RE; disc + Ghidra only, no capture)

**1a. Flag-setter census extension (disc-side).** Extend the field-VM/motion-VM disassembly census beyond MAN `0x50/0x60/0x70` to every op family that can write the system flag bank (motion-VM op-7/op-8 per the 549 precedent; actor-VM + timeline SET ops). Sweep every scene MAN partition + v12 prescript on the disc for operands `0x142`, `0x482`, `0x1BE`, and any `DAT_8007b7fc`-analogue battle-id write. Build on `crates/asset` field_disasm + `man_field_scripts` (system-flag census machinery already exists — `--system-flag-census`). Hit ⇒ pin writer scene+record, document, and port organically (Arc 1c).

**1b. Un-dumped-code recovery (Ghidra-side).** Two targets, both findable statically:
   - The **post-battle victory / boss-defeat teardown** band in the battle overlay (0898) — the doc-suspected `0x142` writer site (`docs/subsystems/world-map.md:261`). Dump the victory chain from the battle-action SM exit states; look for flag-set calls with struct/table-driven `a0`.
   - The **debug-menu FMV trigger path**: the corpus's nine FMV states were "debug-menu-driven" but no dumped code writes `_DAT_8007BA78` outside the three known field-VM/title sites. Signature byte-search across the PROT corpus (`asset overlay scan/find-sig`) to locate the un-imported menu code, add a `static-overlays.toml` row, dump + decode. This also explains the user-observed teleport+flag application (scene-restore list at `0x801CE8AC` + whatever applies flags).

**1c. Engine port (contingent on a hit).** Replace the corresponding interim latch with the real writer: drop/reduce `SCRIPTED_SCENE_BOSSES` + `arm_scripted_scene_boss` (`crates/engine-core/src/world/encounters.rs:25`, `scene/host/scene_entry.rs:34-62`) for Zeto if its trigger is found; supply the `0x142`/`0x482` setters so the existing `ConditionalDest` + mist-wall gate plumbing fires organically. Disc-gated oracle asserts organic flag application (no seeded flags).

**1d. Index/docs.** Record the debug-STR observation + the falsified per-FMV-event-table shape in `open-rev-eng-threads.md`; update the three spine-writer rows with the static-hunt outcome (closed, or "static leads exhausted → capture remains the closer"). Site fragments mirrored.

## Arc 2 — GAP-2 multi-context timeline decoupling (engine-only)

Decouple "spawned P2 record" from THE single cutscene timeline so op-0x44 SPAWN_RECORD works mid-play, not just under `opening_chain_active`.
- `World::cutscene_timeline: Option<CutsceneTimeline>` (`crates/engine-core/src/world/state.rs:1527`) → multi-context container; companions `in_cutscene_timeline` (:1541), single-slot `pending_record_spawn` (:1641), `opening_chain_active` (:1651), `entering_town01_opening` (:1602).
- Un-scope guards in `crates/engine-core/src/scene/host/scene_entry.rs:1083-1106` (op-0x44), `:892-927` (arrival spawn), `:995+` (walk-on dispatch); split `install_cutscene_timeline_record`.
- Retail reference: per-context `ctx[+0x90]/+0x9e/+0x10|=0x100` set by `FUN_8003BDE0` (`docs/reference/functions.md:150`).
- Guard-rails: opening-chain e2e, town01 dinner one-pass rule, chapter-1 spine oracles stay green; new unit tests for concurrent contexts.

## Arc 3 — Hub-leg depth: vozz + jou (disc-gated oracles)

- vozz is story-load-bearing (vozz P1[7] sets Ravine gate 0x193 — `open-rev-eng-threads.md:586`). Decode vozz's interior 0x3F chain + gate census; drive map01→vozz→(interior hop) in a new oracle.
- jou → jouina hop (destination already pinned by hub sweep Part B).
- Reuse `crates/engine-shell/tests/chapter1_hub_sweep_oracle.rs` helpers: `drive_town01_to_map01` (:88), `find_portal_tile` (:120), `scene_dest_names` (:159), plus `partition2_scene_changes` / `overworld_portal_sites` / `partition2_record_gates`.

## Arc 4 — Tile-board visual draw (B9 renderer gap)

Engine state + draw list exist (`tile_board_draw_list`, rebuilt by `refresh_tile_board_draw_list`, `world/frame_tick.rs:484-531`). Remaining: shell/render consumes the list and draws tile actors per frame (same path NPC actors already draw). Verify via offscreen screenshot capture of a tile-board scene; user eyeball deferred to their next window session.

## Arc 5 — A3 Super-Art batch (opportunistic, timeboxed)

13 remaining Super-Art replace-string captures via `autorun_super_art_action_queue.lua`. Attempt agent-side: fixed battle sstate + `probe.pad_force` deterministic inputs (record/replay rig exists). If battle-menu nav proves flaky (it has before), stop and leave a ready-to-run manifest for a short user-driven batch. Byte-exactness only — lowest priority; cut first if the wave runs long.

## Wave mechanics (Wave-3 recipe)

- Branch `re-wave-7` off `main` (@523c73f0). `WAVE-7-PLAN.md` committed branch-local (delete before merge). **NO push.**
- Parallel worktree agents, disjoint file scopes; ONE owner writes shared index files (`open-rev-eng-threads.md`, `functions.md`).
- Re-verify every subagent RESOLVED/DELETE/PORT verdict against local dumps before landing.
- Docs changes mirror `site/_content/` fragments; `python3 site/_gen.py`; check-site-links.py + check-doc-density.py.
- Gates: `cargo fmt --all -- --check`; `cargo clippy --all-targets --workspace -- -D warnings`; full disc-gated suite (`cargo test --workspace --release`, LEGAIA_DISC_BIN from ~/.bashrc), re-run after each fix; never pipe test output through `tail`.
- Ghidra scripts ASCII-only, `# @runtime Jython`; new dumps registered in `dump_funcs.py`; notable entries into `docs/reference/functions.md`.

## Outcome (wave wrap)

- Arc 1a: motion-VM carrier pinned (MAN tail-section 1) + disc-wide census; spine flags NEGATIVE; 549 op-7 carrier FALSIFIED.
- Arc 1b: corpus-wide static negative for all three spine writers (runtime-computed); debug-menu teleport+flags mechanism = 0897 warp appliers + EVENT FLAG editor; new op 0x4C 0xD3 timed-flag scheduler; no new overlay row needed. Capture harness remains the closer.
- Arc 1c NOT triggered (no static writer hit - by design contingent).
- Arc 2: helper contexts landed. Arc 3: vozz/jou depth oracles landed (player-channel 0xF8 gap pinned as a new open thread). Arc 4: tile-board draw landed (retail-unused census).
- Arc 5: 0/14 captured (arts-input save states physically too short for any Super; pad-override segfault); replay/observer probe + 14 derived input strings + human-batch manifest delivered.
- Full disc-gated suite: 4596 passed / 0 failed (baseline 4574).

## Verification

- Full disc-gated suite green (baseline 4574 pass / 0 fail).
- Arc 1: census tool output committed as a doc/threads update with dump citations; any writer close carries a disc-gated oracle asserting organic flag application; falsified shapes recorded so they aren't re-walked.
- Arc 2: existing opening/spine/dinner oracles green + concurrent-context unit tests.
- Arc 3: new oracle tests assert scene load + gate census per hop.
- Arc 4: offscreen screenshot shows tile actors drawn at cell centres.
- Arc 5: each captured Super logged byte-exact vs `art` crate matcher, or a fallback manifest committed.
