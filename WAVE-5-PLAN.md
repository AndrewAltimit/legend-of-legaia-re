# WAVE-5 PLAN (branch-local; DELETE before merge)

Proposed next round of RE + engine work, post-#305 (Wave 4). Grounded
against the backlog (`~/Downloads/legaia-backlog.txt`), the live indices
(`docs/reference/open-rev-eng-threads.md`, `port-catalog --dashboard`),
and a fresh code recon of the engine's battle-entry wiring (file:line
cites below verified against the working tree at `a1735da7`).

Theme: **Track B breadth is the frontier.** Wave 5's centerpiece is the
first boss (Zeto) end to end - the next chapter-spine leg - plus the two
spun-out flag-setter RE questions and a small Track-C close. Track-A
capture residue stays parked (low value, per backlog).

---

## 0. Verified open-set (recon findings; don't re-derive)

The Zeto leg's shape is now precisely known:

- **No dedicated "enter battle" opcode exists** in the field VM. The only
  field->battle bridge is the armed-YIELD path: a `0x37/0x41` YIELD
  executed while a scripted encounter is armed hands the 8-byte window to
  `host.install_scripted_encounter(...)`
  (`crates/engine-vm/src/field/step.rs:243-258`,
  `crates/engine-core/src/world/encounters.rs:433,453`). Retail
  discriminator = the consuming entity SM (`FUN_801DA51C` copies
  `entity[+0x94]` -> `0x8007BD0C`, writes `_DAT_8007B83C = 8`).
- **The cutscene-timeline stepper has NO battle branch.**
  `World::step_cutscene_timeline`
  (`crates/engine-core/src/world/narration.rs:481`) runs P2 record bodies
  through the real field VM; special-cased: narration, inline dialog,
  channel waits, name entry. Nothing battle-shaped.
- **`World::enter_battle(party_count, monster_count)`**
  (`crates/engine-core/src/world/actors.rs:644`, PORT `FUN_800513F0`)
  takes counts only; `enter_battle_from_formation`
  (`crates/engine-core/src/world/field_loop.rs:145`) overlays stats from
  the monster catalog. Only production trigger today = the
  random-encounter session (`begin_encounter_battle`,
  `field_loop.rs:63-76`) + the Tetsu dialogue-accept carrier
  (`world/field_carriers.rs:437`).
- **`encounter_registry` suppresses "zeto"** (pattern registered at
  trigger rate 0, `crates/engine-core/src/encounter_registry.rs:143`) -
  the registry models random-or-suppressed only.
- **The oracle stops at the dungeon door**:
  `crates/engine-shell/tests/chapter1_boss_spine_oracle.rs` asserts
  town01 -> map01 -> inside rikuroa (MAN partitions `[18, 70, 20]`,
  `:202`) / dolk, `SceneMode::Field`, player at the portal entry tile.
  Its module doc (`:24-30`) names the missing leg: the P2-timeline ->
  battle-stack trigger.
- **Tooling is ready**: `legaia-engine man-scripts --scene rikuroa
  --disasm-partition 2 --disasm-record N` (full opcode disasm of one P2
  record), `--gflag-partition 2` (flag SET/CLEAR/TEST sites),
  `--system-flag-census` (disc-wide setter map)
  (`crates/engine-shell/src/bin/legaia-engine/cli.rs:115-170`).
- **Existing captures already bracket Zeto**: `zeto_call_wave_mid_cast` /
  `zeto_big_wave_mid_cast` (scripts/scenarios.toml) are mid-battle
  full-RAM states - the formation cell `0x8007BD0C`, monster archive ids,
  and battle-mode globals are readable pure-Rust (`legaia_mednafen`) with
  NO new capture. `docs/subsystems/battle.md:343` already notes scripted /
  pincer formation ids `0x3D..0x3F` (modes `0xC`/`0x15`).

---

## 1. Arc 1 - Zeto first boss, end to end (B-spine; the centerpiece)

Goal: `chapter1_boss_spine_oracle` extends to: walk to the boss inside
rikuroa -> scripted battle vs the Zeto formation -> victory -> post-boss
story flags latched -> dungeon exit / next leg unlocked.

**1a. RE: pin the retail trigger (static-first, no emulator).**
- Disassemble rikuroa's 20 P2 records (`--disasm-partition 2`) + its
  gate lists (`partition2_record_gates`) and P1 placements. Identify the
  boss record: which op(s) enter battle, which formation id, what C1/C2
  gates it, what it SETs on the victory side.
