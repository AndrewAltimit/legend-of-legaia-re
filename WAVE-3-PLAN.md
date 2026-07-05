# Wave 3 plan — next round of RE + engine work

**Operational planning doc, NOT reference material.** Lives on the `re-wave-3`
working branch only; delete before any merge to main (committed docs carry no
progress trackers). Companion to `~/Downloads/legaia-backlog.txt`, the
question-level index (`docs/reference/open-rev-eng-threads.md`), and the
port-catalog dashboard.

## LANDED (this session — parallel-subagent execution)

A 13-agent parallel wave ran against this plan (9 read-only RE investigations +
3 worktree code arcs + 2 worktree doc-application agents). Committed to
`re-wave-3`, local only. Headline outcomes:

- **Track A largely de-blocked statically (the mislabeled-static pattern held).**
  - **A7** is now ~fully static: complete status table, guard-passive
    auto-clear map, and the pinned `+0x16E`→command-arrow map (`0x08`=Left,
    `0x10`=Right, `0x20`=Up+Down, `0x38`=Attack-off, `0x1000`=Curse→Magic) that
    replaces the engine's reconstruction. Bytes 1/2 are cosmetic-only. The ONLY
    true remaining capture is the `+0x16E` bit `0x400` (Sleep/Numb-like
    guard-disable) applier. Corrections: Curse writes no `+0x21F`; Venom/Toxic
    are 9/10·7/10 roll debuffs, not DoT.
  - **record[0]+0x5C** — static negative close (rebased-at-load vestigial
    paired-relocation field, `FUN_80052FA0:561`, no reader). Off Track A.
  - **scene-v12** — staging hypothesis falsified (`FUN_8001F05C`/`FUN_8002541C`
    hold no v12 handler); b0 stays capture-blocked with a sharper target.
- **Static closes landed:** fishing reel buttons (Cross/Square, not Circle),
  Baka Fighter bindings + vestigial settle-timer, tile-board (always
  procedural + header/flag/render model), `DAT_8007C018[5..N]` populate +
  `attr` render-unused, effect-id→move join design, C1 straggler `801E1FB0`
  (false positive — intra-function label). Port-worklist honesty pass: worklist
  486→479 (7 host-emission helpers ignore-listed); only 2 genuine gaps
  (`801d688c` menu cursor-nav, `80024e08` set-model stub).
- **Engine arcs:** Arc 3 screen-space POLY_FT4/OT overlay pass in engine-render
  (GPU-verified, afterimage/billboard wired); B6 halt-acquire handshake +
  faithful per-leg NPC glide (22/25 town01 placements now disc-derived);
  level-up battle-actor `+0x14C..+0x176` field map + growth-port disc test.
- **Arc 1 (chapter spine)** — recon/blueprint only (deferred implementation to
  avoid clobbering the in-flight engine-core worktrees): chain is
  `town01 → [garmel] → map01 hub → {rikuroa|keikoku} → dolk` (boss Zeto); the
  one hard wall is GAP 1 (unconsumed `WorldMapTransition` + no
  `target_map→CDNAME` resolver). This is the next arc to implement.

Still genuinely capture-blocked (real Track-A residue): the `+0x16E 0x400`
applier, scene-v12 b0, the 13 Super connectors (low value), summon render-mode
live seat (no exerciser). Effect-index CLI/test = documented, ready to
implement.

## Ground truth as of main @ a32d6749 (#303)

Recon verified against the tree (two independent sweeps, 2026-07-05):

