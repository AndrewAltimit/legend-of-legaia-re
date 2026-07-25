# Lane: footstep SFX cue id — runtime capture

## Result: evidenced negative. Retail plays no footstep sound.

`CUE_FOOTSTEP` stays `None` in `crates/web-viewer/src/play_sfx.rs`. There is no
retail id to put there, so the browser play page stays silent while walking and
its doc comment now says why, citing the capture instead of "unpinned".

## The instrument

`scripts/pcsx-redux/autorun_footstep_cue.lua` (new, committed). It does not
guess which producer a footstep would use — it watches all of them at once:

| Watch | What it catches |
|---|---|
| Exec `FUN_80035B50` | ring push (`DAT_8007B6D8[cursor] = id`, timer 0) |
| Exec `FUN_80035BD0` | ring overwrite of the current slot |
| Exec `FUN_8004FCC8` | cue / voice dispatcher |
| Exec `FUN_800250D4` | per-actor SFX trigger (bypasses the ring) |
| Exec `FUN_80065034` | the voice programmer the drainer calls — audibility |
| Write ×4 on `DAT_8007B6D8` | any producer that stores the id directly |
| Write on `DAT_800915DA` | the two bytes `FUN_80018DB0` arms per step |
| Exec `FUN_80018DB0` | the per-frame field cadence itself |

Ring / actuator write callbacks decode the store instruction at PC and read its
source register, because the debug hook fires *before* the store — reading
memory there gives the pre-store value.

`LEGAIA_WALK=1` cycles D-pad directions (`LEGAIA_WALK_DIRS`, default
`UP,RIGHT,DOWN,LEFT`, `LEGAIA_WALK_SEG` vsyncs each; never SELECT/START).
`LEGAIA_WALK=0` injects nothing. Player XZ, scene, mode and the whole ring are
logged every 60 vsyncs, so "did it actually walk" is in the artefact.

## The four runs (900 vsyncs each, same probe)

| State | Walk? | Moved | Cues |
|---|---|---|---|
| `s4_rimelm_door_transition` (town01) | no | no | **none** |
| `s4_rimelm_door_transition` (town01) | yes | (3264,3520)→(2368,3392) | `0x2E`, `0x2F` |
| `s3_rimelm_freeroam` (town01 interior) | yes | (4160,11840)→(4416,12370) | **none** |
| `sol_to_karisto_worldmap` (korout→map03) | yes | (8512,3468)→(9390,4410) | **none** |

Two walking runs across two scene kinds — a house interior and the kingdom
overworld — fire nothing at all: no ring store, no `FUN_800250D4`, and not one
`FUN_80065034` voice program. That last one is the decisive channel, because it
is downstream of every producer.

The one run that fires cues fires exactly **two**, `0x2E` at vsync 426 and
`0x2F` at vsync 628 — 202 vsyncs (~3.4 s) apart, which is not a step cadence.
Both come from `ra = 0x801E0350`, i.e. the `jal` at `0x801E0348` **inside the
field VM `FUN_801DE840`** — that is op `0x36`, bit-15-set sub `0`, the script
SFX arm (`docs/subsystems/script-vm.md`). They are scene-script literals fired
as the player crosses triggers; that run also crossed a door (pos jumps to the
house interior at vsync 180 and back). Descriptors: `0x2E` = program 2 / tone 1
/ note 61, `0x2F` = program 2 / tone 2 / note 62, both 1 voice, category 6.

Non-vacuity: that same run also caught one `FUN_800250D4(0x200, voice 0x12)`
and five `FUN_80065034` voice programs. The silent runs are a silent game, not
a blind probe.

## Reproduce

```bash
for w in 0 1; do
  LEGAIA_WALK=$w LEGAIA_FRAMES=900 LEGAIA_OUT_DIR=/tmp/foot$w \
  timeout --kill-after=20s 900s \
    bash scripts/pcsx-redux/run_probe.sh \
      --sstate saves/library/pcsx-redux/2fba9adf4ade....sstate \
      --lua scripts/pcsx-redux/autorun_footstep_cue.lua
done
```

(`2fba9adf4ade…` = `s3_rimelm_freeroam`; the exterior contrast is
`a89f131f7481…` = `s4_rimelm_door_transition`.) Heard, not just logged: no —
this is a logged result, and what it logs is the *absence* of a key-on, which
an ear cannot distinguish from a muted channel. The `FUN_80065034` count is the
stronger evidence than listening would be.

## Two side findings the wave should own

**1. `FUN_80018DB0` never fires its step gate while walking.** In all four runs
`_DAT_8007B8A4` sits pinned at `2` — the `0xF - (speed >> 4) >= 0xB`
else-branch — so the speed words it reads (`gp+0x614` / `gp+0x618`) never reach
the `0x30` a step needs. Both branches of the function were exercised
(`DAT_8007B79C` reads `1` in town01, `0` in korout/map03) and neither stepped.
Whatever drives that cadence, ordinary field and overworld locomotion does not.
`crates/engine-audio/src/footstep.rs` is a faithful port of the arithmetic, but
the port only reaches its step branch because `play_sfx.rs` feeds it a
synthetic `WALK_SPEED_UNITS = 0x30`; that constant's doc comment now says so.

**2. `FUN_8006E2B4` / `FUN_8006CE30` look like libpad, not SsAPI — this
contradicts a committed doc, and I did not edit it (out of scope).**
`docs/subsystems/audio.md` labels `FUN_8006E2B4` as the SsAPI seq worker-table
init `(cb1, cb2)` and `FUN_8006CE30` as `SsSeqSetUserData`. But the init
`FUN_8001D230` calls them as:

- `FUN_80056778(0x800915D8, 0x80)` and `FUN_80056778(0x800840F8, 0x44)` — zero
  two buffers;
- `FUN_8006E2B4(0x800840F8, 0x8008411A)` — those are **data** buffers 0x22
  apart, and `0x800840F8` is the known libpad report buffer the pad pump
  `FUN_8001822C` decodes. That is `PadInitDirect(pad1, pad2)`, not two
  callbacks;
- `FUN_8006CE30(0, 0x800915DA, 2)` and `FUN_8006CE30(1, 0x8009161A, 2)` — one
  2-byte table per port, 0x40 apart, inside the block just zeroed. That is
  `PadSetAct(port, table, len)`;
- then eight `FUN_80056638` / `FUN_80056668` pairs on event classes
  `0xF4000001` / `0xF0000011` with specs 4 / 0x8000 / 0x100 / 0x2000 — the
  textbook memory-card `InitCARD` open/enable sequence.

Note also that Ghidra drops `param_1` in `FUN_8006CE30`'s decompiled C (the
`jalr v0` passes `a0` through untouched) — the usual dropped-register-argument
artifact. If this reading holds, `DAT_800915DA/DB` are the DualShock actuator
pair and `FUN_80018DB0`'s step output is **rumble**, which would also explain
why the game exposes Vibration Battles/Events/Encounters options. Someone with
`docs/subsystems/audio.md` in scope should re-audit that row set.

## Files touched

- `scripts/pcsx-redux/autorun_footstep_cue.lua` (new)
- `crates/web-viewer/src/play_sfx.rs` (doc comments only; `CUE_FOOTSTEP` stays
  `None`)
- `docs/formats/sfx-table.md` (new section "Walking fires no cue")
- `handoff/lane-sfx.md` (this file)