- Grep `ghidra/scripts/funcs/` for writers of `_DAT_8007B83C = 8` and of
  `0x8007BD0C` beyond `FUN_801DA51C`/`FUN_801D9E1C` ("capture-blocked
  labels rot" - the answer is likely already in the dumps).
- Read the two Zeto mid-battle states to pin the formation contents
  (`0x8007BD0C` cell, monster ids, party seating) as ground truth.
- Competing hypotheses to adjudicate: (H1) timeline-placed carrier -
  the P2 record arms an entity whose SM does the Tetsu-style handoff;
  (H2) a direct op (0x34 stager sub-op or 0x4C sub-op) writing the
  handoff globals; (H3) a walk-on gate-1 trigger into a P2 record whose
  body ends in the handoff. The disasm decides; do not code before this.

**1b. Engine: timeline -> battle-stack bridge.** Whatever 1a pins,
model it as the scripted-battle install path: timeline/entity context ->
formation record -> `install_scripted_encounter` /
`enter_battle_from_formation`, keyed off the rikuroa P2 record + its
C1/C2 gates - NOT off `encounter_registry`. Reuse the pending-install
host-hook idiom (`world/vm_hosts.rs:662`, drained in `step_field`).

**1c. Victory continuation.** Post-battle: resume/complete the P2
timeline (or fire the victory-side record), latch the story flags 1a
found, verify the exit path opens. This is where Arc 3's 0x482 question
may close for free - check before running any RAM diff.

**1d. Oracle.** Extend `chapter1_boss_spine_oracle` with the boss leg
(disc-gated; win via the arm API as in `training_battle.rs`). Keep a
baseline leg so the test stays non-vacuous.

Risk/pull-on-demand: if the boss timeline needs a mid-play spawned P2
record, that is GAP 2 (multi-context timeline decoupling,
`scene_entry.rs` opening_chain gate) - pull it only if 1a proves it is
required.

## 2. Arc 2 - past the boss: interior 0x3F to dolk2 (B-spine leg 2)

dolk2 is reached from a dungeon INTERIOR, not a map01 portal. Once Arc 1
lands, wire the interior 0x3F hop (rikuroa/dolk interior -> dolk2) and
extend the spine oracle one more scene. Small if Arc 1's flag latching is
right; its dependency on Arc 1 is real (ordering, not parallel).

## 3. Arc 3 - flag-setter RE: 549 (0x225) + 0x482 (Track A, cheap)

Both flags have NO field-VM MAN setter (census-proven); the writer is an
overlay / battle / event path. Plan:
- First check Arc 1's disasm output: if the rikuroa victory record sets
  0x482 (mist walls), that half closes statically.
- Otherwise: before/after diff of the `DAT_80085758` system-flag bank
  across the bracketing story beats using the EXISTING mednafen library
  (pure-Rust `legaia_mednafen` read; no live probe, no new capture).
  Deliverable: setter identity documented in world-map.md /
  open-rev-eng-threads.md; engine keeps both gated either way (faithful).

## 4. Arc 4 - Track-C close: new-game roller-config operand decode

The `4C 88` / `CC F8 E8` roller-config op is confirmed real (geometry
seeds `_DAT_801c6ea4 +0x4e/+0x50/+0x52`); the missing piece is the
`CC F8 80` spawner dump. Add it to `dump_funcs.py`, run the post-script,
decode the operands, document. One agent, no engine wiring required.
Opportunistic extras only if cheap: pulls from doc "## Open" sections.

## 5. Deferred (explicitly NOT this wave)

- A7 residue (+0x16E bit 0x400 applier), A3 Super-Art byte-exactness,
  A5 Sim-Seru render-mode exerciser: capture-gated, low value - parked.
- scene-v12 b0 record-table consumer: needs a scene-load write-watchpoint
  on the v12 malloc buffer. Stretch synergy only: if an emulator session
  is up anyway for Arc 3, piggyback the watchpoint on a rikuroa load
  (rikuroa IS a v12-family scene). Do not open a session just for this.
- B9 human eyeball passes (town01 NPC clips, PSX-render overlay pass,
  full spine walkthrough): needs the user at the window - listed as a
  review checklist for the user, not agent work.

---

## 6. Execution recipe (wave-3/4 pattern, unchanged)

- Parallel subagents: read-only recon agents RETURN findings; worktree
  code agents with fully disjoint file scopes; ONE owner reconciles the
  shared indices (open-rev-eng-threads.md, functions.md, site fragments).
- RE-VERIFY every subagent RESOLVED/FALSIFIED/PORT verdict against the
  local dump before landing (Wave 3/4 both caught over-claims).
- Arc ordering: 1a is the gate - 1b/1c/1d and Arc 2 depend on it; Arcs 3
  and 4 are parallel-safe from the start.
- Gates: `cargo fmt --check`, `clippy -D warnings`, full disc-gated suite
  (`LEGAIA_DISC_BIN` exported in ~/.bashrc, so plain `cargo test
  --workspace --release`; never pipe to tail) after reconcile AND re-run
  after each fix. Site fragments mirrored for every docs/ change +
  `python3 site/_gen.py` + link/density checks.
- No Sony bytes committed. Branch `re-wave-5` off main; NOT pushed until
  review. This plan file is branch-local - delete before merge.