- The backlog file is **already consistent with #302/#303** — its Wave-2 and
  C6 sections were rewritten after both merges. Only its header ("post
  #286-#301") lags. Three line-item corrections worth stamping into it:
  - **A6 (ACE key-item consumer hunt): RESOLVED, negative.** All key-item-range
    readers mask `& 0x3ff` into 256-entry tables; no unguarded-index amplifier.
    Read-BP probe deprioritized. Delete the item.
  - **C1 (dump worklist): effectively CLOSED.** 3,575 dumps local; of 771
    unique `FUN_` cites across docs/, only `801E1FB0` (cutscene colour-fade op
    0x34 sub-0) is a real missing dump. One-off backfill, then delete the item
    as a recurring chore (keep the regex check in CI habit).
  - **A5 (summon render-mode live seat): reconfirmed blocked.** The test file
    exists and landed its finding — no catalogued state seats a live
    0x4000/0x4001 node; needs a frame-stepped *enemy* Sim-Seru cast that no
    catalogued save reaches. Keep, but mark "no known exerciser" so nobody
    burns a session hunting for one.
- Port-catalog global: 1,665 dumped / 1,513 documented / 267 ported /
  486 on the worklist. `boot-toc`, `cd-io`, `field-load`, `asset-loader` are
  effectively finished features; the 200+-missing counts in
  battle-action/cutscene/minigames are dominated by ~10 shared *host-emission*
  helpers (sprite/panel emitters, SFX cue enqueue, cursor nav) that repeat in
  every feature view.

## The genuinely-open set (verified, deduped)

**Capture-blocked (Track A; probe harness mature):**
1. **A7 — status bytes 1/2 (Toxic/Numb) mechanical arm** + which `+0x16E` limb
   bit grays which command arrow. Highest-value open RE question: the engine's
   Left/Right/Down arrow-graying is a reconstruction.
2. **record[0]+0x5C consumer** — read-watchpoint on the zero word
   (`clut_a_off − 4`); no traced reader. (Moved out of C6 by #303.)
3. **scene-v12 staging site + header b0** — scene-load write-watchpoint on the
   v12 malloc buffer; static half first: dump lead `FUN_8002541C`.
4. A3 — 13 remaining Super replace-strings (one capture each; byte-exactness
   only, low value; batchable).
5. map03 full-VRAM-oracle residual (needs a map03-WorldMap-resident mednafen
   state; low).
6. muscle-dome `ctx+6` full phase graph; inn menu-overlay DATA (costs).

**Static / dump-doable (Track C; no emulator):**
7. Fishing reel-button bit assignment in `_DAT_8007b850` — likely now
   *derivable* from the packed pad-layout builder `FUN_8001822C`
   (boot.md § pad layout); doc still lists it open.
8. Baka Fighter: pad-button→attack-type binding; settle-timer seeder;
   action-vs-display anim indexing.
9. Tile-board: fixed vs procedural boards (disc scan of inline op-0x49 cell
   arrays); event-cell `+7`/`+9` flag operands.
10. World-map-overlay: which call populates the kingdom-derived
    `DAT_8007C018[5..N]` entries; slot-4 per-vertex `attr` non-render consumer
    (corpus read sweep; low).
11. Effect-id → triggering-move join off move-power (disc-gated; feeds docs +
    site enemy tables).
12. `801E1FB0` dump backfill (the one real C1 straggler).
13. Level-up residue: engine port of the SCUS growth-curve tables into
    `seru_stats.rs`; document the battle-actor `+0x14C..+0x176` field map
    (the struct #302 disentangled from the char record).

**Engine-side, no RE blocker (Track B):**
14. **Screen-space 2D overlay pass** in engine-render (POLY_FT4 + ordering-table
    semantics). This single capability unlocks four parked, already-ported
    modules: `afterimage` + `billboard` (authored-but-unwired), the
    `screen_fx` widget family (iris/letterbox/panel/sprite — ending scenes +
    field-VM op 0x43), and tile-board per-cell tile-actor rendering ("engine
    draws nothing yet").
15. **B6 residue:** (a) halt-acquire / CFLAG_TST handshake — timeline currently
    steps past cross-context flag-waits by width instead of parking; engine
    modelling fix, no capture. (b) Faithful NPC glide — read the real per-leg
    motion-op operand base steps from MAN bytes instead of
    `FIELD_NPC_MOTION_SPEED = 8`. (c) Scripted initial facing stays blocked
    structurally (needs the `FUN_8001B47C`→`FUN_80029888` GTE matrix path in
    the render side; fold into item 14's neighborhood or defer).
16. **Persistent playable game** — the backlog's stated frontier. The slice
    covers boot → prologue → Rim Elm free-roam → battles → menus → save; what
    it lacks is a *story spine*: flag-gated progression across scene chains
    beyond the opening, validated end-to-end.
17. Port-worklist honesty pass: adjudicate the ~10 top-cited shared
    host-emission helpers (port / tag the existing engine-idiom equivalent /
    move to ignore list) so the 486 number stops overstating open work.

**Human-only (B9):** four eyeball passes at the window (NPC clips/facing in
town01; blend modes under `LEGAIA_PSX_RENDER=1`; opening chain end-to-end;
free-roam NPC mis-park check with `animate_field_npcs` on).

## Proposed Wave 3 (three arcs + one batch, in priority order)

### Arc 1 — headline (Track B): "Chapter 1 spine" playable-progression arc
Extend the faithful opening (prologue → Rim Elm, #296–#301) into the first
real story chapter: story-flag-gated scene progression out of Rim Elm through
the first dungeon beat, driven by the real field VM + walk-on
trigger/transition machinery that already exists. Deliverables:
- A disc-gated **chapter oracle** in the style of `mode_trace`/opening oracles:
  scripted pad trace drives cold boot → prologue → Rim Elm → first exit →
  next scene chain; asserts scene names, game modes, story-flag writes against
  the PCSX playthrough anchors (s1..s5) + memory-card library states.
- Whatever the trace flushes out gets fixed in the same arc (the pattern from
  #277: the oracle finds the gaps, the arc closes them).
- Scope guard: stop at the first boss-battle hand-off; battles themselves are
  already covered.

### Arc 2 — Track A batch: one emulator session, three probes
Amortize the PCSX-Redux setup across the genuinely capture-blocked residue,
highest value first:
1. **A7**: before/after capture around byte-1/byte-2 attacks → the mechanical
   arm; plus the `+0x16E` limb-bit → arrow assignment (menu render trace).
2. **record[0]+0x5C**: read-watchpoint across a battle load + art playback.
3. **scene-v12**: `FUN_8002541C` Ghidra dump *first* (static, may dissolve
   it — the mislabeled-static pattern), then the write-watchpoint on the v12
   malloc buffer during a scene load; b0 hypothesis check rides along.
Optional tail if the session is going well: 2–3 of the 13 Super connector
captures. Every run wrapped in `timeout --kill-after`; grep `funcs/` for each
target global before authoring (standing rule).

### Arc 3 — Track B render capability: screen-space overlay pass
One PR: a 2D screen-space POLY_FT4/OT pass in engine-render, then wire the
four parked consumers (afterimage, billboard, screen_fx widgets, tile-board
cell actors). Each wire gets its own small oracle/test; the screen_fx one can
byte-pin against the ending-scene/op-0x43 disc records already parsed.
This also retires the "authored-but-unwired" annotations from #302/#303.

### Batch 4 — parallel static closes (read-only agents, central writes)
The Wave-2 recipe (worktree isolation not needed — read-only recon, findings
returned, I write the shared index files centrally): items 7–13 + 17 above.
Cheap, independent, and each is a one-session close. Fold the B6 halt-acquire
handshake (15a) and NPC glide operands (15b) in as the two engine-code
members of the batch, each with its disc-gated test.

### Explicitly NOT this wave
- A5 (no exerciser exists), A3 beyond the opportunistic tail (low value),
  map03 VRAM residual, inn costs (needs its own overlay capture — ride a
  future Track A session), slot-4 `attr` deep hunt (render-unused; low).
- B9 eyeball passes — those are yours; the list is above whenever you have
  minutes at the window.

## Process guardrails (carried from Wave 1/2 lessons)
- Recon/adjudication agents run read-only; shared index files
  (open-rev-eng-threads.md, functions.md) are written by one owner.
- Re-verify every subagent RESOLVED/DELETE verdict against the local dump.
- Full disc-gated suite to green after each arc; re-run after each fix (first
  failing binary masks later regressions).
- Docs↔site mirroring per PR (`git diff main...HEAD --stat -- docs/ site/`),
  doc-density + site-link checks.
- Work stays local; no pushes.
