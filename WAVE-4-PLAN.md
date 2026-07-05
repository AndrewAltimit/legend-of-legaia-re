# WAVE-4 PLAN (branch-local, delete before merge)

Proposed next round of RE + engine work, off `main` @ `0e0d412d` (#304).
Written after a verification pass over the post-#304 backlog
(`~/Downloads/legaia-backlog.txt`), the live indices, and the code itself -
per the backlog's own ground-truth rule. Two backlog items were found stale
and one "fresh RE thread" turned out to be mostly static; the corrections
below reshape the wave.

---

## 1. Verified open-set (backlog corrections first)

### 1a. GAP 6 is REAL but misdiagnosed - it is a bundle-LAYOUT gap, not an offset gap

Backlog claim: "rikuroa/dolk2/dolk bundles use EXTENDED-FOOTPRINT offsets that
`scene_asset_table::detect` misses - the dungeon-leg loader must use the
extended-offset table." **Stale on both halves:**

- `detect` already tolerates extended-footprint offsets
  (`crates/asset/src/scene_asset_table.rs:373`, test
  `accepts_extended_footprint_offset_past_indexed_size` at `:524`), and the
  engine loader already feeds extended bytes
  (`crates/engine-core/src/scene/prot_index.rs:168` `entry_bytes_extended`,
  used throughout `scene_ty.rs` + `scene_bundle.rs`).
- `dolk` loads fine TODAY (disc-verified this session: 44 triggers, 8 portal
  sites, MAN decodes).
- The REAL gap: `rikuroa` and `dolk2` have **no detectable asset-table entry
  at all**. Their first slot classifies as `SceneEventScripts` (the
  v12-family prescript layout), and neither `scene_asset_table::detect` nor
  `scene_scripted_asset_table::detect` hits ANY entry in either scene, so
  `find_bundle` -> `None` -> `field_man_payload` -> `None`. Disc-verified:
  - rikuroa entries 155..164: `SceneEventScripts(555008)`, `LzsContainer`,
    `DataFieldTruncated`, 5x `SceneTmdStream`, `MostlyZeros`, `SceneV12Table`.
  - dolk2 entries 68..76: `SceneEventScripts(471040)`, `LzsContainer`,
    `DataFieldStreaming`, `SceneTmdStream`, `TmdSizePrefix`, 2x `PochiFiller`,
    `MostlyZeros`, `SceneV12Table`.
- New RE question (Arc 2): **where does the MAN live in a v12-family dungeon
  bundle?** Candidates: interior of the `SceneEventScripts` entry at a
  non-zero offset, or a descriptor of the LZS/DataField sibling. This joins
  the existing scene-v12 consumer thread (open-rev-eng-threads.md) - and may
  finally give that thread a consumer question with an engine payoff.

### 1b. B-overworld-gate is mostly STATIC, not a capture thread

The backlog calls the overworld progress-flag map "UN-RE'd". A one-session
disc probe (temp test, not committed; log in session scratchpad) shows the
gating is right there in the map01 MAN partition-2 C1/C2 condition lists,
decodable with EXISTING APIs (`partition2_record_gates`):

- All 24 map01 portal sites resolve; the five keikoku portal records
  (P2[21]/[23]/[25]/[27]) carry **C1=[0x193]** (one-shot latch: the portal
  record stops spawning once 0x193 is set - the post-Ravine state change).
- The mist-wall force-walk bands are plain gate-1 beat records with C1
  latches: **P2[34]/[35]/[36] C1=[0x482]** (the band across the map interior)
  and **P2[9] C1=[0x2FC]** (next to the cave01 portal at (37,109..110)).
- rikuroa/dolk/dolk2/bylon/jou/etc. portal records are C1=C2=[] -
  unconditional; retail ordering emerges from the beat-band records blocking
  the paths, not from per-portal gates.

So the actual remaining RE question is only: **which scene/record SETS
0x193 / 0x482 / 0x2FC (and town01's 549/550/551)?** That is a disc-wide
static census once the tooling gap below is closed. "Capture-blocked labels
rot" strikes again - grep the disc first.

### 1c. The tooling gap is tiny: the gflag walker ignores SystemFlag ops

`field_disasm` ALREADY decodes the 0x50..0x7F system-flag ops as
`InsnInfo::SystemFlag` (`crates/asset/src/field_disasm/decode.rs:442-475`).
`walk_partition_gflag_sites` (`crates/engine-core/src/man_field_scripts/`
`partitions.rs:227`) just never matches that variant - it only surfaces the
scratchpad 0x2E/0x2F `GFlag` ops. Flag 551 lives in the SYSTEM bank, so the
current tool is structurally blind to any 551 writer. Extension = one match
arm + `GFlagSite` field + the `man-scripts --gflag-partition` printer
(`commands/trace.rs:138-155`). This one small change unblocks BOTH
B-overworld-gate and B-flag551.

### 1d. Port gaps confirmed (with one filename fix)

- `801d688c` menu cursor-nav: zero engine hits; dump exists locally as
  `ghidra/scripts/funcs/overlay_save_ui_select_801d688c.txt` (NOT
  `801d688c.txt` as the backlog implies). Shape confirmed: confirm mask ->
  SFX 0x36 ret 1; cancel -> 0x37 ret 2; held left 0x1000 / right 0x4000
  cursor +-1 clamped to count -> SFX 0x21 ret 3; else ret 0.
- `80024e08` set-model: stub confirmed at
  `crates/engine-vm/src/field/host.rs:1172` (`op4c_n5_sub0_set_actor_model`,
  body `let _ = ...`). Dump present (`funcs/80024e08.txt`). Semantics from
  the dump: `sh model_id, actor+0x64`; zero `actor+0x5c`; clear draw bit
  0x1000 in `actor+0x10`; `_DAT_8007b83c == 0xf` branch mirrors the id into
  `+0x60` (else additionally clears bit 0x8000); re-stage via
  `FUN_80020F88(actor)`; restore saved `+0x10`.

### 1e. GAP 2 (op-0x44 gate) confirmed - but the boolean is not the blocker

Both spawn paths guard on `opening_chain_active`
(`scene_entry.rs:955` + `:814`). The docstrings name the real constraint:
the engine models a spawned P2 record as THE single cutscene timeline.
Generalizing is a modelling decision (multi-context vs "no active timeline"
relaxation), so it is pulled on demand by Arc 2, not done speculatively.

### 1f. Everything else spot-checked

Spine oracle ends at keikoku (`chapter1_spine_oracle.rs`); portals install
unconditionally (`scene_entry.rs:757-779`); world-map.md:235-238 carries the
matching UNKNOWN note; port-catalog dashboard: 479 open, the two genuine
gaps above; Track A residue (A7-0x400 applier, A3 batch, A5, v12-b0
watchpoint) is as the backlog says - short, capture-gated, low value.

---

## 2. Proposed arcs

Priority order. Each arc = a self-contained PR-sized unit with a disc-gated
oracle; worktree-per-code-arc with disjoint file scopes, per the wave-3
recipe.

### Arc 1 - Overworld progression gating (RE + engine; the headline)

Closes B-overworld-gate + B-flag551 together, static-first.

1. **Tooling**: extend `walk_partition_gflag_sites` + `man-scripts
   --gflag-partition` to `InsnInfo::SystemFlag` (SET/CLR/TEST, all
   partitions). Add a disc-wide census mode (every CDNAME scene x partition
   -> `flag -> [(scene, partition, record, op, kind)]`), since setters for a
   scene's gates usually live in OTHER scenes' MANs.
2. **RE close**: from the census, pin the setters for 0x193 (keikoku/Ravine
   completion), 0x482 + 0x2FC (mist walls), and town01's 549/550/551 (the
   DINNER latch; 551 is likely a talk-to-Mei interaction record). Escalate
   to a live probe ONLY for flags with no script setter (engine-side grants,
   e.g. battle-victory writes) - the save-state library brackets the Ravine
   completion, so a before/after RAM diff is one `legaia_mednafen` read, not
   a new capture.
3. **Engine**: evaluate `p2_record_gates_pass` on the walk-on/OverworldPortal
   engage path (worldmap.rs / scene_entry.rs drain) and install the C1-gated
   beat-band records on the overworld, so pre-0x482 the mist walls force-walk
   the player back exactly as retail. Wire the town01 551 setter once pinned,
   killing the DINNER re-fire.
4. **Oracle**: extend `chapter1_spine_oracle` - assert keikoku is the ONLY
   reachable dungeon on a fresh overworld arrival, and that flipping the
   pinned flags opens the retail sequence. Plus a `walk_on_trigger` test that
   551 latches after the setter runs (replacing the current "never set"
   assertion at `walk_on_trigger_dispatch_disc.rs:326`).
5. **Docs**: resolve the world-map.md UNKNOWN; new subsection in
   field-locomotion.md or world-map.md for the C1-latch gating idiom; update
   open-rev-eng-threads.md; mirror site fragments.

### Arc 2 - B-spine leg 2: the first-boss chain (map01 -> rikuroa/dolk/dolk2 -> boss)

1. **RE**: find the MAN carrier in the v12-family bundle layout (rikuroa
   entry 155 / dolk2 entry 68). Static-first: scan the `SceneEventScripts`
   entry interior + siblings for MAN section headers / an interior asset
   table; grep the funcs/ corpus for the v12 prescript reader's MAN hand-off.
   This doubles as a fresh attack on the open scene-v12 consumer thread.
2. **Engine**: teach `find_bundle`/`field_man_payload` the layout; then
   extend the spine - map01 -> rikuroa (Genesis Tree leg) and the dolk/dolk2
   town legs, ending in the first boss battle end-to-end via the existing
   battle stack (encounter registry already carries dolk/zeto patterns).
3. **GAP 2 on demand**: if a spine scene spawns helper records mid-play,
   relax the op-0x44 gate to "no active timeline" (keeping the
   single-timeline model) rather than a full multi-context rework.
4. **Oracle**: spine oracle legs 3+4 (rikuroa arrival; boss battle enter +
   victory loot applies), anchored to catalogued save states.

### Arc 3 - The two port gaps (small, disjoint, parallel-safe)

1. `80024e08` set-model: implement the stub body per the dump semantics
   (1d above), `// PORT: FUN_80024e08`, unit tests (34 refs).
2. `801d688c` cursor-nav: port as an engine menu-input primitive and route
   the save/menu screens' left/right/confirm/cancel handling through it
   (67 refs). Honours the existing SFX cue ids (0x36/0x37/0x21).

### Arc 4 - Opportunistic Track C closes (pick 2-3, dump-only)

From the per-doc Open sections, best value-per-hour candidates:
- battle-action.md: sweep the unassigned state opcodes (reserved-padding vs
  reachable) - one dump pass over the 0898 overlay dispatcher.
- New-game roller-config sub-thread: decode the `4C 88`/`CC F8 E8` config-op
  operands so `RollerParams` derives from scene bytecode instead of the
  per-scene pixel table.
- tile-board.md: per-cell tile-actor rendering (engine currently draws
  nothing for boards; template ids already tracked).
- muscle-dome.md: label the `&PTR_DAT_801f4d34` sub-draws via a
  `FUN_801d8de8` HUD census (static).

### Deprioritized (unchanged from backlog)

Track A capture residue (A7 bit-0x400 applier, A3 Super-Art byte-exactness
batch, A5 Sim-Seru validation) - only if an emulator session is already up
for Arc 1.2's escalation. B9 human eyeball passes remain user-time items;
Arc 1+2 add "retail-order progression" to that checklist.

---

## 3. Execution shape + gates

- Wave-3 recipe: read-only recon agents RETURN findings; ONE owner writes
  shared indices (open-rev-eng-threads.md, functions.md); worktree per code
  arc with disjoint file scopes; cherry-pick reconcile.
- RE-VERIFY every RESOLVED/FALSIFIED verdict against the local dump before
  landing (wave-3 caught an over-claim only via re-verify).
- Full disc-gated suite after reconcile, re-run after each fix (first
  failing binary aborts and masks later regressions). Never pipe gate
  commands through `tail`.
- Docs rules: present tense, no session markers; every docs/ change mirrors
  its site/_content fragment + `python3 site/_gen.py` + link/density checks.
- This file is branch-local planning state - DELETE before merge.
