# WAVE-6 PLAN (branch-local - DELETE before merge)

Branch `re-wave-6` off main @ `232afed2` (#306). Status: PROPOSED, awaiting review.
Nothing below is executed yet; this doc is the review artifact.

## Where things stand (deep-dive synthesis, verified against the tree)

- Waves 3-5 spent the static RE surface. Track 1 is in cleanup; the port
  worklist sits at 478 functions but the *question-level* residue is thin.
  The frontier is Track B breadth, and its next blocker is ORGANIC
  progression: three story-flag writers live in un-imported overlays.
- The capture harness is READY for the writer hunt: `lib/probe/watch.lua`
  is a first-class write-watchpoint layer (logs `tick,label,addr,pc,ra,value`
  + call context), and `autorun_flag_bank_watcher.lua` already implements the
  sharper pattern for flag writes (Exec-bp on setter `FUN_8003CE08` +
  `a0 == idx` filter, which isolates the exact flag AND names the caller
  `ra` directly - better than a raw byte watch that fires for all 8 flags
  sharing the byte).
- BUT: **no catalogued state brackets any of the three writes.** Both Zeto
  states are mid-battle (post-write); no dolk/dolk2 scenario exists at all;
  0x482 reads 0 across the entire library. Every capture must be
  manufactured by loading a chapter-1 in-game save from `saves/library/cards/`
  and playing forward. Probes must run `-interpreter -debugger` (Lua BPs
  don't fire under `--fast`) and the harness has no auto-quit for
  story-beat sessions - these are interactive, human-at-the-controls runs.
- The spine oracles cover `town01 -> map01 -> keikoku` + the boss leg
  `map01 -> rikuroa (Zeto) / dolk -> dolk2`. The map01 hub's full 0x3F
  destination set is already decoded: beyond the covered scenes the
  remaining hub legs are `cave01, vell, vozz, suimon, jou`.
- GAP-2 (op-0x44 SPAWN_RECORD gated to `opening_chain_active`) is scoped:
  a pure engine-ownership refactor of the single `cutscene_timeline`
  slot + its companion fields; no new disc RE required.
- Index drift found by the survey (cheap hygiene): the open-threads doc
  still carries a stale "roller config op" sub-thread Wave 5 closed, the
  slot-4 row's "open" status label contradicts its own resolved body, the
  debug-flags detail header still says "partial", and the three spine
  flag-writers + A7 exist only in the backlog (no threads-doc rows).

## Watchpoint targets (derived, ready to arm)

| Target | Address / method |
|---|---|
| `DAT_8007b7fc = 0x4B` (Zeto battle-id) | raw Write-watch `0x8007b7fc`, width 1 (widen if silent) |
| flag `0x142` (dolk clear) | Exec-bp `0x8003CE08` + `a0 == 322`; fallback Write-watch byte `0x80085780` bit `0x20` |
| flag `0x482` (mist walls) | Exec-bp `0x8003CE08` + `a0 == 1154`; fallback Write-watch byte `0x800857E8` bit `0x20` |

All three beats are sequential chapter-1 story events, so ONE play-forward
session with all watches armed simultaneously can bracket all three, and
each beat is also a chance to CATALOGUE the missing pre-write states
(pre-Zeto rikuroa, pre-dolk-clear, pre/post-mist) into `scenarios.toml` -
valuable library additions regardless of what the watches catch.

## Arc 1 - spine flag-writer hunt (CRITICAL PATH; capture is user-assisted)

1a. **Probe + runbook authoring (agent-doable, no emulator).** One
    `autorun_spine_flag_writers.lua` arming all three watches (template:
    `autorun_flag_bank_watcher.lua` + `watch.lua`), CSV + call-context
    output, wrap launch in `timeout --kill-after`. A short runbook doc
    (which card save to load, the beat order, what "caught" looks like)
    so the interactive session is mechanical.

1b. **Interactive capture session (NEEDS YOU at the controls).** Load the
    nearest chapter-1 card save, play keikoku -> rikuroa (Zeto trigger fires
    the 0x8007b7fc write) -> Zeto victory (0x1BE / possibly 0x142 beats) ->
    dolk clear -> the mist-wall story event (0x482). Save-state at each
    beat boundary; fingerprint + catalogue after. Fold the B9 spine
    eyeball checklist into the same session (you're already at the window):
    town01 NPC clips/parking, PSX-render overlay pass, dolk2 walk-in with
    0x142 forced.

1c. **Post-capture RE (agent-doable).** Attribute each caught `ra` by
    containment (`attribute_overlay_hits.py`); if the writer is in an
    un-imported overlay, run the static-overlay pipeline (`asset overlay
    generate/verify/extract/ghidra`, add the `static-overlays.toml` row),
    dump + decode the writer function, document in encounter.md /
    world-map.md / functions.md.

1d. **Engine: organic progression.** Replace the interim latches with the
    faithful mechanism the writer reveals: `arm_scripted_scene_boss`'s
    scene-entry latch (scene_entry.rs:34-62), the oracle's direct 0x142
    seed, and the 0x482 gate. Extend `chapter1_boss_spine_oracle` so the
    flags are set by PLAY (enter -> fight -> win -> walk out), not by test
    scaffolding. If a capture leg fails or a beat is unreachable in the
    session, the latch stays (it is faithful pre-beat) and the arc still
    lands 1a/1c for whatever was caught.

## Arc 2 - spine past dolk2 + Drake hub sweep (engine breadth, no capture)

- Decode dolk2's own 0x3F destinations (`scene_destinations` +
  `partition2_scene_changes` on its v12-embedded MAN, same method as the
  Wave-5 map01 decode) and drive the next interior leg into the oracle.
- Sweep the remaining map01 hub legs: walk-in oracle parts for
  `cave01, vell, vozz, suimon, jou` (portal-tile drive -> Field entry ->
  MAN partition shape assert, the Arc-2 Part B pattern). Expect more
  v12-embedded MAN scenes; the Wave-4 loader fix should cover them -
  any that fail become named findings, not blockers.
- Story-gate census on the new legs: which entrances carry C1/C2 flag
  gates (the 0x193/0x482 pattern), so the progression order is documented
  ahead of the writers landing.

## Arc 3 - GAP-2 multi-context spawned records (engine refactor; contingent)

Pull ONLY if an Arc-2 scene actually spawns a helper record mid-play
(the backlog's own gate). Scope (already mapped): replace
`World::cutscene_timeline: Option<CutsceneTimeline>` single-slot ownership
(state.rs:1511) with a spawned-record context that doesn't claim the
cutscene camera/locomotion lock or clobber `field_channels`; drop the
`opening_chain_active` gates in scene_entry.rs:893/1092; split
`install_cutscene_timeline_record` (narration.rs:400) so timeline-install
is separable from record-body install.

## Arc 4 - index hygiene + small static closes (Track C)

- Fix the three stale open-threads rows (roller sub-thread closed by Wave 5;
  slot-4 status label vs resolved body; debug-flags "partial" header) and
  add rows for the backlog-only threads (0x142 / 0x482 writers, A7 residue)
  so the two indices agree. Mirror site fragments.
- Static close: the battle-data-pack art-archive main-vs-base picker
  (`FUN_801F12D0` selection decode - currently pinned by exact cover only).
- Optional small port: tile-board per-cell tile-actor draw (RE done,
  engine draws nothing).

## Opportunistic (only if the Arc-1 emulator session is running anyway)

- A3: batch the ~13 remaining Super-Art queue captures through
  `autorun_super_art_action_queue.lua` (byte-exactness only).
- A7: the +0x16E bit 0x400 applier, if a status-inflicting fight happens
  to occur en route. Low value; do not detour for it.

## Deferred (explicitly not this wave)

- A5 summon render-mode live exerciser (validation-only; no exerciser hunt).
- scene-v12 b0 consumer (piggyback-only if a scene-load watchpoint is armed
  during Arc 1 anyway; not a goal).
- Inn costs / muscle-dome phase graph / dance banners (capture-leaning,
  no engine consumer pressure).

## Sequencing + gates

1a (probes) and Arc 2 and Arc 4 are parallel-agent-friendly (read-only recon
+ disjoint worktree scopes, Wave-3 recipe; re-verify every RESOLVED verdict
against the local dump before landing). 1b blocks on you; 1c/1d block on 1b.
Arc 2 does NOT block on Arc 1 - the interim latches keep the oracle green.

Gates per landed arc: full disc-gated workspace suite (never piped through
tail), clippy -D warnings + fmt, doc-density, site fragments mirrored +
`site/_gen.py` + check-site-links. No Sony bytes. Commits stay local
(no push) until review.
