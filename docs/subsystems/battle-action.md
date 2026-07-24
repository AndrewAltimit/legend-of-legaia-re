# Battle action state machine

A two-level finite state machine that drives the per-actor execution of a chosen battle action - the layer between "the player picked Attack" and "the actor's body has finished swinging the sword and HP has been deducted." Lives in the battle overlay (`0898`, RAM-resident at `0x801C0000+`). The driver is `FUN_801E295C`, dumped as `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt` (16 KB / 4099 MIPS instructions / 155 outgoing calls - the largest function in the battle overlay).

## Contents

- [One-paragraph overview](#one-paragraph-overview)
- [Outer dispatch - `ctx[7]` action-state cursor](#outer-dispatch---ctx7-action-state-cursor) · [state table](#state-table)
- [Inner dispatch - actor action category](#inner-dispatch---actor-action-category) · [per-actor sub-state surface](#per-actor-sub-state-surface)
- [The `0x51` exit gate and the HP-bar settle invariant](#the-0x51-exit-gate-and-the-hp-bar-settle-invariant) - the endless-camera-orbit softlock class
- [Cross-references with other battle helpers](#cross-references-with-other-battle-helpers) - [range/LOS](#fun_8004e2f0---battle-range--line-of-sight) · [stat aggregator](#fun_80042558---per-frame-stat-aggregator) · [effect spawn API](#fun_801dfdf8---effect-bundle-public-spawn-api) · [summon-overlay dispatch](#seru-magic-summon-overlay-dispatch) · [pose driver](#fun_801d5854---per-actor-pose-driver) · [party/monster setup](#fun_801eed1c--fun_801e7320---party--monster-setup-hooks) · [camera bounds](#fun_801efe44---battle-camera-bounds) · [escape roll](#the-escape-roll-fun_801e791c) · [battle voice cues](#battle-voice-cues---the-xa30-grunt-vs-the-xa2xa4xa6-arts-shout) · [helper functions](#battle-helper-functions)
- [Notes for the engine port](#notes-for-the-engine-port) · [decompile quirks](#decompile-quirks-worth-knowing) · [engine port](#engine-port)
- [Action validator (`FUN_8003FB10`)](#action-validator-fun_8003fb10) · [action queue + Tactical Arts trigger ordering](#action-queue-and-tactical-arts-trigger-ordering) · [Miracle / Super in the live Arts submenu](#miracle--super-in-the-live-player-driven-arts-submenu)
- [Pose-slot table `0x80076C10`](#pose-slot-table-0x80076c10-and-its-copy-helpers) · [overlay-local PRNG](#overlay-local-prng-fun_801d0290) · [open work](#open-work)

## One-paragraph overview

`FUN_801E295C` runs every frame from the [battle main dispatcher `FUN_801D0748`](../reference/functions.md). It picks up the global battle context (`_DAT_8007BD24`, a pointer to the live ctx struct at `0x800EB654`), resolves the **active actor** via `(&DAT_801C9370)[ctx[0x13]]` (the [8-slot battle actor pointer table](battle.md) - slots 0..2 party, 3..7 monsters), and pumps the action through three nested keys:

1. **Action category** - the actor's `+0x1DE` byte: 0=Martial Arts (Tactical Arts), 1=Item, 2=Magic, 3=Attack, 4=Spirit, 5=Run/Defend. Read once at the action-start case (`ctx[7] == 0xC`) and used to seed the next state.
2. **Execution phase** - `ctx[7]`, the action-state cursor. The outer `switch (ctx[7])`. States are numbered to bin by action category: `0x14..0x20` = Attack chain, `0x28..0x2E` = Magic / Item, `0x32..0x38` = Summon, `0x3C..0x40` = Spirit, `0x46..0x48` = Spirit-arts variant, `0x50..0x52` = Done / cleanup, `0x5A` = end-of-action gate, `0x64..0x6B` = Run / capture-fail, `0x6E..0x71` = Capture sequence.
3. **Per-actor sub-state** - `actor[+0x1DC]` (flag bits - `0x01` "windup done", `0x02` "advance done", `0x04` "exit"), and several per-actor scratch fields (`+0x1DA` queued anim ID, `+0x1D9` current anim ID, `+0x1DF..+0x1F2` action-parameter byte stream).

The function is not a bytecode VM. There is no opcode table, no PC stride. It is a **per-frame edge-triggered state machine**: each `case ctx[7]` body waits on a per-actor condition (animation matched, timer expired, distance check passed) and writes the next `ctx[7]` value when ready. Actions that need multiple frames (most) do nothing on the frames where their condition isn't met yet.

## Outer dispatch - `ctx[7]` action-state cursor

`ctx[7]` is the **execution phase** byte at `_DAT_8007BD24[7]`. The runtime models it as a `byte`, but the value range is sparse: the handled states fall into contiguous bands (one per action category). The dispatcher is a single MIPS `jr` jump table at `0x801CED44 + (ctx[7] << 2)` (`sltiu` bound `0x100` → 256 word slots, **no `default` case**); every state byte indexes the table, and any slot outside the handled set points at the shared post-switch epilogue (see [Open work](#open-work)).

| State band | Phase | Action category |
|---|---|---|
| `0x00`, `0x0A`–`0x0C` | Init / re-entry | (any) |
| `0x14`–`0x20` | Attack chain | Attack (`+0x1DE == 3`) |
| `0x28`–`0x2E` | Magic / Item flow | Item (`+0x1DE == 1`) or Magic (`+0x1DE == 2`) |
| `0x32`–`0x38` | Summon flow | Magic with summon flag |
| `0x3C`–`0x40` | Spirit flow | Spirit (`+0x1DE == 4`) |
| `0x46`–`0x48` | Spirit super-arts variant | Spirit (`+0x1DE == 4` with `+0x1F9 != 0`) |
| `0x50`–`0x52`, `0x5A` | Done / cleanup / end-of-action | (any) |
| `0x64`–`0x6B` | Run / Defend / capture-fail | Flee (`+0x1DE == 5`) |
| `0x6E`–`0x71` | Capture sequence | Magic with capture flag |
| `0xFD`, `0xFF` | Idle hold / battle-end | (any) |

### State table

Each row: `ctx[7]` value, what runs during that frame, and the next state(s). All citations are to `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt`.

| `ctx[7]` | Phase | What runs | Next state |
|---|---|---|---|
| `0x00` | Action begin | Resets ctx counters at `+0x6DA..+0x6DB`; copies `ctx[+0x274]` (the active-actor index set by `recompute_battle_order`) → `actor[+0x1A]`; **latches** `ctx[+0x290]` → `ctx[+0x291]` and *then* clears `ctx[+0x290]` (`0x801E2B30`). The latch is what the escape roll reads all battle - see [the escape roll](#the-escape-roll-fun_801e791c). | `0x0A` (or `0x0B` if `ctx[+0x276] != 0` is set, i.e. action queued from menu). |
| `0x0A` | Pre-action wait | Calls `func_0x8003F2B8(1)` (likely a "pause until previous animation cleared" gate). | `0x0C` when ready, else stays. |
| `0x0B` | Action queued from menu | Holds while `ctx[+0x276] != 0` (menu still open). | `0x0A` once cleared. |
| `0x0C` | **Action seed** - reads `actor[+0x1DE]` (action category) and dispatches into the appropriate band. Calls `FUN_801EED1C` (the arts queue-builder; slot < 3) or, for a monster slot with the `+0x16E & 0x380` bits, `FUN_801E7320` (random-retarget: the rolled action - including a Magic cast - is kept, only its target re-rolls to the opposite side; see the [`0x380` notes](#ai-delegated-0x380-party-members---what-is-and-isnt-pinned)). Reads RNG via `func_0x80056798()`. Calls `FUN_801EFE44` (camera bounds) and `FUN_801D5854(actor_id, 6)` (idle pose) unless `+0x1DE == 5` (run). The inner switch on `actor[+0x1DE]` is the "action category" dispatch - see [Inner dispatch](#inner-dispatch---actor-action-category). | `0x14`/`0x28`/`0x3C`/`0x46`/`0x50`/`0x64`/`0x68` per category. |
| `0x14` | **Attack - face target** | `FUN_801D5854(actor, 6)` (ready pose); computes target bearing via `func_0x80019B28(s8 X/Z, actor X/Z)` and writes facing into `actor[+0x46]`; iterates the 8-actor table at `0x801C9370` writing AI-side facing offsets at `ctx[+0x6E6 + i*2]`; calls `FUN_8004E2F0(actor, target)` for [range/LOS](battle.md). If range = 0 → `0x1E` (skip approach). Party arm: stages approach anim `+0x1DA = 1` (the walk entry) → short-step. Monster arm: first-byte tag search over its action-record array (`FUN_80050E2C`, tag `0x20`, retry `1`) stages the returned entry index. | `0x15` (monster, tag-0x20 found); `0x19` (party); `0x1E` (out of range). |
| `0x15` | Attack - windup | Same idle pose + facing update; advances anim cursor `actor[+0x1DA]` until it matches `actor[+0x1D9]`, then re-queries swing table. | `0x16`. |
| `0x16` | Attack - advance | Same pose; rechecks range with `FUN_8004E2F0`. While out of range, advances `actor[+0x38/+0x34]` along bearing using sin/cos LUTs `_DAT_8007B7F8` / `_DAT_8007B81C` (steps both attacker and target s8 by `>> 9`); re-tests every iteration. When in range, queries the next entry from the per-character swing table. | `0x17`. |
| `0x17` | Attack - close-range | Anim/facing update; matches `actor[+0x1DA]` against `actor[+0x1D9]`. | `0x18`. |
| `0x18` | Attack - strike | Final anim match → falls into the swing apex frame. | `0x1E`. |
| `0x19` | Attack - short-step (party slot < 3 only) | Idle pose + facing + range recheck. While range > 0 → stays. Range == 0 → bumps `actor[+0x1DC] |= 1` (windup-done flag) and `actor[+0x16] = 0`. | `0x1E`. |
| `0x1E` | **Attack chain - strike loop** | Per-strike counters (`+0x15`/`+0x16`) advancing the attack-script byte stream at `actor[+0x1DF + +0x15]`, with counter-attack redirect and ability-flag impact-step physics. Full step body: [Attack chain - strike loop (`0x1E`)](#attack-chain---strike-loop-0x1e). | `0x1F` once the strike-script terminator is hit. |
| `0x1F` | Attack - recovery wait | `FUN_801D5854(actor, 7 or 8)` (recover-pose; pose 8 if target's anim matched a counter trigger at `s8[+0x1F1]/+0x1F2`). Waits for `actor[+0x1DC] & 2 == 0`. | `0x20`. |
| `0x20` | Attack - return | Decides if combat continues by inspecting target liveness (`s8[+0x14C] != 0` for monster-slot, plus `s8[+0x1D9]` == `0` or `8`, plus `actor[*0x22C][+0x74] & 0xFFFFFF`), and counter-attack trigger flags (`ctx[+0x287] != 0 && DAT_8007BD0D == 0 && ctx[+0x288] != 0`). If combat ended → `0x50`. Else loops `FUN_801D5854(actor, 7 or 8)` per liveness. | `0x50` (done) or stays. |
| `0x28` | **Magic / Item - cast begin** | Resolves bearing + facing, sets the cast timer, looks up the spell-name HUD label, and deducts the (ability-bit-scaled) MP cost; capture-class spells route to `0x6E`. Full step body: [Magic / Item - cast begin (`0x28`)](#magic--item---cast-begin-0x28). | `0x29` (or `0x6E` for capture). |
| `0x29` | Magic - pre-cast wait | Decrements `ctx[+0x6D8]` by `DAT_1F800393` (frame dt). When it goes negative: if party_id < 3 calls `FUN_801DBF9C(party, spell_id)` (spell anim trigger). If `actor[+0x1E0] == 9` → routes to `0x32` (summon path). Pulls next anim from `actor[+0x1DF + +0x15]`; if `-1`, finishes → `0x50`. Else if spell_id < 0x81 → `FUN_801DC0A0(party, anim_id)` and special-case sound triggers (`func_0x8004FCC8(0x14C / 0x144 / 0x15E)` for spell IDs `0x3F / 0x2C / 0x6A`). | `0x29` loop or `0x32` (summon) or `0x50` (done). |
| `0x2A` | Magic - animation chain | Reads next byte from `actor[+0x1DF + +0x15]`. If not terminator: stages `actor[+0x1DA] = next_byte`, calls `FUN_801DC0A0`, sets `actor[+0x1FA] = 1`. On terminator (`-1`): if `actor[+0x15] == 2` sets `actor[+0x1FA] = 1` and OR's `actor[+0x1DC] |= 4`. | `0x2B`. |
| `0x2B` | Magic - sustained anim | Continues `FUN_801DC0A0` calls; checks `actor[+0x1FA] == 0`. | `0x2C` (and OR's `actor[+0x1DC] |= 4`). |
| `0x2C` | Magic - hit-frame loop | `FUN_801DC0A0` per frame; condition: `actor[+0x1D9] == 0` OR `(ctx[+0x24C] >= actor[+0x21B] && actor[+0x21B] != 0)` (hit-counter reaches script bound). | `0x2D`. |
| `0x2D` | Magic - recovery | If `ctx[+0x24D] == 0`: clears `actor[+0x176]` and `actor[+0x21B]`. Item-class spells (target == 9) set `DAT_8007B64C = 0x78` (UI flash). | `0x2E` once `+0x24D == 0`. |
| `0x2E` | Magic - exit | Gated on `ctx[+0x249] == 0`. Resets screen-shake (`_DAT_8007B790` if > 400, sets to 0; `_DAT_800840BC = 0x500`). | `0x50`. |
| `0x32` | Summon - invoke | `FUN_801D5854(actor, 6)` + waits on `func_0x8003DE7C(1)` (sound bank ready). When ready, computes summon-frame index `bVar5` from `actor[+0x1DF]` (if < 0x9A: `(actor[+0x1DF] + 0x7F) * 3 + 0x80`, else `actor[+0x1DF] * 4 + 99`); writes `ctx[+0x277] = bVar5`, `ctx[+0x276] = 1`, `ctx[+0x278] = 1`. Sets `actor[+0x1DA] = 9`, `actor[+0x1DC] |= 1`, `actor[+0x1FA]++`. | `0x33`. |
| `0x33` | Summon - fade in | `FUN_801DC0A0(party, 0x12)`. When `actor[+0x1F5] != 0` (anim cue): writes a 16-byte BG fade descriptor (`DAT_801C9070..0x9086`: time `0x14`, RGB `(0xFF,0xFF,0xFF)`, alpha 0→`0x14`); calls `func_0x80024E80` (push fade primitive). | `0x34`. |
| `0x34` | Summon - actor freeze | `FUN_801DC0A0(party, 0x12)`. When `actor[+0x1D9] == 0`: OR's the fade primitive bit `8`, clears `ctx[+0x278/+0x279]`, sets `ctx[+0x6D8] = 0x78` (timer), calls `func_0x801F1ED4` (the [player-summon effect-script dispatcher](#battle-helper-functions), keyed on the summon id `actor[+0x1DF]`), iterates the 8-actor table to clear `actor[+0x4]` and set `+0x21C = 0xFF` (actor-hidden marker). Writes a second fade descriptor (`0x78` time, alpha `0xFF→0`). | `0x35`. |
| `0x35` | Summon - sustain | Decrements `ctx[+0x6D8]`; ramps screen brightness `_DAT_8007B910` down by `DAT_1F800393` per frame, clamped at `(_DAT_8008457C * 0x4B) / 100` (75%) for spells < 0x99 or 50% for higher. If `+0x6D8 < 0` and `ctx[+0x276] != 0`, force-clamp `+0x6D8 = 1`. | `0x36` when timer expires. |
| `0x36` | Summon - return-from-fade | Runs `func_0x801F1ED4` (the [player-summon effect-script dispatcher](#battle-helper-functions)) again while the fade settles. Then iterates 8-actor table clearing `+0x21C = 0` and resetting `+0x8 = 0x81000000` for actors with `+0x4 == 0`. Calls `FUN_801E70BC` (the summon-magic level-up check - see [`reference/functions.md`](../reference/functions.md); engine `World::accrue_summon_spell_xp` + `battle_formulas::summon_magic_levels_up`). | `0x37`. |
| `0x37` | Summon - verify all alive | `FUN_801D5854(actor, 6)`. Iterates the 8-actor table (party + active monsters); checks each is alive (`+0x14C != 0` AND `+0x1D9 != 0`). Sets a 4-byte fade-back-in sentinel at `ctx[+0x890..+0x893]` (`84 10 42 08`). | `0x38`. |
| `0x38` | Summon - done | OR's the fade primitive bit `8`; clears `DAT_801C938C[+0x22C]`. | `0x50`. |
| `0x3C` | **Spirit / Item - pre-arm** | `FUN_801D5854(actor, 6)`. Sets `actor[+0x1DA] = actor[+0x1E7]` (queued anim). Sets `ctx[+0x243] = 1` ("action in progress" marker). For `+0x1DE == 1` (Item): looks up item record at `ctx[+0x1DF]*0xC + -0x7FF8BC97` for label/icon; writes `actor[+0x1E8/+0x1E9]` (icon page/x); writes HUD via `_DAT_80077332..+0x35C`. Special case: `actor[+0x1DF] == 0xFE` (Pomander) → label = `s_Points_returned_801CED34`. For non-Item (Magic/Spirit, `+0x1DE != 1`): does the same write of `+0x1E8/+0x1E9` from the spell table at `actor[+0x1DF]*0xC + -0x7FF8AB38`, computes MP cost (with ability-bit half/quarter), subtracts from `actor[+0x150]`; for party_id < 3 fires `FUN_801D8DE8(7, 0)` (UI element). Always fires `FUN_801D8DE8(0x4C, 0)` (HUD label). | `0x3D`. |
| `0x3D` | Spirit - wait | `FUN_801D5854(actor, 6)`. Holds while `actor[+0x1DA] != actor[+0x1D9]`. When matched, clears `actor[+0x1DA]`, calls `func_0x801F3990` (the [cast audio-cue dispatcher](#battle-helper-functions)). | `0x3E`. |
| `0x3E` | Spirit - fire | `FUN_801D5854(actor, 6)`. Holds while `actor[+0x1D9] != 0`. Calls `func_0x800319A8(0x21)` and `FUN_801D8DE8(0x4C, 1)`. For spirit-type 4 (Originals) on party, fires `FUN_801D8DE8(0x34, 1)`. For type 5 (Spirit-arts variant), invokes the Damage UI: writes `_DAT_80076D7E` (damage value) from target HP+formula, calls `FUN_801D8DE8(0xF, 0)` (damage popup) and `FUN_801D8DE8(0x52, 0)` (damage text); RNG via `func_0x80056798`; computes damage scaling: `((target_HP * 7) / 5) + 8`, capped at 0x120 or 100. Otherwise re-fires UI elements 6/0x4E/0x4F (monster effect) or 7 (party effect) per slot. Sets `ctx[+0x6D8] = 0x20` (post-cast timer). | `0x3F`. |
| `0x3F` | Spirit - wait & fire damage | Decrements `ctx[+0x6D8]`. On expiration: calls `func_0x800402F4(actor[+0x1E8], actor[+0x1E9], target, party_id-1)` - the **damage application primitive**. Sets `ctx[+0x6D8] = 0x80` (post-damage cooldown). | `0x40`. |
| `0x40` | Spirit - post-damage | `FUN_801D5854(target, 6)`. Iterates HP-bar widget at `ctx[+0x1080]+0xE`: ramps it toward `ctx[+0x6DC]` (target HP) by `DAT_1F800393` per frame; mirrors damage-popup widget at `_DAT_801F6968+0x10`. When `ctx[+0x6D8] < 0` and target is no longer valid (dead or out of slot), sets `actor[+0x1DE] = 0` and clears HUD. | `0x50`. |
| `0x46` | **Spirit super-arts - entry variant** | `FUN_801D5854(actor, 6)`. Sets `actor[+0x1DC] = 2` (overrides flags). Stages anim `actor[+0x1DA] = actor[+0x1E7]`. Computes damage = `((target_HP * 7) / 5) + 8` (capped 0x120 / 100); HP-bar target = `actor[+0x170] + 0x20` (or `+0x28`/`+0x23` per ability-flag bits). | `0x47`. |
| `0x47` | Spirit-arts - sustain | `FUN_801D5854(actor, 6)`. When `actor[+0x1D9] != 0`, clears `actor[+0x1DA]`. Decrements `ctx[+0x6D8]`. While running: ramps damage-popup HP/widget; when expired and `actor[+0x1F9] == 0` (no spirit-shield), advances HP-bar at `ctx[+0x1074]+0xE`. | `0x48` once exit-flag (`actor[+0x1DC] == 0`) and timers settle. |
| `0x48` | Spirit-arts - flush | Final ramp of HP-bar / damage-popup. When all targets read zero AND timer expired AND anim flags clear → done. | `0x50`. |
| `0x50` | **Done - cleanup phase** | The universal "action concluded, clean up" arm. Calls `FUN_801E6968` (the Lost Grail **Final Heal** auto-revive; engine `World::apply_final_heal_revives`), counts living party + monster actors (`+0x14C != 0 && (+0x16E & 4) == 0`); if any survivors → `FUN_801DABA4` (recompute battle ordering). Resets `actor[+0x224] = 8` (or `0x20` for spirits/`+0x1DE == 4`). Adjusts `actor[+0x170]` (HP-bar target) by ability-flag bits `0x100`/`0x200`. Clamps `actor[+0x170]` at 100. OR's `actor[+0x1DC] |= 4`. Per category: `+0x1DE == 5` (run) → screen-shake; `+0x1DE == 3` (attack) or party with dead s8 → pose 8; otherwise pose 6. Sets `ctx[+0x6D8] = 0x3C` (or `0x96` if shake, `+0x26 != 0`). If `ctx[7] == 0x50`, advances to `0x51`. | `0x51`. |
| `0x51` | Done - fade-down | Ramps `_DAT_8007B910` back up to `_DAT_8008457C` (full brightness). Per-category pose updates. Calls `FUN_801E7250` (?); decrements `ctx[+0x6D8]`. When < 0 and `ctx[+0x276] == 0`: if `ctx[+0x269] == 0` → `0x5A` (next-actor / end-of-action); else → `0x52` (continue queue). When timer < 0xC, calls `FUN_801D99BC` and unloads all UI elements: `FUN_801D8DE8(actor[+0x18], 1)` (anim), `+0x4E/+0x4F` if anim was 6, `actor[+0x26]`, `+0xF/+0x52` (damage), `+0x44`, `+0x59` (queue marker), `+0x51`/`+0x50` (banner). For multi-cast (`_DAT_801F6974 != 0`), iterates queue at `((+0x6974)-1)*4 + -0x7FE097CC` firing every queued effect's terminate marker. | `0x52` or `0x5A`. |
| `0x52` | Done - multi-cast continuation | `FUN_801D5854(actor, 8)` (action-end pose). Decrements `ctx[+0x6D8]`. If timer > 0x13 and screen-shake active (`_DAT_8007B874 != 0`), clamps timer at 0x13. When < 0: clears `ctx[+0x269]`, advances to `0x5A`. When < 0x14 and `actor[+0x17] != 0` (was running), unloads the queue marker. | `0x5A`. |
| `0x5A` | **End-of-action gate** | Iterates 8-actor table clearing per-actor anim flag bits (`+0x8 &= 0x7CFFFFFF`, `+0x21F = 0`). Resets dead/inactive actors' `+0x36 = 0`, `+0x21C = 0`, `+0x225 = 0`. Counts living actors per side: if all party or all monsters dead, sets `DAT_8007BD71 = 0xFE` (battle-end signal) + `_DAT_8007BD2C = 5` (party wipe) or `0` (monster wipe), AND's `DAT_8007BD60 &= 0x7F`. Otherwise, picks the next active actor: bumps `actor[+0x1A]++`; if `+0x1A < (party_count + monster_count - +0x25)`, advances to `0x0A` (next action); else → `0xFF` (battle complete). | `0x0A` (next actor) / `0xFF` (battle ends). |
| `0x64` (100) | **Run - flee anim begin** | Calls `FUN_801E791C` ([the escape roll](#the-escape-roll-fun_801e791c) - decides the flee, writes `_DAT_8007726C`). Sets `ctx[+0x6D8] = 0x3C`. Fires `FUN_801D8DE8(0x43, 0)` (run UI). Iterates monster slots: if monster has rotation trigger (`+0x16C != 0`) and isn't immune (`(&DAT_8007BD10)[i] != 4`), bumps `actor[+0x1A]++`. If party-side ran (`_DAT_8007726C != ctx + 0x189`, the run roll succeeded): screen-shake, and **floors every party actor's live HP at 1** (`+0x14C == 0` → `1`, loop bound = party count) - a downed or petrified member leaves the battle alive, the mechanism behind "escape restores a Stoned member". Ported: `engine-vm::battle_action` `RunBegin` + `StatusEffectTracker::cure_stone_on_escape`. Else screen-shake only. | `0x65`. |
| `0x65` | Run - wait | If the run failed (`_DAT_8007726C == ctx + 0x189`) → screen-shake `_DAT_8007B792` rotates. Decrements `ctx[+0x6D8]`. When < 0: **failed run** → `0x50` (Done band - the action is consumed, the battle continues); **successful escape** → `0x66`. | `0x50` (failed) or `0x66` (escaped). |
| `0x66` | Run - **successful-escape teardown** | Writes the fade template at `DAT_801C9070` - kind 2, time `0x40`, start `(0,0,0)` → end `(0xFF,0xFF,0xFF)` (a black→white white-out, ramped by the `FUN_80020B00` fade-state loader) - and spawns it via `func_0x80024E80(&DAT_801C9070, 0)`. Sets `DAT_8007BD71 = 0xFE` - the **battle-end signal**, the same byte the `0x5A` wipe gate sets - so the party leaves the battle. (The earlier "run failed, battle continues" reading of this state is falsified by that signal byte; the failed-run path is `0x65 → 0x50`.) Engine: `ActionState::RunEscape` → `BattleEndCause::Escaped`; the fade is the `engine_core::fade` kernel. | `0x67` (terminal hold; no case body - falls through to default no-op). |
| `0x68` | **Capture - start** | RNG via `func_0x80056798`. Adjusts `ctx[+0x6DA] += 0x780 + (rand%2)*0x80`. `FUN_801D5854(actor, 6)`, `FUN_801E7824(actor)` (?), `FUN_801DABA4`. Sets `ctx[+0x6D8] = 0x1E`. | `0x69`. |
| `0x69` | Capture - wait | `FUN_801D5854(actor, 6)`. Decrements `ctx[+0x6D8]`. When < 0: sets `ctx[+0x6D8] = 0x5A`, sets `actor[+0x225] = 2`, `+0x21C = 2`. | `0x6A`. |
| `0x6A` | Capture - sustain | Decrements `ctx[+0x6D8]`; if `ctx[+0x276] != 0` clamps timer at 1. When < 0: ctx[+0x6D8] = 0x3C, calls `FUN_801D99BC`, `FUN_801D8DE8(0x43, 1)` (run-UI close), `actor[+0x4] = 0`, `FUN_801D5854(0, 9)` (defeat pose). Screen rotates. | `0x6B`. |
| `0x6B` | Capture - end | `FUN_801D5854(0, 9)`; screen rotates; decrements timer. When < 0 → `0x5A` (end-of-action). | `0x5A`. |
| `0x6E` | **Magic-capture branch** | `FUN_801D5854(actor, 6)`; waits on `func_0x8003DE7C(1)` (CD ready). When ready: calls `func_0x8003EAE4(0, capture_index)` (load capture archive); sets `_DAT_8007BDB0` to capture-monster index. | `0x6F`. |
| `0x6F` | Magic-capture - fade | If `ctx[+0x287] != 0`: ramp `_DAT_8007B910 -= DAT_1F800393`, clamp to `(_DAT_8008457C * 0x4B) / 100`. Adjusts ctx-buffer X position. Waits on `func_0x8003F2B8(1)`. | `0x70`. |
| `0x70` | Magic-capture - phase 2 | Same brightness ramp as `0x6F`. Runs `func_0x801F2160` (the [magic effect-class dispatcher](#battle-helper-functions), keyed on the spell's effect-class byte). When done, calls `func_0x801F0348` (the [target-size camera framing](#battle-helper-functions)). | `0x71`. |
| `0x71` | Magic-capture - finalize | `FUN_801D5854(actor, 6)`; checks all 8 slots are settled (alive with non-zero `+0x4`, or non-`8` `+0x1D9`). Once stable: clears ctx buffers, writes the 4-byte fade sentinel (`84 10 42 08`), iterates resetting per-actor `+0x21C = 0` and `+0x8 = 0x81000000`. | `0x50`. |
| `0xFD` | Idle hold (battle paused?) | `FUN_801D5854(actor, 8)`. No state change. | (stays). |
| `0xFF` | **End of round** (not battle end - see below) | Sets `ctx[+0x6] = 0x14`, increments `ctx[+0x28A]` (round counter), calls `func_0x801F45A4` (the [end-of-action damage/HP-bar settle](#battle-helper-functions)). | round boundary; the next round's actor selection follows. |

States `0x67` (post-fade hold), `0x07` (unused?), and several gaps in the table are not present as case labels - they fall into the `default` no-op arm (the dispatcher's `sltiu v0, v1, 0x100` bound is 256 and the JT slot for unhandled values points at the function epilogue at `0x801E6814`).

#### `0xFF` is the round boundary, not the battle's end

Worth stating separately, because the opposite reading was recorded here and the engine
port inherited it. `0xFF` has exactly one writer: the **non-wipe** arm of `0x5A`, reached
when every living actor has acted and both sides still have someone standing. The wipe
arms do not write a state byte at all - they raise the battle-end *signal*
`DAT_8007BD71 = 0xFE` (with `_DAT_8007BD2C` = `5` party wipe / `0` monster wipe), which is
also what the successful-escape teardown `0x66` raises. So battle end is signalled through
`DAT_8007BD71`, never through the state byte, and reaching `0xFF` means "the round is
over", not "the battle is over".

Read the two rows together and the table already said so: the `0x5A` row routes wipes to
the signal and only the everyone-has-acted path to `0xFF`.

**Port consequence, open.** `engine_vm::battle_action` maps `0xFF` to
`ActionState::BattleComplete`, whose handler calls `battle_end(BattleEndCause::MonsterWipe)`
- asserting a wipe that has not happened - and the live loop turns that into
`finish_battle`. Whether a real battle reaches that path depends on how
`action_queue_counter` accumulates across a round, which is not settled here; the failure
mode if it does is a spurious victory, with loot and XP granted after one round. Tracked in
[`reference/open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

#### Attack chain - strike loop (`0x1E`)

The full step body for state `0x1E`:

Counters `actor[+0x15]` (per-strike index) + `actor[+0x16]` (combo bit). Reads the
per-actor attack-script byte stream at `actor[+0x1DF + +0x15]`. The inner step writes
`actor[+0x1DA] = next_anim_id` and OR's `+0x1DC |= 2`. The byte read is **gated on
`+0x1DC` bit `0x2` being clear** (`0x801E370C`: `lbu +0x1DC; andi 0x2; bne -> skip`) -
while the previous staged swing is still in flight the step does only the per-frame
physics, so strikes pace one-per-clip, with the anim system's end-of-clip edge clearing
the bit. Counter-attack handling: if
`_DAT_801F6970 != 0` and the *target's* sub-state byte at `s8[+0x1DE] == 3` (was
attacking), redirects active actor to the counterattacker - sets
`s_Counterattack_successful_801CED18` text, fires effect `FUN_801D8DE8(0x66, 0)`, swaps
`actor[+0x13]`. Per-frame physics: target/attacker drift along bearing scaled by
`actor[+0x21D]` (impact-step magnitude) when ability flags `0x10/0x20` are set in the
character record at `0x80084708 + (party_id-1)*0x414`. Reads `actor[+0x1DF + +0x15]`
until the `0x00` terminator is hit (the magic band is the band that uses `-1`; the
earlier `0xFF` note here was wrong). The stream alphabet for a party attack is direction
swings `0x0C..0x0F`, art starters `0x19`/`0x1A`, and art action constants `0x1B+` (see
[art-data.md](../formats/art-data.md)); the Miracle-Art continuation refills consumed
slots with `0x19` before re-walking. Staged ids `>= 0x10` are remapped to the dynamic
art slots `0x10`/`0x11` by the anim commit `FUN_8004AD80` at install (see
[battle-data-pack.md § Battle
animations](../formats/battle-data-pack.md#battle-animations-record0)).

#### Magic / Item - cast begin (`0x28`)

The full step body for state `0x28`:

- If `+0x1DE == 9` (item-target re-route), reseats `actor[+0x1DD]` to `ctx[+0x24B]` (item-target). If `+0x1DE == 8`, similar reroute via `ctx[+0x24A] - 1`.
- Then resolves bearing to target and writes facing. Sets `ctx[+0x6D8] = 0x14` (frame timer).
- For party (`actor_id < 3`), looks up the spell-name string via `&DAT_800754D0 + actor[+0x1DF]*0xC`, computes centered X for HUD, writes `_DAT_80077332/+0x33A/+0x344/+0x352/+0x35C` (HUD label slots), fires UI element `FUN_801D8DE8(0x4C, 0)` (spell label).
- If the spell's first table byte is `'c'` (capture-class spell) → `ctx[7] = 0x6E` (capture path) + queues capture archive load via `func_0x8003EC70`.
- Reads MP cost from `&DAT_800754D0 + spell_id*0xC + 3` (entry +3); reduces it by half (`cost - cost>>1`) if the character's ability bitmask has `0x20` ("MP-half"), else by a quarter (`cost - cost>>2`) if `0x10` ("MP-quarter") - `0x20` is tested first and wins when both are set (`0x801E3D0C`). Subtracts from `actor[+0x150]` (MP).

## Inner dispatch - actor action category

Read once at `ctx[7] == 0x0C`, the byte `actor[+0x1DE]` selects the action category and seeds `ctx[7]`. The actor pointer is `(&DAT_801C9370)[ctx[+0x13]]` - i.e. the active battle actor.

| `actor[+0x1DE]` | Action category | Initial `ctx[7]` | Notes |
|---|---|---|---|
| `0` | **Martial Arts (Tactical Arts)** | `0x50` (skip - UI inputs handle the chain) | Sets `ctx[+0x6D0/+0x6D1] = (0, 8)` (UI cursor anchor), `ctx[+0xD] = 0`. The tactical-arts directional input is run by a separate flow before this state machine; by the time `ctx[7]` reaches `0x0C`, the chain is recorded and the action is "done" for this driver. |
| `1` | **Item** | `0x3C` (default) or `0x28` if RNG-conditional check (item byte at `+0x1DF + 0x68U < 2`) | Items branch into the same path Magic uses, except the lookup table pivots from spell-table to item-table. Item-class capture (Amulet) hits the `'c'` branch in `0x28` and routes to `0x6E` (capture path). |
| `2` | **Magic** | `0x28` (default) or `0x3C` if low-tier magic | Discriminator: `*(byte *)(actor[+0x1DF] * 0xC + -0x7FF8AB38) > 0x13` OR `actor[+0x1DF] > 100` → fall through to attack-style (`0x3C` via `LAB_801E2F24`) for status spells. Standard offensive magic hits `0x28`. |
| `3` | **Attack** | `0x14` | Sets `ctx[+0x6DA/+0x6DB] = (0, 2)` (combo timer). For party_id < 3, sets `actor[+0x20] = +0x1DE` and fires `FUN_801D8DE8(7, 0)` + `actor[+0x18] = 7` (UI weapon-slash element). |
| `4` | **Spirit (Originals)** | `0x46` | Sets `_DAT_80076D7E = actor[+0x154] - 6` (or `((actor[+0x156] * 7) / 5) + 8` capped at 0x120 if `actor[+0x1F9] != 0`). Fires `FUN_801D8DE8(0xF, 0)` + `0x52` (damage popup), bumps `actor[+0x19]++`. |
| `5` | **Run / Defend** | `0x64` (party) or `0x68` (monster) | Party run: rotates screen `_DAT_8007B792 += DAT_1F800393 * -2`, fires `FUN_801D5854(0, 9)` (defeat pose). Monster run hits the capture path at `0x68`. Either path resets `_DAT_801F69D0 = 0` (counter-attack flag). |

## Per-actor sub-state surface

Beyond `actor[+0x1DE]` (category), these per-actor bytes are read or written by `FUN_801E295C`:

| Offset | Type | Use |
|---|---|---|
| `+0x14C` | u16 | **Live HP** (`+0x14E` is max HP). Doubles as the liveness flag - every state's "is target valid" check is `+0x14C != 0`. Paired with the displayed-HP mirror `+0x172`; see [the `0x51` exit gate](#the-0x51-exit-gate-and-the-hp-bar-settle-invariant). |
| `+0x16E` | u16 | Per-actor flag bank. Bit `0x4` = "non-targetable", bit `0x380` = "AI-controlled", bit `0x404` = "AI + non-targetable". Read at state-`0x0C` to decide between `FUN_801EED1C` and `FUN_801E7320`. |
| `+0x10` | i32 | **Pending HP-bar delta** - how much `+0x172` still has to move. Ramped into the bar a quarter at a time by `FUN_80047430`, and only while it is non-zero. |
| `+0x172`/`+0x174` | u16 | **Displayed** HP / MP - the values the HUD bars draw, lagging live HP `+0x14C` and live MP `+0x150`. `FUN_80047430` ramps `+0x172` by the `+0x10` accumulator and `+0x174` by `+0x178`, in the same quarter-step shape. |
| `+0x178` | u16 | Last-action MP cost (used to display `-N MP` on screen). |
| `+0x1A` | u8 | Party-action queue counter. Incremented by `0x0` (action-begin), `0x1E` (counter-attack swap), `0x64` (run advance), `0x5A` (next-actor). |
| `+0x1D9` | u8 | **Current** anim ID (read-only here; written by the animation system). |
| `+0x1DA` | u8 | **Queued** next anim ID. The state machine writes this; the animation system reads `+0x1D9` toward `+0x1DA`. |
| `+0x1DC` | u8 | Per-actor flag bits. `0x01` = "windup done", `0x02` = "advance done", `0x04` = "exit". Set by the strike/spell loops. |
| `+0x1DD` | u8 | Active-target slot index (used by Magic / Item to retarget mid-chain). |
| `+0x1DE` | u8 | **Action category** (the inner-dispatch key - see above). |
| `+0x1DF..+0x1F2` | u8 × N | Per-action parameter byte stream (item ID / spell ID / strike-anim list). The **attack band terminates on `0x00`**, the magic band on `0xFF` (`-1`). Read sequentially via `actor[+0x1DF + actor[+0x15]]`. For a party attack the bytes are direction-command swings `0x0C..0x0F`, art starters `0x19`/`0x1A`, and art action constants `0x1B+` (seeded by `FUN_801EED1C`); for a monster they are entry indices from the AI picker. |
| `+0x1F5` | u8 | Anim-cue flag (read at state `0x33` for fade-in trigger). |
| `+0x1F9` | u8 | "Spirit shield" flag - gates spirit-arts variant path. Written by `FUN_800402F4` case 5 (set) / case 4 (cleanse clears), selected by `actor[+0x1E8]` seeded from the spell-table class byte (`DAT_800754C8 +0`, `5` = shield / `4` = cleanse). |
| `+0x1FA` | u8 | Spell-cast iteration counter. |
| `+0x21B` | u8 | Hit-count bound (script-defined; loop exits at `ctx[+0x24C] >= +0x21B`). |
| `+0x21C` | u8 | Per-actor render flag - `0xFF` while hidden by summon fade, `0x02` while captured, `0` otherwise. |
| `+0x21D` | u8 | Impact-step magnitude - multiplied into the per-frame X/Z drift during attacks. |
| `+0x224` | u8 | "Action recoil" magnitude - written by `0x50`. |
| `+0x225` | u8 | Capture state byte - `2` while captured. |
| `+0x46` | u16 | Facing angle (i12 in 0xFFF range; written from bearing checks). |
| `+0x6D6` (ctx) | u16 | The state machine's "PC offset" cursor - `_DAT_8007BD24 + 0x6D6` is the per-action ramp target. |
| `+0x6D8`/`+0x6D9` (ctx) | i16 | Frame countdown timer. Decremented by `DAT_1F800393` (frame dt) every state that needs to wait. |
| `+0x6DA`/`+0x6DB` (ctx) | i16 | Combo / sub-timer (separate from `+0x6D8`). |
| `+0x6DC`/`+0x6DE` (ctx) | i16 × 2 | Damage-target / HP-bar target values for the spirit-arts ramp (`0x47`/`0x40`). |
| `+0x6E6 + i*2` (ctx) | u16 × 8 | Per-actor facing offsets (one per slot 0..7). Written by `0x14` for AI bookkeeping. |
| `+0x890..+0x893` (ctx) | u8 × 4 | 4-byte fade-back sentinel `84 10 42 08`. Written by the summon `0x37` and capture `0x71` paths. |
| `+0x102C`/`+0x1080`/`+0x1074` (ctx) | int* | Scratch pointers to live UI widgets (fade primitive, HP-bar, damage-popup). |

## The `0x51` exit gate and the HP-bar settle invariant

State `0x51` (done / fade-down) leaves the action band only when
`ctx[+0x6D8] < 0 && ctx[+0x276] == 0`, and state `0x50` seeds that countdown
with `0x3C`. The countdown is **not** decremented unconditionally - the arm
gates it on a call:

```
801e6044  jal  0x801e7250          ; HP-bar settle check
801e604c  bne  v0,zero,0x801e60b8  ; "not settled" -> branch PAST the decrement
801e6054  lh   v0,0x2(s7)          ; s7+2 is ctx+0x6D8
801e6068  lbu  v0,0x393(v0)        ; DAT_1F800393, the per-frame delta
801e6070  subu a0,v1,v0
801e6074  sh   a0,0x2(s7)          ; ctx+0x6D8 -= delta
```

The branch target `0x801E60B8` rejoins after the store, so a "not settled"
answer skips the store and nothing else. A park at `0x51` with `ctx[+0x6D8]`
holding exactly the `0x3C` that `0x50` seeded, `ctx[+0x276] == 0` and a healthy
`DAT_1F800393` is therefore neither a stalled effect child, nor a zero frame
delta, nor a pinned census. The state machine is still being entered on its
normal cadence (once per game frame, which `DAT_1F800393 = 3` makes roughly one
vsync in three) and the `0x51` arm still reaches the `jal` - `FUN_801E7250` is
simply answering "not settled" every time.

### What `FUN_801E7250` measures

52 instructions; see `ghidra/scripts/funcs/overlay_battle_action_801e7250.txt`.
It reads the acting actor's active-target slot `actor[+0x1DD]` and branches on
the target class:

| `actor[+0x1DD]` | Result |
|---|---|
| `0`–`2` | 1 ("not settled") when that party actor's live HP `+0x14C` differs from its displayed HP `+0x172`; else 0. |
| `3`–`7` | 0 immediately - a monster target can never hold the exit. |
| `8` (all) | 1 when **any** slot below `ctx[+0x00]` has `+0x14C != +0x172`; else 0. |
| `> 8` | 0. |

Only party-side slots are ever inspected, in either arm. An action aimed at a
monster clears the gate on the frame it is asked, whatever the enemy's mirror is
doing; an action aimed at the party - an enemy attack, a heal, any all-target
cast - is the only kind that can be held. `ctx[+0x00]` is the party member
count, so the all-target arm is a party-side scan too.

### Why only the party side has anything to wait for

Retail draws **no HP readout for monsters**. The party HUD counts its HP down
over several frames after a hit; an enemy's HP is never shown at all. That is
what the whole asymmetry is for, and it explains three things that otherwise
look arbitrary:

- `+0x172` is maintained for monster slots but never drawn. `FUN_80047430`'s
  non-party arm therefore does not animate: it applies the entire delta in one
  frame and clears the accumulator (`0x80047578`). There is no readout to ramp.
- The settle gate returns 0 for monster targets because there is no animation to
  wait on - the wait exists purely to let the party's readout finish counting
  before the action ends.
- The same arm's `lhu` read of the signed accumulator, and the non-party MP
  path's use of the HP fields (see below), are unobservable in retail precisely
  because nothing renders the values they corrupt.

So "HP bar" is shorthand throughout this page for the **displayed-HP mirror**
`+0x172`, which is a drawn readout only on the party side. Readers of `+0x172`
in the corpus are UI-side: `FUN_80046A20` (`0x80046AA8`) and the
`FUN_801D8DE8` UI-element family (`0x801D9758`).

### The invariant the check assumes

Live HP `+0x14C` and displayed HP `+0x172` converge through a third field,
`actor[+0x10]`, a signed pending-delta accumulator. The per-actor tick
`FUN_80047430` (SCUS band, `see ghidra/scripts/funcs/80047430.txt`) drains it
into the bar. A **party** slot gets a quarter per game frame - `0x172 -= step`
and `acc -= step`, with `step` a divide-by-four biased so it is never zero for a
non-zero accumulator (`(acc+3)>>2` positive, `acc>>2` negative) - so the total
bar movement equals the seeded accumulator exactly and the sequence terminates
at zero for either sign. A **monster** slot instead takes the whole delta in one
frame and clears the accumulator (`0x80047578`), which is a second reason a
monster target never holds the `0x51` exit.

The whole ramp sits behind one guard at `0x800474E8`
(`lw a0,0x10(s2); beq a0,zero,<skip>`): **with a zero accumulator the bar is not
touched at all.**

Two retail quirks live in the non-party arms and are unobservable because
nothing draws a monster's readout. The HP arm reads the signed accumulator with
`lhu` (`0x8004757C`), so a negative accumulator on a monster - a heal - wraps
through the low halfword. And the non-party **MP** arm at `0x80047624` operates
on `+0x172` / `+0x10`, the **HP** fields, rather than `+0x174` / `+0x178`: a
copy-paste of the HP arm. The consequence is that a monster's MP accumulator
`+0x178` is never cleared, so that branch re-runs every frame for the rest of
the battle, subtracting an already-zeroed HP accumulator. Both are faithful
behaviour for a port to reproduce, not defects to correct.

That makes `+0x14C != +0x172` with `+0x10 == 0` on a party slot an **absorbing
state** for as long as the actor takes ordinary hits: the drain is the only
thing that moves the bar, and a subsequent damage or heal adds its own delta to
both sides so the constant offset rides along. One path does re-derive the bar
from live HP - the per-round status ticker
[`FUN_801E752C`](#fun_801e752c---per-round-status-dot-ticker) force-assigns
`+0x172 = +0x14C` right after its own HP write (`0x801E7600` and `0x801E7698`,
one per status bit) - so a poison or regen tick on the affected actor clears the
mismatch. That is the only re-sync in the dumped battle corpus, and it explains
why the softlock is survivable rather than terminal for a statused party.
Absent it, every action that targets the
party side reaches `0x51`, is told "not settled", and never decrements its
countdown. The battle camera's idle azimuth sweep (`FUN_801D0748` stepping
`_DAT_8007B792`) runs unconditionally and never consults the state machine, so
the visible result is a battle that keeps orbiting the acting actor forever -
an endless-camera-orbit softlock with no other symptom.

The park is directly reproducible: offsetting one party slot's `+0x172` by a
single point while clearing its `+0x10` is enough, and the next party-targeted
action hangs while monster-targeted actions in between still complete normally.
Probe: `scripts/pcsx-redux/autorun_gaza2_hpbar_settle.lua`.

### Phased crediting: the invariant breaks mid-action by design

A multi-strike action does not apply its damage once. The unsafe kernel
credits the bar accumulator **per strike** - each strike adds the same delta
`a0 = s0 - s1` to the per-action damage total `actor[+0x00]` (`0x801EDB40`)
and to the accumulator `+0x10` (`0x801EDB58`), paired stores off one register -
while live HP is committed **once, at the end of the resolution**, from the
accumulated total (`0x801EEA10`). Between the first credit and the commit the
readout is draining toward damage that live HP does not yet show, so a
watchpoint sees `+0x14C != +0x172` with `+0x10 == 0` - the absorbing shape -
**transiently, inside the action, as normal behaviour**. At the commit the
total that live HP absorbs equals the sum the bar was credited, and the pair
reconciles; the state `0x51` settle wait then holds the action open until the
tail of the drain lands. Measured end to end on the Gaza 2 save
(`autorun_gaza2_acc_discard.lua`, `invariant.csv` / `acc_writes.csv`): a
three-strike physical resolved as bar credits of 338 + 344 + 304 drained
per-strike, one live-HP commit of 986 at the end, and a bar that landed
exactly on live HP.

Two prior observations are re-attributed by this. The "party slot holding
live HP 266 with the bar drawing 0" capture that used to sit under the clamp
asymmetry below is a phased mid-action state - the window closed with a death
commit ~90 vsyncs later. And a mid-action watchpoint that samples between
strike and commit will always find "absorbing" shapes on healthy fights;
only a mismatch that **survives the action's own commit and settle wait** is
a real desync. No such survivor has been captured from retail-only play -
see [the instrumentation section](#consequences-for-instrumentation) for the
measured campaign.

### Where the desync comes from: two seeding conventions

Every writer of `+0x10` in the dumped battle corpus follows one of two
conventions, and they disagree about what to do when a new delta arrives while
the bar is still moving.

**The battle damage/heal kernel `FUN_801EC3E4` accumulates at every one of its
sites** (`see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`):

| store | shape | branch |
|---|---|---|
| `0x801EDAF0` | `acc -= (max - hp)` | overheal - live HP saturates at `+0x14E` first, so only the amount actually applied is credited |
| `0x801EDB14` | `acc -= (s0 - s1)` | ordinary net delta, paired with the live-HP write at `0x801EDAFC` |
| `0x801EDB58` | `acc += (s0 - s1)` | the second actor the same hit credits |
| `0x801EDB7C` | `acc = bar` | anti-overkill clamp, guarded `if (bar < acc)` at `0x801EDB70` - caps the drain at the whole visible bar |

Each is a read-modify-write, so overlapping hits compose and the invariant
`(+0x172 - +0x14C) == +0x10` survives every path through the damage kernel.

**The item / restore applier `FUN_800402F4` assigns.** Its head builds a pointer
table over `&actor[+0x14C]`, `+0x14E`, `+0x150`, `+0x152` for slots `0..6`
(battle mode `0x15`; a different source table otherwise), applies the restore
with `hp = hp + amount` at `0x800408AC`, and then seeds the bar with a bare
store:

```
800408f0  lw   v1,0x0(v1)      ; v1 = the actor
800408f4  subu v0,zero,v0      ; v0 = -amount
800408f8  jal  0x801e22c8
800408fc  _sw  v0,0x10(v1)     ; actor[+0x10] = -amount   <- the old value is never read
```

All three of its seeds - `0x800408FC`, `0x80040D28`, `0x800410BC` - are that
same shape. Because none of them reads the old accumulator, **a restore that
lands while a damage drain is still in flight discards the remainder.** The rest
is forced arithmetic: with live HP `L`, bar `D` and remainder `A = D - L`, a
restore of `H` leaves live HP at `L + H` and ramps the bar from `D` to `D + H`,
so the bar settles exactly `A` above live HP with the accumulator back at zero.
That is the absorbing state above, reached through nothing but ordinary game
actions.

So the retail trigger is **healing a party member whose HP readout has not
finished counting down from a recent hit** - and the residual desync is exactly
the amount of readout movement the heal cancelled. Every later action whose
acting actor targets that slot (`+0x1DD` in `0..2`) or the whole party
(`+0x1DD == 8`, which is what a party-wide spell uses) then parks at `0x51`.

#### The auto-revive reaches the assigning seed by itself, one state early

The chain does not need a player to time an item badly, because state `0x50`
does it unprompted. `FUN_801E6968` - the Lost Grail **Final Heal** auto-revive
that `0x50` runs - calls `FUN_800402F4` twice, at `0x801E6A24` and `0x801E6BD0`,
both with `a0 = 4, a1 = 1`: **effect class 4** (revive) at tier 1 (full).
Class 4 dispatches through the applier's jump table at `0x80014FA0` into the
revive arm at `0x80040F14`, and that arm's accumulator seed - `0x800410BC` - is
one of the three bare assigns. Each call is guarded by
`lhu v0,0x14c(<actor>); bne v0,zero,<skip>`, so it fires only on a member whose
live HP has just reached zero.

On paper that is the worst possible moment: if the readout is still mid-drop
when the revive lands, the assign discards whatever is left of it, the bar
settles above live HP, and `0x50`'s only successor is `0x51` - the state that
asks whether the readout has settled. The park would land on the very action
that triggered the revive.

Measured, the race is **starved on the Gaza 2 fight**. The probe
`scripts/pcsx-redux/autorun_gaza2_acc_discard.lua` arms an Exec breakpoint on
every `+0x10` writer in the corpus plus the two Final Heal call sites, and a
capture campaign on the Gaza 2 save (Lost Grail armed on the party, no
harness write touching any HP / readout / accumulator field) drove **twelve**
auto-revives through `FUN_801E6968` across single-target and party-wide,
cast-path and kernel-path kills. **Every assign landed on an accumulator
already drained to zero**, margins 143-280 vsyncs. The starvation is
structural on this move set: [phased crediting](#phased-crediting-the-invariant-breaks-mid-action-by-design)
lands the bar credits per strike, *early* in the resolution, while `0x50`
arrives only after the remaining targets resolve and the effects tear down.

The margin is quantifiable from the same captures, and it is thin. Grouping
every party-side credit into actions and measuring last-credit to first
`0x51` settle check: minimum gap `90` vsyncs (~27 rendered frames at the
light-load 3-4 vsync cadence), median ~110-220. Against that, the biased
quarter-step (`acc -= (acc+3)>>2`) drains a full readout in a
size-insensitive ~20-30 frames - `600 -> 20`, `1289 -> 23`, `3000 -> 26`,
`9999 -> 30` (exact iteration of the retail step). A LV23 party (readouts
1289-1382, 23 frames) misses the fastest observed Gaza 2 tail by ~4 frames -
which is why twelve revives all came up clean - but the drain grows about one
frame per doubling while the action tail does not, and a `9999`-HP readout
(30 frames) crosses the fastest tail. **The prediction that falls out: the
discard-and-park fires for high-max-HP (late-game) parties killed by
fast-tailed moves, and cannot fire at low HP pools** - matching the
community's clustering of orbit reports on late-game bosses. Untested: it
needs a capture with a late-game-sized readout, and the frame-vs-vsync
clocking of the specific tail states (timed states compensate by
`DAT_1F800393`, animation waits do not) shifts the line by a few frames
either way.

The same arm is what a **Phoenix** (class 4) reaches from the battle item
menu, and the class 0 / class 1 heal arms reach the sibling assign at
`0x800408FC`. But note the shape of the gate itself: a party-targeted action
holds its own `0x51` open until every party readout settles, so **the drain a
menu restore could interrupt has always finished before the menu can act** -
the inter-action race is closed by the very wait this page documents. The
intra-action Final Heal is the one crack, and it is measured tight above.

### The clamp asymmetry: two overkill guards against different references

The corpus contains two ways to apply damage to a party actor, and they clamp
overkill against **different** values. This is the generator that needs no
restore and no timing race at all.

The **safe** shape - the enemy-cast damage applier at `0x801E1924`, reached from
the cast dispatch just above it (`jal 0x801DD0AC` at `0x801E188C`) - clamps the
**damage** first and then applies that one clamped value to both fields:

```
801e1924  lhu  a0,0x14c(v1)   ; live HP
801e192c  sltu v0,a0,a1       ; if (hp < damage)
801e1938  move a1,a0          ;     damage = hp          <- clamp the DAMAGE
801e1944  addu v0,v0,a1       ; acc += damage
801e1948  sw   v0,0x10(v1)
801e195c  subu v0,v0,a1       ; hp  -= damage
801e1960  sh   v0,0x14c(v1)
```

Both fields move by the same amount, so the invariant holds by construction.

The **unsafe** shape is `FUN_801EC3E4`, where the two fields are written at
different times against different references. The bar accumulator is credited
while the action resolves and is clamped against the **displayed bar**
(`if (bar < acc) acc = bar` at `0x801EDB70`), while live HP is committed only at
the end of the action, from the separate per-action damage total `actor[+0x00]`,
and is clamped against **live HP**:

```
801eea10  lhu  a0,0x14c(v1)   ; live HP
801eea14  lw   v0,0x0(v1)     ; the action's accumulated damage
801eea1c  sltu v0,v0,a0       ; if (damage < hp)
801eea2c  _sh  zero,0x14c(v1) ;     else hp = 0          <- clamp the HP
801eea38  subu v0,a0,v0       ;     hp -= damage
801eea3c  sh   v0,0x14c(v1)
801eea74  sw   zero,0x0(v1)   ; damage total cleared
```

The two clamps agree only while `+0x172 == +0x14C` **at the action's start** -
and the `0x51` settle wait of the previous party-targeted action guarantees
exactly that. From a synced start the arithmetic is forced to agree: the
bar-side clamp trips only when the credited sum exceeds the starting bar,
which from a synced start means it exceeds starting live HP too, so the HP
commit floors at `0` on the same action and both readings land at zero
together (a kill, consistent). The asymmetry is therefore an **amplifier of a
pre-existing offset, not a standalone generator**: it needs `+0x172` already
below `+0x14C` when the action begins, which is the very desync whose origin
is in question. The earlier reading of this section - "reachable from
ordinary damage, no restore and no timing race" - is withdrawn; its
supporting capture (live HP `266`, bar `0`, zero accumulator on the Gaza 2
save) is a [phased mid-action state](#phased-crediting-the-invariant-breaks-mid-action-by-design)
that closed with a death commit, not a settled desync.

Three further guards above the commit (`0x801EE988`, `0x801EE9AC`, `0x801EE9EC`)
branch to `0x801EEB5C` / `0x801EEB60` and skip the live-HP write entirely. A
skip that fires with the bar accumulator already credited would move the bar
without moving live HP - a real credit-without-commit generator if any retail
path reaches it. No capture has caught one firing that way yet; which action
classes route through those guards is the open question this thread reduces
to.

Probe: `scripts/pcsx-redux/autorun_gaza2_hpbar_writers.lua` puts Write
watchpoints on each party actor's `+0x00` / `+0x10` / `+0x14C` / `+0x172` rather
than guessing store addresses, and Exec breakpoints on the commit and its skip
exits, so the writers name themselves and the pairing is auditable per frame.

### Consequences for instrumentation

Any intervention that force-writes `+0x14C` without re-seeding `+0x10` -
a capture-harness HP clamp, a max-HP cheat code, an engine debug key -
manufactures this park by construction. The failure shape is worth naming
because it is easy to mistake for the retail bug: the clamp restores live HP on
the frame damage lands, the in-flight accumulator keeps draining the bar
downward, the ramp then stops at zero accumulator, and the bar is left short of
a live HP that the clamp holds pinned at maximum. A "reproduction" captured
under such a clamp is measuring the instrument. A clamp that also assigns
`+0x172` and zeroes `+0x10` keeps the invariant intact.

Two further instrumentation facts, measured on the Gaza 2 save:

- **The stat aggregator does not tick during battle.** Poking an equipment id
  into a character record mid-battle never reaches the ability bitfield -
  `FUN_80042558` runs again only on isolated menu-side paths (observed twice
  in ~48k captured vsyncs). To arm an equipment-derived battle behaviour (the
  Lost Grail Final Heal bit `+0xF8 & 0x80`) from a save already inside the
  battle, seed the bit once alongside the equipment id - that mirrors what
  pre-battle aggregation of the same equipment would have left. The
  aggregation-derived bit is also cleared when the revive consumes the grail
  and any aggregator pass rebuilds the field, so re-arming between deaths is
  part of the same mirroring.
- **The watchpoint shape that matters is action-scoped, not frame-scoped.**
  Because of [phased crediting](#phased-crediting-the-invariant-breaks-mid-action-by-design),
  per-frame sampling flags absorbing shapes on healthy fights. The probe
  `autorun_gaza2_acc_discard.lua` therefore keys its verdicts on the two
  events that survive phasing: an assigning store landing on a non-zero
  accumulator (`discards.csv`), and a `0x51` settle verdict streak with a
  frozen `ctx[+0x6D8]` (`settle.csv`). A campaign of three such captures
  (~84k vsyncs, twelve Final Heal revives, one menu heal, no harness write to
  any HP / readout / accumulator field) produced zero of either.

## Cross-references with other battle helpers

### `FUN_8004E2F0` - battle range / line-of-sight

Called from states `0x14`, `0x16`, `0x19` (during the attack chain). Returns a 16-bit distance metric. The state machine treats `0` as "in range" and any non-zero as "still approaching," which keeps `0x16` running its sin/cos-LUT advance loop until the gap closes. Cited in [battle.md](battle.md). Definition in `ghidra/scripts/funcs/8004e2f0.txt`.

### `FUN_80042558` - per-frame stat aggregator

Not called *directly* from `FUN_801E295C`, but the global ability bitmask it maintains (4×u32 at `0x80074358..0x80074368`) is read indirectly here:

- State `0x28` reduces MP cost by half (bit `0x20`, `cost - cost>>1`) or by a quarter (bit `0x10`, `cost - cost>>2`) based on the character record bits - `0x20` takes priority when both are set (the bit indices match the bitmask layout `FUN_80042558` populates).
- States `0x1E` (attack drift) and `0x46` (spirit-arts HP-bar) read character record bits `0x100`/`0x200` for impact-magnitude scaling.

The bitmask is cited via `*(uint *)(((byte)(&DAT_8007BD10)[ctx[+0x13]] - 1) * 0x414 + -0x7FF7B804)` - i.e. the active character's record at `0x80084708 + (party_id - 1) * 0x414 + 0xF4`, which is exactly the per-character `+0xF4..0x100` block that `FUN_80042558` OR-aggregates into the global bitmask.

**Field map (character record `0x80084708 + n*0x414`, `n = 0..2`), from
`ghidra/scripts/funcs/80042558.txt`.** Every field `FUN_80042558` reads or writes lives in the
record's `+0xF4..+0x13D` region:

| Offset | Field |
|---|---|
| `+0xF4..0x103` | 128-bit ability/passive bitfield (4×u32). Cleared, then each active passive sets bit `index` (`index < 0x40`); also OR-aggregated into the globals `DAT_80074358..0x80074364`. |
| `+0x104..0x11B` | Effective (passive-boosted, capped) stat block, seeded from `+0x11C..0x12D`. `+0x104` HP (cap `9999`), `+0x108` MP (cap `999`), `+0x10C` (cap `100`), `+0x110` AGL-class (cap `0x118` = 280), `+0x112/0x114/0x116/0x118/0x11A` combat stats (cap `999`); `+0x106/0x10A/0x10E` are running-minimum companions. |
| `+0x11C..0x12D` | Base (unmodified) stat block - the source the effective block is rebuilt from each frame. |
| `+0x13C` | Count of learned Seru/ability entries. |
| `+0x13D..` | That many ability/Seru id bytes (ids `0x99..0xA0` handled). |
| `+0x196..0x19D` | 8 equipped-item ids; each item's descriptor (`kind==1`→equip-bonus `+5`, `kind==2`→item-effect `+3`) supplies the passive index bit set in `+0xF4`. |

The percent boosts applied per ability bit are the accessory-passive magnitudes (`+10%` = base/10,
`+25%` = base>>2, `+20%` = base/5; see [accessory-passive-table.md](../formats/accessory-passive-table.md)).

**Scope correction (do not re-walk).** `FUN_80042558` touches **only** this character-record
`+0xF4..+0x13D` block. It does **not** write the *battle-actor runtime* struct (the
`DAT_801C9370` actor pool) - `actor[+0x14C]` (HP), `actor[+0x150]` (MP) and `actor[+0x176]` are
runtime fields written by the battle loader and this action SM (`FUN_801E295C`), a different
struct. The earlier attribution of `+0x14C..+0x176` to `FUN_80042558` is wrong.

### `FUN_801E752C` - per-round status DoT ticker

Not an SM state: the round driver `FUN_801D0748` calls it once per round (state
`0x14`, gated on the round counter `ctx[+0x28A] != 0`, so the first round never
ticks). Applies the Venom / Toxic HP drains off the `+0x16E` status bits -
exact arithmetic, caps, and the never-kill clamp in
[battle-formulas.md](battle-formulas.md#per-round-status-dot-ticker---fun_801e752c);
ported as `engine-vm::status_effects::toxic_tick_damage` / `venom_tick_damage`
(`StatusEffectTracker::tick_actor`). The same walk pays the Life Grail / Magic
Grail per-round recoveries for party slots.

### `FUN_801DD4B0` / `FUN_801DD6B4` - the two per-move damage-roll wrappers

Two sibling **damage kernels** the action path calls to resolve one hit. Each
draws an attacker roll and a defender roll from the two battle-actor records
(`(&DAT_801C9370)[slot]`), calls the affinity scale `FUN_801DD864` and then the
closed-form finisher `FUN_801DDB30`, and returns `attacker_roll - defender_roll`
as the net damage. They differ in two ways: `FUN_801DD4B0` mixes the physical
attack/defence stat `+0x168` into both rolls and passes finisher **`param_5 = 0`**
(the equipment resist ladder - jewels / elemental guards / All Guard - runs);
`FUN_801DD6B4` uses the spell-power stat `+0x158` and passes **`param_5 = 1`**,
which makes the finisher **skip the whole party-defender resist block**. The
`param_5 = 1` path is the **resist-BYPASS** wrapper: a hit routed through it
takes no Earth/Luminous-Jewel or All-Guard reduction even when the defender is
elementally warded, which is the mechanism behind the non-elemental capture-class
boss casts (Bloody Horns / Terio Punch). The affinity scale still reads the
caster's slot element either way, so the attacker's element is applied - only the
defender's jewel stage is dropped. Full stat fields, the finisher stage list, the
per-spell module census, and the engine mirror (`damage_finish::bypass_party_resist`)
are in
[battle-formulas.md § Summon-magic damage roll](battle-formulas.md#summon-magic-damage-roll---fun_801dd0ac).
See
`ghidra/scripts/funcs/overlay_battle_action_801dd4b0.txt` /
`_801dd6b4.txt`.

### AI-delegated (`0x380`) party members - what is and isn't pinned

`FUN_80047430` sets `actor[+0x16E] |= 0x380` each frame on a party slot whose
character record carries ability-bitfield bit 45 (`+0xF8 & 0x2000` = accessory
passive `0x2D` Rage / Evil Medallion - the neighbouring bits byte-match the
[accessory-passive index table](../formats/accessory-passive-table.md): `0x100`/
`0x200` = AP Boost, `0x800` = AP Used Down, `0x20`/`0x40` = HP/MP After). The
SM and the next-actor selector treat the bits as "AI-controlled", but the code
that *chooses* an action for a delegated **party** member is not in the dumped
corpus: the round driver `FUN_801D0748` routes party slots to the player
command menu with no `0x380` test, `FUN_801DABA4` calls the AI picker
(`FUN_801E9FD4`, fully dumped + ported as `engine-core::monster_ai`) only for
monster slots (`a0 = active_index - 3`, gated `active_index >= 3` at
`0x801DAEF8`), and `FUN_801EED1C`'s auto-fight block is keyed to **character id
4** (`(&DAT_8007BD10)[slot] == 4`; `DAT_8007BD10[slot]` is the per-slot roster
character id - byte-confirmed `01 02 03` = Vahn/Noa/Gala in
`evil_medallion_rage_battle` - and the block indexes the live char record
`(id-1)*0x414 + 0x800847FC`), **not** to `0x380`. So char id 4 is the
AI-controlled companion (Terra in the retail roster), not a Rage delegate.
Pinning the **Rage** party-side auto-pick *writer* (does it cast? does the
pattern vary?) still needs a runtime capture watching the writers of
`actor[+0x1DE]`/`+0x1DD` during the command phase - the `0x380` flag is consumed
in **only** three dumped battle functions (`FUN_801E295C`, `FUN_801E9FD4`,
`FUN_801DABA4`) plus the charm redirect `FUN_801E7320`, none of which fills an
arts-combo stream for a controllable character, so the captured arts combo below
was set upstream (the command-menu controller, undumped). The monster-side
confuse behaviour *is* pinned (picker `& 0x380` guards + `FUN_801E7320` retarget
at ActionSeed).

**Char-id-4 (Terra) auto-AI pick - pinned.** `FUN_801EED1C`'s `== 4` block
chooses by the actor's special gauge (`+0x14C` current / `+0x14E` max; seeded to
`0xC8` when `DAT_8007BD11 == 4`) and status word (`+0x16E`):

| condition | category `+0x1DE` | detail | writer PC |
|---|---|---|---|
| `+0x14C == 0` | `2` (Magic) | spell id `0x16`, target 0 | `0x801EEE70` |
| `+0x14C < +0x14E >> 1` | `2` (Magic) | spell id `0x0D` | `0x801EEEAC` |
| healthy, `+0x16E != 0` (statused) | `2` (Magic) | spell id `0x11` | `0x801EEEE0` |
| healthy, no status | `3` (Attack) or none | `rand()&1`: 50% Attack with a 1-2 hit directional stream (`0x0C`/`0x0D`/`0x0E`), else category 0 (no action) | `0x801EEF28` |

The spell id lands in `+0x1DF` (`0x801EEEF8`) and `+0x1E7 = 9` (`0x801EEF00`).
So the AI companion *does* vary its pick (magic when low/statused, else a coin-flip
between a short physical and standing by); it is not a flat auto-attack. This
branch has no engine consumer yet (Terra joins past the playable slice), so it is
documented but unported.

**One delegated pick is now observed** (`evil_medallion_rage_battle`; disc +
library gated `rage_delegated_pick`). In the battle-actor pool, exactly the
Evil-Medallion wearer carries the delegation bits `+0x16E & 0x380 == 0x380`
(the other party slots read `+0x16E == 0`), and its already-resolved pick is
category `+0x1DE == 3` (Attack) with the `+0x1DF` action stream
`[0x22,0x26,0x25,0x22,0x21]` - a five-element multi-strike, not a single plain
attack. Two qualifications: (a) within the **battle-actor** struct the `+0xF8`
bit `0x2000` is set on every party slot at this instant, so there it is not the
per-actor delegation discriminator - `+0x16E & 0x380` is (the
`FUN_80047430`/`+0xF8 & 0x2000` relation above is on the **character record**,
a different struct); (b) this is a single sample, so the engine's auto-physical
stand-in stays a stand-in - the writer and the pick variability are still open.

**`FUN_801F0450` - the auto arts-combo assembler (a candidate writer).** A REAL
928-instruction battle-action body (`--explain 801f0450` => `REAL`, entry
`801f0450`, `jr ra`) that, for each party slot with `ctx[+0x266 + slot] != 0`
and action category `actor[+0x1DE] == 3` (Attack), **fills the `actor[+0x1DF+n]`
arts-command stream** the strike loop later walks - the AI-side counterpart to
the player queue-builder `FUN_801EED1C`. It scans the per-command arm entries of
the arts-command table `DAT_801C9360[slot][cmd]` (`cmd` `0xC..=0xF`), reads each
command's AP cost byte `+0x74` (the same [arts AP-gauge cost](arts-command-gauge.md)
the randomizer edits), builds a weighted candidate pool - dropping any command
whose per-command elemental/status guard mask `DAT_801F672C[cmd-0xC]` collides
with the target's status word `actor[+0x16E]` - then draws from the pool with
`func_0x80056798` (RNG) until the actor's special gauge `actor[+0x154]` no longer
covers the cheapest command, halving the budget when the AP-Used-Down passive bit
`0x800` is set. It is the natural producer of the observed delegated
`[0x22,0x26,0x25,0x22,0x21]` multi-strike, but the outer gate keys on `+0xF8 &
0x2000` / `+0x16E & 0x404` rather than the `0x380` delegation bits directly, so
confirming it is *the* Rage-delegate path (versus a shared auto-fight assembler)
still wants the runtime `actor[+0x1DF]`-writer capture this section calls for. No
engine consumer yet; documented, unported. See
`ghidra/scripts/funcs/overlay_battle_action_801f0450.txt`.

### Enemy AGL action-budget (`FUN_801E9FD4`)

The monster AI picker `FUN_801E9FD4` (fully dumped + ported as
`engine-core::monster_ai`) queues **more than one action per turn** out of an
AGL-scaled budget - the enemy analogue of the party's [Arts command
gauge](arts-command-gauge.md). Its physical branch fills the actor's
action-parameter byte stream at `actor[+0x1DF..]` by repeatedly rolling
candidate moves (each candidate's tag byte at `+0x00` in the `0x0C..0x1F`
command band, its cost the same `+0x74` swing-record byte the party gauge
reads) and appending them while the budget holds. The budget is the per-round
**AGL gauge** at `actor[+0x154]`, seeded from the monster record's AGL
(`+0x0E`) and reset to base at the start of each round by `FUN_801D88CC`; each
appended action debits the move's cost. The fill is bounded at 15 queued
actions and 16 failed candidate rolls so a low-cost roster can't loop forever.
So an agile enemy takes several strikes per turn, the same "wide gauge = more
commands" mechanic the party's arm width drives ([arts-command-gauge.md
§ How the gauge consumes it](arts-command-gauge.md#how-the-gauge-consumes-it)).

### `FUN_801DFDF8` - effect-bundle public spawn API

`FUN_801E295C` does **not** call `FUN_801DFDF8` directly. Effect spawning happens through one of two indirections:

- **`FUN_801D8DE8(effect_id, mode)`** - the hottest battle utility (3 KB / 77 incoming refs), called 30+ times across the state machine. This is the wrapper that lays out a battle UI element (damage popup, weapon-slash trail, spell-icon banner, run-status banner, etc.) and internally schedules its visuals. Effect IDs surfaced in this function: `0x07` (party weapon-slash), `0x0F` (damage popup setup), `0x34` (Originals burst), `0x43` (run banner), `0x44` (terminate banner), `0x4C` (spell-name HUD), `0x4E`/`0x4F` (monster effect pair), `0x51` (combo continue), `0x52` (damage text), `0x59` (queue marker), `0x66` (counter-attack flash). The `mode` argument is `0` for "spawn / reset" and `1` for "terminate / unload."
- **`FUN_801DBF9C(party, spell_id)`** + **`FUN_801DC0A0(actor, anim_id)`** - chained from state `0x29` and `0x2A..0x2D` to drive spell visuals. These ultimately fan out to the [effect VM](effect-vm.md) which uses `FUN_801DFDF8` for the actual sprite-anim spawn.

So the dataflow is `FUN_801E295C` → `FUN_801D8DE8` / `FUN_801DBF9C` / `FUN_801DC0A0` → effect VM (`FUN_801DE914` / `FUN_801E0088`) → `FUN_801DFDF8`. The state machine never names an effect ID directly; it names *UI element* IDs which the effect VM resolves. Note this path drives the **2D UI/sprite** layer (`FUN_801DFDF8` emits `POLY_FT4` billboard quads into the effect pool); the 3D summon model is a separate mechanism (next).

### Seru-magic summon-overlay dispatch

The 3D visual of a player Seru-magic cast (the summoned Seru and its attack mesh - e.g. Gimard's *Burning Attack* flame) is **not** spawned by an opcode and does **not** live in `befect_data`. It is a **per-summon code overlay** paged in on demand. In outer state **case `0x29`**, when the queued action's spell id `actor[+0x1df]` is in the player Seru-magic block `0x81..0x8b`:

```c
_DAT_8007bd24[7] = 0x32;                                   // advance to the cast band
_DAT_8007ba2c = (&PTR_s_re_check_801f6734)[id - 0x81];     // per-summon effect-data pointer
FUN_8003ec70(id - 0x79, 0);                                // overlay loader B: PROT (id - 0x79 + 0x381)
```

`FUN_8003EC70(param)` (overlay loader B) loads `FUN_8003E8A8(param + 0x381)` into
`*DAT_80010390` (= `0x801F69D8`, above the resident battle overlay) - which in **extraction
index space is PROT entry `param + 0x37F`** (the resolver indexes the raw in-RAM `PROT.DAT`
head, 2 entries above extraction indexing; see [formats/prot.md § In-RAM
TOC](../formats/prot.md#in-ram-toc)). So the summons map to extraction **PROT 903..913** (Gimard
*Burning Attack* `0x81` → param `8` → **PROT 903**; the earlier "905..915 / Gimard → 905" reading was
this off-by-2 - the per-spell attribution below it was arithmetic-derived, never
content-pinned). **The Gimard leg is capture-pinned**: the loader-B current-id global
(`gp+0x934` = `0x8007BC4C`) reads `8` → extraction **PROT 903** in all three catalogued
player-Gimard cast states (`gimard_summon_start` / `_visible` / `_burning_attack` - the
value sits in the save-state RAM, no live probe needed), and stays `8` through the whole
cast; the **enemy** Gimard "Fire Tail" frames instead hold `5` → extraction **PROT 0900** (the
move-FX module). **Enemy boss specials ride their own stagers through the same loader**:
the catalogued final-boss corpus (six Cort mid-cast states) lands every leg on the same
linear arithmetic, byte-resident at slot B `0x801F69D8` - Mystic Circle `0x2B` → **938**,
Mystic Shield `0x2D` → **940**, Guilty Cross `0x31` → **944**, evolved-form Final Crisis
`0x42` → **961** and Ultra Charge `0x43` → **962**, and Cort's Evil Seru Magic `0x47` →
**966** - the last **distinct from the player-side Juggernaut stager 0927** (loader id
`0x20`): the player and enemy arms of the same spell ship separate stagers. So the
enemy-special id band `0x2B..0x47` maps to extraction **938..966**, while small ids
(`5`/`6` → 0900/0901) are the move-FX / widget modules streaming through the same slot. The
capture-class (`'c'`) spell branch loads from a different base:
`FUN_8003EC70(spell_record[+1] + 0x28)`. **The whole block is capture-pinned**: every spell
id `0x81..=0x8B` was observed mid-cast loading its arithmetic slot (`903..=913`), with zero
exceptions. PROT 0907 on the spell-`0x85` slot is **Nighto's stager** - its head title
"Hell's Music" is the attack's display name (the SCUS spell table carries the same string),
not a Disco King dance song (that reading is refuted: the dance overlay, 0980, contains no
slot-B loader callsite - its music is sequenced BGM). See
[`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md).

#### Inside a summon overlay (extraction PROT 905, decoded)

> The deep-dive below analyzes the **extraction-905 file** - under the corrected loader arithmetic that is the spell-`0x83` slot, *not* Gimard's (`0x81` → 903, which parses identically as a stager under the same link base, and is now capture-pinned as the Gimard load via the loader-B current-id in the catalogued cast states). The file-level findings stand for the 905 file itself; the live-capture findings (flame mesh `DAT_8007C018[26]`, part-actor motion) are capture-derived and independent.
> The per-spell file attributions for the whole block (`0x81..=0x8B` → `903..=913`) are capture-pinned from per-spell mid-cast states. **Parse counts quoted for any stager must come from the entry trimmed to its TOC-gap footprint** (see [the trim subsection below](#enemy-boss-stagers--the-record-table-trim)); untrimmed extraction files over-read into the neighbouring stagers and inflate the spawn-site/record census.

The summon overlay carries **no embedded TMD geometry** (no `0x80000002` magic). The summon's meshes are the separately-loaded `DAT_8007C018` model library: **PROT entry 871** (`etmd.dat`), a 30-entry `asset::pack` of Legaia TMDs that the battle scene loader `FUN_800520F0` pulls at battle init (debug index `0x367`, retail dev path `h:\prot\battle\etmd.dat`) and registers via `FUN_80026B4C`, populating `DAT_8007C018[3..32]` (`[0..2]` are the party battle meshes). Despite its CDNAME label `sound_data`, PROT 871 is the effect-model library; its texture sibling PROT 870 (a 256×256 flame-frame atlas, also `sound_data`) is loaded by a separate path. The overlay spawns and animates part-actors over those meshes. **Decompiled** (PROT 905 imported raw at base `0x801F0000`,
`ghidra/scripts/dump_summon_overlay.py`):

- The overlay spawns part-actors via the SCUS part-stager **`FUN_80021B04(world_pos, render_slots, record_ptr, 0x1000)`** (`param_1` = world position written to `actor[+0x14..0x18]`, `param_3` = a part record, allocated from the effect pool `DAT_8007062c`) - either directly, or through the thin pool wrapper **`FUN_80050ED4`** (stores the spawned actor pointer in the first free slot of the 0x60-pointer pool at `DAT_801C90F0`, then forwards the same arguments; the dominant call form in the high-summon and enemy boss stagers, `see ghidra/scripts/funcs/80050ed4.txt`).
  `record[+0]` (`model_sel`) drives the spawn-time render seat: `≥ 0` → library mesh `DAT_8007C018[model_sel + gp[0x754]]` (`actor[+0x5A] = 1`), any negative value (`-1` canonical) = no-mesh transform/pivot node (`actor[+0x56] = 0`, `actor[+0x5A] = 0`, draw-flag bit 2), `0x4000`/`0x4001` = special render-mode nodes (`actor[+0x5A] = 3` / `5`).
- Three staging functions drive the spawn: **`FUN_801F16A0`** (phase 0 = a `do { FUN_80021B04(...) } while(< 8)` loop spawning **8** flame parts, each with `rand()`-seeded actor params - `actor[+0x84]`, `actor[+0xb4] = rng%15 + 16`, `actor[+0xb6] = rng%255 + 512`, `actor[+0x28]`; phase 1 = 1 more part), **`FUN_801F36A0`**, **`FUN_801F4DD0`**. The per-frame motion is the standard actor-tick consuming those RNG-seeded fields.
- **Part records ARE in-file and move-VM bytecode (corrected link base).** Under the correct link base `0x801F69D8` (not `0x801F0000`), each `FUN_80021B04` call's record pointer resolves to PROT 905 **file `0x180C..0x1E00`** - a contiguous table of `[i16 model_sel][u16 flags][move-VM bytecode @+4]` records, recovered by `legaia_asset::summon_overlay` (disc-gated `summon_overlay_real`). This **supersedes** the two earlier wrong-link-base "FALSIFIED" readings - "the records are beyond the `0x5800` file / `0x180C` is only coincidentally record-shaped / parser reverted" and "there is no move VM here." The records *are* move-VM bytecode;
  the reason PROT 905 has zero `jal 0x80023070` *inside the overlay* is simply that the `jal` lives in the SCUS stager `FUN_80021B04` (which seats `actor[+0x70] = 2` PC → bytecode at `record+4`, then ticks `FUN_80023070`), not in the overlay image.
- **But the move-VM scene-graph is NOT how retail renders the player summon (live trace).** A PCSX-Redux trace of a player Gimard *Burning Attack* cast shows `FUN_801F7088` = **0×**, the move VM `FUN_80023070` = **2-3×** (noise), and the **battle per-actor draw `FUN_80048A08` = 35-64×/frame** → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`. So the **player** summon is drawn as an ordinary battle actor (per-object TRS keyframes), the faithful path being `engine-vm/anim_vm.rs` (`FUN_80048A08` / `FUN_8004998C`). The move-VM stager records still exist (and the engine drives them in `summon::SummonScene` as a stand-in), but they aren't the player summon's per-frame render path. SCOPE: the trace covers the **player** "Burning Attack" only;
  the **enemy** Gimard *Fire Tail* boss move is a distinct path - see the Fire-Tail note below.

The flame renders as Gouraud-textured (`POLY_GT3`/`POLY_GT4`) prims sampling the resident `etim` page (832,256) 4bpp; `cba`/`tsb` are applied at render.

- In a live Tail-Fire capture the summon library occupies `DAT_8007C018[3..32]`; ten of those (`[23..32]`) are fire-textured meshes (cba row 478 `0x778B` baked), and the **active Gimard flame is `DAT_8007C018[26]`** - the only rendered model baking etim, with both rendering actors carrying `actor[+0x64]=26` and `actor[+0x56]=5` (full-TMD mode → `FUN_8002735C`).
- Each individual flame mesh is **static geometry**; the visible fire motion is the **spawned part-actors** moving (the 8 RNG-seeded parts above), **not** CLUT cycling - the entire CLUT band is byte-identical across two animation-distinct `battle_gimard_tail_fire_a/_b` frames while the framebuffer differs ~21% (this falsifies the earlier "fire flicker = CLUT/palette animation" reading).
- The PROT 905 `LoadImage` (`FUN_800583C8`) CLUT uploads target VRAM row `481+` (the character/party-CLUT region), conditionally, not the flame's row 478.
- **Residual:** the part records are now recovered (`legaia_asset::summon_overlay`) and driven as a stand-in; what's open is the faithful **player** render - the battle TRS-keyframe path (`FUN_80048A08` / `FUN_8004998C`, ported) needs the summon's per-object keyframe source wired in place of the move-VM stand-in. See [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

##### Enemy "Fire Tail" - move-VM part, not the widget path

The **enemy** Gimard *Fire Tail* boss move is the distinct path the player-summon
trace did not cover, and it is now characterized from the two catalogued
mid-cast frames (`battle_gimard_tail_fire_a/_b`; disc + library gated
`firetail_movefx_liveness`). Unlike the player summons and the Cort/Delilas/Zeto
boss specials - which page a per-spell *stager* into slot B - Fire Tail's slot-B
occupant is the move-FX module **PROT 0900** itself (loader-B id `5`, byte-exact
at the residency pin file `0x1628` ↔ `0x801F8000`).
But PROT 0900's **screen-widget family is dormant**: an effect-actor-list walk of
both frames finds **zero** live mask/sprite/panel/letterbox widgets - so Fire
Tail is not the cutscene widget path (that stays exclusive to the eight ending
scenes; see [`move-vm.md` § screen-effect widget family](move-vm.md#screen-effect-widget-family-prot-0900)).
The live effect is instead a single **move-VM part-actor** in the part pool
`DAT_801C90F0`, ticked per frame by the generic SCUS actor tick `FUN_80021DF4`
(→ `FUN_80023070`) - a live capture pinning that render-tail driver. Its
`[i16 model_sel][u16 flags][bytecode]` record (`actor[+0x48]`) lives in the
**battle overlay (0898)** resident data at `0x801F5xxx` (below the 0900 slot-B
link base `0x801F69D8`), `model_sel` reading `-1` (transform node) / `5` (library
mesh `DAT_8007C018[5 + base]`) - the summon part-record format, sourced from the
battle overlay rather than a stager. So the move-VM scene-graph *is* Fire Tail's
render path (one live part), but its records are battle-overlay data and PROT
0900's role there is resident move-FX code, not the live driver.

#### Enemy boss stagers + the record-table trim

The six final-boss Cort special-attack stagers - extraction PROT **0938** (Mystic Circle), **0940** (Mystic Shield), **0944** (Guilty Cross), **0961** (Final Crisis), **0962** (Ultra Charge), **0966** (Evil Seru Magic; distinct from the player Juggernaut stager 0927) - parse as summon stagers under the same `0x801F69D8` link base and record format as the player block (`summon_overlay::ENEMY_BOSS_STAGER_PROT`; disc-gated `enemy_stager_real`). They spawn dominantly through the `FUN_80050ED4` pool wrapper rather than direct `FUN_80021B04` calls.

**The enemy-cast stager path is not Cort-specific.** Mid-cast captures of ordinary bosses pin the same mechanism on the universal `extraction = id + 895` arithmetic (loader-B current-id at `0x8007BC4C`, byte-resident at slot B; disc+library-gated `enemy_stager_binding`): the Delilas brothers - Gi / Blazing Slash `0x3F → 0958`, Che / Megaton Press `0x40 → 0959`, Lu / Plasma Strike `0x41 → 0960` - and Zeto, whose Call Wave and Big Wave are one logical attack over two turns and so share a single stager (`0x33 → 0946`). None of these four carries a `0x4000` render-mode record, and at the captured instants the part pool `DAT_801C90F0` is empty (no live part seated) - so the render-mode draw still has no live exerciser.

**Stager extraction entries are over-read windows.** The TOC-indexed footprint of every stager entry runs past the next entry's start LBA, so an extraction `.BIN` is `[this stager][the following stagers' bytes...]`; only the first `(next_start_lba - start_lba) * 0x800` bytes are the entry's own content (`summon_overlay::unique_content_len`).
The Cort mid-cast saves pin the boundary byte-exactly: each state's slot-B resident image matches its stager file up to precisely the TOC gap (0938 → `0x1800`, 0940/0944/0961 → `0x2000`, 0962 → `0x2800`, 0966 → `0x4000`) and diverges after it (stale bytes of the slot's previous occupant). Spawn sites in the over-read tail belong to *neighbouring* stagers, and their `lui/addiu` record pointers - valid only for the neighbour's own load at the shared base - dereference unrelated bytes in the wrong file window.

**That trim resolves the record-first-word "sentinel" question.** Across every trimmed stager (player 0903..=0913, the evolved-Seru block 0914..=0923, high 0927..=0934, the six Cort entries) the first word is only ever `-1` (transform node, dominant), a small library-mesh index, or **`0x4000`** - matching `FUN_80021B04`'s own dispatch exactly (negative → transform path, `0x4000`/`0x4001` → render-mode nodes, else library index). The previously-reported `0x1000`/`0x8000`-class sentinel population was the over-read artifact.

**Render-mode-node census (`0x4000`).** A static sweep of the trimmed stager
corpus (disc-gated `summon_overlay_block`) finds `0x4000` records in **five**
stagers: the three Sim-Seru high casts Palma (0928, 4 records), Mule (0929),
Jedo (0931), **plus two evolved-Seru player casts** - spell `0x8E` → 0916
(4 records) and `0x93` → 0921 (6). The evolved-Seru block (`spell_id
0x8C..=0x95` → extraction 0914..=0923, `summon_overlay::EVOLVED_SUMMON_STAGER_PROT`)
is the contiguous continuation of the player block under the same linear loader
arithmetic (`extraction = (id - 0x81) + 903`); every entry trims to a clean
move-VM stager, so the evolved casts ride the stager mechanism. **Eight of the
ten legs are capture-pinned** (`0x8C..=0x8F` → 914..917, `0x92..=0x95` →
920..923; one mid-cast state each, loader-B id read mid-cast + the stager 100%
byte-resident at slot B - disc+library-gated `evolved_summon_binding`); only
`0x90 → 918` / `0x91 → 919` stay arithmetic-predicted. **Both render-mode
carriers are pinned as player casts** - `0x8E → 916` (Aluru) and `0x93 → 921`
(Iota) - so neither unblocks the live-exerciser question below (a player cast
renders the namesake creature, never seats the stager parts). The two flanking
blocks carry the same byte-pin oracle: the base block `0x82..=0x8B` → 904..913
and the high block `0x99..=0xA0` → 927..934 each byte-pin one mid-cast state
per leg (loader-B id + slot-B-resident stager; disc+library-gated
`summon_binding_base_high`), so `0x82..=0x95` (minus the two predicted evolved
legs) and `0x99..=0xA0` are all regression-covered against real RAM.
Live correlation from the Cort states: every live pooled part-actor (`DAT_801C90F0` slots) carries `actor[+0x48]` pointing into the trimmed record table at a `-1` record (RAM first word == file first word), with the spawn-time `+0x56`/`+0x5A` zeros rebound post-spawn by the move-VM ops (`+0x56 = 4` / `+0x5A = 2` dominate mid-cast) and `actor[+0x64] = 0` throughout. No `0x4000`/`0x4001` part-actor was live in these captures.

**The render-mode nodes have no live exerciser in the catalogued corpus.**
For the three player Sim-Seru casts in the mid-cast save corpus whose stagers
*carry* `0x4000` records - Palma (0928), Mule (0929), Jedo (0931) -
a pointer-scan of each state's full RAM finds **zero** words referencing
any of the stager's record starts (or their `record+4` bytecode entries), even
though the stager is 99.9–100% byte-resident at slot B. So in a player cast the
move-VM scene-graph is not live at the on-screen instant at all - the summon
renders as its namesake `battle_data` creature through the monster animation
pipeline (the player-summon correction), and the stager part-actors (including
any `0x4000` node) are already gone. The Cort *enemy* path does run live stager
parts but holds only `-1` nodes. Pinning the `0x4000`/`0x4001` draw behaviour
therefore needs a frame-stepped capture inside an *enemy* stager-spawn window
whose stager carries a `0x4000` record - not reachable from the catalogued
states (`crates/mednafen/tests/summon_render_mode_node.rs`).

### `FUN_801D5854` - per-actor pose driver

The single most-cited helper inside `FUN_801E295C` (~30 call sites). Signature `FUN_801D5854(actor_id, pose_id)`. Pose IDs surfaced:
- `6` = idle / breathing
- `7` = ready / pre-action
- `8` = action-end / hit-recovery
- `9` = defeat / down

It is a **camera/presentation program driver**, not the animation system: its body dispatches `pose_id` 0..9 through a jump table at `0x801CEA00` computing three i16[3] tween-target vectors handed to `0x801D7130` (with a secondary dispatch on `actor[+0x1DB]` values `0x11..0x18` - per-art camera variants for the dynamically-installed art anims). It never writes `+0x1D9/+0x1DA`; the same-numbered **anim** ids 7/8/9 are staged separately (by the SM's own `+0x1DA` stores and the `FUN_8004AD80` end-of-clip chains), and the anim system's idle id is `0` - pose 6 has no anim counterpart (record[0] entry 6 is empty in every player file). The two id spaces are designed to align numerically at 7/8/9, which is what made the conflated reading stick.

`FUN_801D5854` opens with an **out-of-range guard** (`0x801D58C8..0x801D58E8`, `param_1` = `a0` = actor slot in `s5`, `param_2` = `a1` = pose id in `s4`): when `param_2 >= 6` *and* `param_1 >= 8` - i.e. a real pose requested for a slot outside the 8-entry pool - it forces `param_2 = 9` and calls `FUN_801DB9C4`, which scrubs the `+0x8` flag word across the pool. It is a defensive path, not the run-side animation lookup an earlier reading called it, and the operands are `>= 6` / `>= 8`, not `== 9` / `== 7`.

#### Case `0` - the submenu close-up framing

Pose `0` is the per-character command-menu close-up, called as `FUN_801D5854(actor_slot, 0)` from `FUN_801D388C`. Every component is a constant or a function of the acting actor; there is no per-seat table and no `base + seat * delta` angle law:

| Slot | Value | Kind |
|---|---|---|
| pitch | `0x20` | constant |
| yaw | `0x8F0 - actor[+0x46]` | facing-relative |
| TR.x | `-0x200` | constant |
| TR.y | `[0x801F4D2C + (char_id - 1) * 2]` | per-character height |
| TR.z | `0x600` (prescaled) | constant |
| focus | `-actor[+0x34/+0x36/+0x38]` | negated world position |
| duration | `0xC` = 12 frames | 6 camera steps x 2 vsyncs |

The battle actor pointer table is `0x801C9370`, indexed by slot (sibling of the `0x801C9360` arts-gauge table). `char_id = DAT_8007BD10[slot]` is the 1-based party-record selector, so TR.y keys on **character identity** (Vahn / Noa / Gala / Terra), not on seat - a per-model height offset. The table holds one entry per playable character; it is static overlay data, parsed off the disc by `legaia_asset::battle_camera_table` rather than transcribed, and installed on the world at scene entry. Vahn's entry is `0x480` = 1152, the value the solo-Vahn camera trace observes - which is what anchors the table's base and stride to the measurement.

A yaw of `2288` measured on a solo-Vahn fight is therefore not a seat constant: it is `0x8F0` with Vahn's battle facing of `0` subtracted, and `FUN_801E7824` resetting `actor[+0x46] = 0` is what makes that facing `0`. The framing is a fixed over-the-shoulder offset that generalizes to any seat once facing is tracked. The **per-seat variation lives entirely in the focus trio** (`0x80089118/1C/20`): the camera orbits about whichever actor is acting. With one party member that is indistinguishable from a constant, which is why a solo trace reads as a single fixed pose.

`TR.z` is the one prescaled slot - see [`FUN_801D829C`](#fun_801d829c---camera-angle-tween-prescale) below. Case `3` is the same shape with `0x900 - actor[+0x46]`, a second close-up `0x10` units round from this one.

#### Case `9` - the far Begin/Run framing

Pose `9` is the wide menu framing. Its depth and focus are **computed from the live formation**, so - like case `0`'s yaw - none of it is a magic number:

| Slot | Value | Kind |
|---|---|---|
| pitch | `0x20` | constant |
| yaw | `_DAT_8007B792` | passed through - the idle orbit owns it |
| TR.x / TR.y | `0`, `0x500` | constants |
| TR.z | `max(span * 3, 0x800)` (prescaled) | formation-sized depth |
| focus | `-(bbox centre)` | formation centre |
| duration | `0xE` = 14 frames | 7 camera steps x 2 vsyncs |

The builder walks a slot range selected by the framing argument (`0` = the whole field, `1` = enemies only, `2` = party only), skipping actors whose presence halfword `actor[+0x14c]` is zero, and accumulates `min`/`max` of `actor[+0x34]` (X) and `actor[+0x38]` (Z). `span` is the **larger** of the two extents, so a wide-but-shallow line frames on its width. The walk's slot mapping folds the party and enemy blocks together: on reaching the party count it jumps to slot 3, the first enemy slot.

The far framing's traced `TR.z` of `7680` is `prescale(0x12C0)`, i.e. `span = 1600`. That is a measurement of one formation, not a constant - and it is reproduced independently by the retail seat tables: the traced fight is a solo Vahn (party row 1, seat `z = -800`) against one monster (monster row 1, seat `z = +800`), a Z span of exactly `1600`. A three-member party frames wider.

#### `FUN_801D829C` - camera angle-tween prescale

The angle-tween builder takes three caller buffers of 3 x `i16` (rotation trio `0x8007B790/92/94`, translation trio `0x800840B8/BC/C0`, focus trio `0x80089118/1C/20`) plus a frame count. It rewrites **slot 5 only** - `TR.z` - as `(z << 8) / 0xA0`, converting a world-space camera distance into GTE projection units (`0xA0` = 160 = PSX screen half-width, `<< 8` = GTE `H = 256`).

The divide truncates, which is the fingerprint to look for: traced `TR.z` values are floors of a round raw, not exact divides. `0x400 -> 1638`, `0x600 -> 2457`, `0x800 -> 3276`.

The fourth argument is a **frame count**, not a speed - the stored word is the per-frame increment and the tween lasts that many vsyncs. The submenu call passes `0xC` (12 frames = the 6 measured camera steps at 2 vsyncs each); the action-camera sites pass `1` (instant cut) and `0x30`. Under a speed reading the submenu tween would take 436 steps.

The engine port of the framing rules lives at `crates/engine-shell/src/bin/legaia-engine/window/battle_cam.rs` (`BattleCamActor::submenu_pose` for case `0`, `menu_framing` for case `9`); the fixed-point tween kernel is `legaia_engine_vm::battle_camera`. The port tweens the focus trio on the same clock as the rotation and translation trios, and the window camera consumes it as the look-at target, so a non-Vahn seat frames on the acting member rather than on the formation centre.

### `FUN_801EED1C` / `FUN_801E7320` - party / monster setup hooks

Called from state `0x0C`:
- Party (`actor_id < 3`): `FUN_801EED1C()` - initialises per-character action data.
- Monster with AI flag (`+0x16E & 0x380 != 0`): `FUN_801E7320()` - initialises monster-AI action.
- Otherwise: neither - actor inherits from previous frame.

### `FUN_801EFE44` - battle camera bounds

Called from state `0x0C` for non-flee actions. Walks the 8-slot actor table computing min/max X and Z to set the battle camera's frustum. Read-only with respect to the action state machine; pure rendering helper.

### The escape roll (`FUN_801E791C`)

Called by state `0x64` to decide a retail flee. It is the writer of `_DAT_8007726C` - the
battle-message source pointer states `0x64`/`0x65` test: `ctx + 0x159` ("escaped" text) on
success, `ctx + 0x189` ("couldn't escape") on failure. From the dump
(`ghidra/scripts/funcs/overlay_battle_action_801e791c.txt`):

```
party_score = Σ_party  (SPD*3)>>1 + (maxHP - curHP)>>4    ; actor +0x164 / +0x14E / +0x14C
enemy_score = Σ_enemy   SPD      + (maxHP - curHP)>>5
roll_p = rand() % party_score ;  roll_e = rand() % enemy_score
if Escape Boost (ability bit 52):                 roll_p += roll_p >> 1
if Great Escape (bit 55) or ctx[+0x291] == 2
   or (_DAT_8007BAC0 & 0x100):                    roll_p = roll_e
FAIL iff  !(_DAT_8007BAC0 & 0x100)
          && (roll_p < roll_e  ||  ctx[+0x287] != 0)
```

Both sides run faster the more hurt they are (missing HP raises the score) and the party's
SPD is weighted 1.5x against the enemies' 1x; every slot contributes, downed members
included. The two ability bits are read from the *living* party members' second
accessory-passive word (character record `+0xF8`): bit 52 = passive `0x34` **Escape Boost**
(Chicken Heart, roll x1.5), bit 55 = passive `0x37` **Great Escape** (Chicken King) - the
assured bit forces the party roll equal to the enemy roll so the compare cannot fail, but
the scripted no-escape flag `ctx[+0x287]` is tested *after* that (`0x801E7AF0` sets the tie,
`0x801E7B14` reads `+0x287`) and still blocks it - "assured" describes only the compare, never
the outcome, which is why Chicken King is "assured escape (non-boss)" (see the
[accessory-passive table](../formats/accessory-passive-table.md)). The battle flag
`_DAT_8007BAC0 & 0x100` forces the flee outright - it bypasses even `ctx[+0x287]` and skips
the "No. of Escapes" Records counter (`_DAT_800846A8`) the normal success path increments.

**Both ctx inputs are written at battle setup, not by the roll.** `ctx[+0x287]` (the scripted
no-escape flag, also read by the state-`0x20` counter-attack gate) is latched by the SCUS
battle-setup routine `FUN_800513F0` in its first instructions: `ctx[+0x287] = (DAT_8007BD60 >> 5)
& 4` - it carries bit `0x80` of the battle-flags byte `DAT_8007BD60` (the same byte state `0x5A`
masks with `&= 0x7F`), so a scripted "can't run" fight sets it to `4` at load (`0x801E5058` reads
it; `see ghidra/scripts/funcs/800513f0.txt`). `ctx[+0x291]` is not written directly - it is a
**latch** of `ctx[+0x290]`: the SM's state-`0x00` action-begin does `ctx[+0x291] = ctx[+0x290]`
then clears `+0x290` (`0x801E2B38`). `ctx[+0x290]` itself is written by the formation-setup
routine `FUN_80051D84` - `1` under a monster-id-range test, or `2` on a `func_0x80056798()`
(BIOS-rand) roll - so `ctx[+0x291] == 2` is a per-formation flag set at battle setup (`see
ghidra/scripts/funcs/80051d84.txt`) that reaches the *same* forced-tie store as the Great Escape
bit, and carries the same caveat: it makes the compare unfailable, not the escape certain, since
`ctx[+0x287]` is read afterwards. Note also that `1` (back attack) is never compared here - the
roll only ever tests against `2`, so a back attack costs the party its round-one initiative keys
and nothing else. Because the roll reads only the *latched* copy, a state-`0x00` that clears
`+0x290` without copying it - or an engine that stores the latch and never reads it back -
silently disables pre-emptive-strike escapes for the whole battle.

On success the routine also stages the flee scene: every party actor is marked fleeing
(`+0x1DA`/`+0x1DC` = 1, facing `+0x46` = `0x800`, pose byte `+0x1DD` = 9), positions are
pulled toward the camera and spread at least 200 units apart, live HP/MP are written back to
the character records with downed members **floored at 1 HP** (the record-side half of the
state-`0x64` floor), and the camera move fires via `FUN_801D829C`. Ported:
`engine-vm::battle_formulas::escape_roll` (+ `escape_party_score` / `escape_enemy_score` /
`EscapeFlags`), rolled live by `engine-core::World::roll_battle_escape` when the command
menu resolves Run.

### Battle voice cues - the XA30 grunt vs the XA2/XA4/XA6 arts shout

Legaia's battle voices are **XA stream cues, not SPU samples**. There are two distinct
per-character voice cues, each fired through the SCUS clip player
`FUN_8003D53C(clip_slot, channel, dur)` (the runtime clip table at `0x801C6ED8` follows
`slot i` = `XA<i+1>`, see [cutscene.md](cutscene.md); the sequencer `FUN_8003D764` runs
`CdlSetloc` + `CdlSetfilter{file 1, chan}` + `CdlReadS`, and `dur` converts to an absolute CD
stop position `end = start + (dur * 0x96 + 0x95) / 0x3c`, a physical span `~dur * 2.5` sectors;
`see ghidra/scripts/funcs/8003d53c.txt`).

**1. Normal-move grunt (`XA30.XA`).** The battle-action overlay's input handler around
`0x801EEB44` (`see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`) reads the acting
slot's 1-based character id from `DAT_8007BD10[slot]` and fires `FUN_8003D53C(0x1D, chan, dur)`
(clip slot `0x1D` = `XA/XA30.XA`) with a per-character channel: Vahn chan 0 (`dur 0x26`), Noa
chan 4 (`0x2E`), Gala chan 6 (`0x1A`). This is the short grunt an ordinary directional attack
plays; each XA30 hero channel is one clean ~0.4-0.7 s vocalization.

**2. Tactical-Arts shout (`XA2` / `XA4` / `XA6`).** When the staged-anim materialiser
`FUN_8004AD80` runs a party art action, it calls the arts-voice cue selector
`FUN_8004C140(char_id, action_constant, flag)` (`see ghidra/scripts/funcs/8004c140.txt`),
which fires `FUN_8003D53C(clip_slot = (char_id-1)*2+1, channel, dur)`:

| character | clip slot | arts-voice file |
|---|---|---|
| Vahn | 1 | `XA2.XA` |
| Noa  | 3 | `XA4.XA` |
| Gala | 5 | `XA6.XA` |

all 16-channel short-mono shout banks. This is **traced and capture-verified**, not by-ear: a
live PCSX-Redux trace of Vahn's Tri-Somersault fires `FUN_8003D53C(0x01=XA2, chan 0/6, ...)`
and Noa's Miracle fires `(0x03=XA4, ...)`, both from `FUN_8004C140` (`ra = 0x8004C464`;
scenarios `battle_vahn_tri_somersault_super` / `battle_noa_miracle_art_combo`, probe
`scripts/pcsx-redux/autorun_arts_voice_cue.lua`). The `channel` is chosen **at random**
(avoiding an immediate repeat, via `gp+0xa4a`) from a per-art **candidate-channel pool** keyed
by the art's action constant. The pools live in SCUS tables: a range table at `0x800781A4`
(`[lo, hi, second_lo]` per character), a first-half table (`base + (hi - ac)*0x0F`) for
`lo <= ac <= hi`, and a second-half table (`base + (ac - second_lo)*0x10`) for `ac >= second_lo`
(non-combat bases `0x80077B64/0x80077D5C/0x80077F54` and `0x800780A4/0x80078104/0x80078154`;
`FUN_8004C140` also carries combat-mode / special-flag variants). Each record is a channel list
- byte `+0` is always a member (channel 0 is legal) and, when `+1 != 0`, runs to the next `0`.
`dur = (dur_table[channel + char*0x10] * 0x3C + 99) / 100` from `0x80077A8C` (verified: Vahn
`ch0` -> `0x2D`, `ch6` -> `0x3D`, matching the trace). Parser: `legaia_art::arts_voice`.

Note the arts shout is **not** in the art record ([art-data](../formats/art-data.md)); its Hit
Effect Cue `0x1A` low half is an SPU SFX-descriptor id ([sfx-table](../formats/sfx-table.md)),
a separate subsystem. The stereo long-clip banks `XA3` / `XA5` are Noa/Gala Miracle & summon
**fanfares** fired from the `FUN_8004FCC8` jingle path (`ra = 0x8004FD7C`), not the per-art
shout - an easy mis-ID by ear. Sibling cue in the battle overlay: SM state `0x6E` of
`FUN_801E295C` plays a whole-file XA stream via `FUN_8003EAE4(0, slot)` with the slot from the
SCUS byte table at `0x800787AF` (heroes → slot `0x08` = `XA9.XA`, no channel filter).

The site's arts page reproduces cue 2 faithfully: `crates/web-viewer/src/arts_view.rs` parses
`legaia_art::arts_voice` off the visitor's `SCUS_942.54`, demuxes the character's `XA2`/`XA4`/`XA6`
channels, and maps each art (by its record `anim_id` = action constant) to a stable member of
its real candidate pool; `site/js/arts-viewer.js` plays that channel as the art starts.

### Battle helper functions

Four helper addresses `FUN_801E295C` calls in the `0x801Fxxxx` battle-overlay region
(`0x801F0348` / `0x801F1ED4` / `0x801F3990` / `0x801F45A4`). **Dump-aliasing caution:** the
`overlay_0897_801f*` dumps these were first decoded from are double-shifted - PROT 0897's
extraction over-reads into PROT 0898 and that Ghidra program maps the file at base `0x801C0000`
instead of the true `0x801CE818`, so every function it surfaces at a `0x801Fxxxx` VA is really a
different battle-overlay function (and the "mid-body label inside an earlier entry" pairing is
an artifact of the same shift). `0x801F3990` is re-pinned below from battle-resident bytes; the
other three descriptions are retained but **need re-verification** against a battle-resident
dump (`overlay_battle_action_*`) before being relied on.

**Over-read `0x801Fxxxx` / `0x8020xxxx` alias resolutions.** A cluster of addresses that
surface as self-entry bodies in the `overlay_0897*` / `overlay_0897_xxx_dat*` dumps are the
same double-shifted images - each is byte-identical modulo relocation to a battle function
already pinned under its true entry, and nothing attests the printed VA. Resolve them to the
real entry (arbiter `classify-worklist.py --explain`; the first two independently confirmed
from the disassembly against the descriptions cited):

- `0x80205504` -> `FUN_801EED1C`, the retail queue-builder (below): zeros the 16-word scratch
  at the shifted `0x801F6990`, writes the action queue `actor[+0x1DF..+0x1E2]`, calls the
  Super applier `FUN_801EF9E4`.
- `0x8020A178` -> `FUN_801F3990`, the cast audio-cue dispatcher (below): mode gate on
  `DAT_8007BD10[ctx+0x13]`, two 9-entry `jr` jump tables keyed on `actor[+0x1E8]`, cues via
  `FUN_8004FCC8`; `actor[+0x1DF] == 0xFE` takes an effect-spawn path.
- `0x802028C4` -> `FUN_801EC0DC`.
- `0x801FD150` -> `FUN_801E6968`, the Lost Grail Final Heal auto-revive (state `0x50`).
- `0x801F8580` -> `FUN_801E1D98`.
- `0x801F8AB0` -> `FUN_801E22C8`.

Read the true entry's battle-resident dump, never the shifted alias. The remaining worklist
addresses at these VAs are non-standalone (interior citations, shared tails, `$zero`-absolute
data decoded as code, or 0-instruction stubs) and carry no body to document.

**`0x801F0348` - target-size camera framing.** Pinned from battle-resident bytes
(`overlay_battle_action_801f0348.txt`). It writes the camera height/distance at ctx `+0x6D0`
(i16) from a monster's **size class**, the byte at monster record `+0x1F`:

```text
ctx+0x6D0 = clamp(size_class << 7, 0x0C00, 0x1400)
```

The default `0x0C00` is also the floor, so only monsters with a size class above `0x18` pull the
camera back at all and everything from `0x28` up saturates. The slot it reads the size from is
resolved twice: first from the acting actor's target slot (`+0x1DD`, when `>= 3`), then - when the
acting actor is *itself* a monster (`ctx+0x13 >= 3`) - overwritten with the acting actor's own
size. The second store really does clobber the first, so a monster's attack frames on the
attacker's bulk rather than the target's. Record pointers come from the monster table at
`0x801C9348 + (slot-3)*4`.

Both lookups sit behind an **outer gate** on the target byte, `sltiu v0,v1,0x8` at `0x801F037C`,
and its branch target is the *clamp* rather than the attacker arm. A target slot of `8` or above
therefore suppresses the attacker-side store as well, leaving `ctx+0x6D0` at the `0x0C00` seed -
the one path on which a monster attacker's own size is ignored. Live slot bytes are only ever
`0..=6`, so the gate is a guard against a stale or uninitialised `+0x1DD`.

Ported as `battle_formulas::camera_height_for_frame` (whole routine, gate included) over
`camera_height_from_size_class` (the `<< 7` + clamp arithmetic), and wired: the port runs at
`ActionSeed` - the same edge as retail's call at `801e2d2c`, ahead of the gated
`FUN_801EFE44` bounds walk - feeding `BattleActionHost::camera_frame_height` and landing on
`World::battle_camera_frame_height`. The size input comes from the monster record's `+0x1F`
([`monster-animation.md`](../formats/monster-animation.md), `MonsterRecord::size_class` ->
`MonsterDef::size_class`) through the `BattleActionHost::monster_size_class` hook.

Retail's monster-band base is the literal `3` at `0x801F0384` / `0x801F03CC`, because retail
reserves three party slots whatever the party size. The port takes that base as a parameter
(`RETAIL_MONSTER_SLOT_BASE` for the retail reading) because `engine-core` compacts its seating
and seats the first monster at `party_count`; the two agree for any three-member party. This is
the same seating split `apply_side_lockout` documents from the other side.

> The earlier reading of this address - a 40-slot widget-pool teardown walking ctx `+0x11B4` -
> came from the aliased `overlay_0897_801f0348.txt` dump and is **falsified**: the
> battle-resident body contains no widget table, no free call and no `0x801C8FA0` clear.

**`0x801F1ED4` - player-summon effect-script dispatcher.** Re-pinned from a clean
self-entry dump: the classifier confirms the muscle-dome capture's bytes at this
VA are byte-identical to the PROT 898 battle-action image (`--explain 801f1ed4`
=> `REAL`; entry `801f1ed4`, 163 insns, `jr ra`; the `overlay_0897` dump is
interior, entry `801f1cc8`). The body is a `jr`-jump-table dispatcher on
`actor[+0x1DF] - 0x81` (the summon / Enhanced-Seru-Magic id block `0x81..=0xA0`;
`sltiu` bound `0x20`, table at `0x801D54EC`) into per-summon effect routines
(`FUN_801F69D8`, `FUN_801F6A84`, ...); after the routine, if `ctx[+0x27A] != 0`
it calls `FUN_801F2410`. It is the summon spell's visual driver, not a centroid
re-centre. The earlier "summon actor/camera re-frame - bounding-box recentre onto
the cast centroid" reading came from the aliased `overlay_0897_801f1ed4.txt` dump
and is **falsified**: that centroid-recentre body is really the formation
span-normalise leaf `FUN_801DB318` (documented above), surfaced at a shifted VA.
The summon **creature spawn** is a separate mechanism (the `summon.dat` applier
`FUN_801F12D0` / `FUN_801F19EC`, see [summon-readef](../formats/summon-readef.md)).
See `ghidra/scripts/funcs/overlay_muscle_dome_801f1ed4.txt`.

**`0x801F2160` - magic effect-class dispatcher.** Sibling of `0x801F1ED4`, called
from state `0x70` (magic-capture phase 2). Re-pinned from a clean self-entry dump
(`--explain 801f2160` => `REAL`; muscle-dome bytes = PROT 898; entry `801f2160`,
172 insns, `jr ra`). A `jr`-jump-table dispatcher on the spell's **effect-class
byte** - `*(DAT_800754C8 + actor[+0x1DF]*0xC + 1)`, i.e. `+1` of the SCUS
spell-table record ([spell-table.md](../formats/spell-table.md)); `sltiu` bound
`0x20`, table at `0x801D5A6C` - into per-effect-class routines
(`FUN_801F69D8`..`FUN_801F9BA8`); after the routine, if `ctx[+0x27A] != 0` calls
`FUN_801F2410`. Effect/render-track (each target routine drives that class's
visual sequence), documented not ported. See
`ghidra/scripts/funcs/overlay_muscle_dome_801f2160.txt`.

**`FUN_801F3990` - cast audio-cue dispatcher.** Pinned from battle-resident bytes (the aliased
`overlay_0897_801f3990.txt` dump surfaces a different function - really `FUN_801DD0AC` - at this
VA). Argument-less: reads the active-actor index `ctx[+0x13]` and the per-slot char-kind table
`DAT_8007BD10`, dispatches on `actor[+0x1E8]` through two 9-entry jump tables, and plays the
per-class cast sound cues via `FUN_8004FCC8`. The earlier "per-move damage roll `FUN_801F3894` -
move-power table + RNG → damage, with a `FUN_801EC964` decimal-digit formatter" description came
from that double-shifted dump and is falsified. The spirit damage the state-`0x3D` reading
attributed here is state `0x3E`'s inline formula, ported as `battle_formulas::spirit_damage`.

**`0x801F45A4` - end-of-action damage / HP-bar settle.** *Decoded from the aliased
`overlay_0897_801f45a4.txt` dump (disasm only; the Ghidra decompile times out) - identity, entry
address, and body need re-verification against battle-resident bytes.* Per
action category (`actor[+0x1DE]` `1..6`) it tests the actor's ability bits in the character record's
`+0xF4`/`+0xF8` bitfield (base `0x80084140 + (char_id-1)*0x414`, fields `+0x6BC`/`+0x6C0` = record
`+0xF4`/`+0xF8`) and, when set, ramps a value pair `*s0` toward `*s2` by half per pass (`*s0 += (*s2
- *s0) >> 1`) - the HP-bar / damage-number settle. It applies the AP-boost bits (`+0x200`/`+0x100`)
to `actor[+0x170]` and clamps it at 100 (`0x64`) - the same adjust-and-clamp the `0x50` Done arm
performs - clears status-word `actor[+0x16E]` bits, resets brightness/screen globals, and ends in a
per-actor jump-table dispatch keyed on `actor[+0x1D]`. Called at battle-complete (`0xFF`); it is the
final damage/HP settle + ability-effect application, not a bare teardown.

### Actor-pool leaf helpers

Small self-contained routines the SM and its round driver call over the
8-slot battle-actor pool (`&DAT_801C9370`) and the ctx target queue. Each is
ported as a pure function in `engine-vm::battle_action` (`pool_ops`); all are
transcribed from the disassembly (`overlay_battle_action_801db9c4.txt` /
`_801db318.txt` / `_801d8a88.txt` / `_801d8d00.txt` / `_801db124.txt` /
`_801db8b4.txt` / `_801dba04.txt` / `_801db81c.txt`, plus `80019b28.txt`).

- **`FUN_801DB9C4` - pool `+0x8` flag-word scrub.** AND-masks the `+0x8` flag
  word of pool slots 0..=6 with `0x7CFFFFFF` (clears bit 31 and bits 25/24).
  Its only static caller in the battle overlay is the pose setter
  `FUN_801D5854`'s out-of-range guard (`jal` at `0x801D58E8`). It is **not** the
  state-`0x5A` per-actor anim-flag clear: state `0x5A` runs its own inline mask
  (`lui a1,0x7cff` at `0x801E6478`, paired with `+0x21F = 0`) and
  `FUN_801E295C` contains no call to `FUN_801DB9C4`. Port:
  `clear_pool_flag_words`.
- **`FUN_801DB318` - formation span-normalise + recentre.** Over the included
  slots (0..2 always, 3.. gated on `+0x14C`), takes the X/Z extents; if an axis
  spans more than `0x800` it rescales every included coordinate by
  `(coord << 11) / span` (`span` is the extent narrowed to i16) and divides the
  matching camera-focus accumulator (`_DAT_80089118` X / `_DAT_80089120` Z)
  likewise; then it recomputes the extents and subtracts the centroid
  `((max + min) as u32) >> 1` from every included slot, shifting the focus
  accumulators back by the same centroid. Port: `normalize_formation_span`.
- **`FUN_801D8A88` - attack target-queue builder.** Builds the ring the cycle
  accessor steps through. Counts live monsters (slots 3..=6) into `ctx[+0x244]`,
  takes the acting actor's `+0x1DD` current target as the wrap slot `+0x245`,
  then computes each monster's bearing offset from the current-target direction
  (via `FUN_80019B28`, each result `+0x800 & 0xFFF`, expressed as a positive
  angle in `[0, 0x1000)`) and appends the three nearest *alive, non-target*
  monster slots to `+0x246..` in ascending order, consuming each pick. Port:
  `build_attack_target_queue` / `AttackTargetQueue` (the bearing is a closure so
  the ordering ports without the retail arctan LUT).
- **`FUN_801D8D00` - attack target-cycle accessor.** Locates the active actor's
  current target inside the multi-target ring built by `FUN_801D8A88`
  (`ctx[+0x244]` count, `+0x245` wrap slot, `+0x246..` ordered slots) and steps
  to the next (`param 0`) or previous (`param 1`) entry, wrapping at the ends.
  Port: `cycle_attack_target` / `TargetCycle`.
- **`FUN_801DB8B4` - first live monster slot.** Scans pool slots 3,4,5,6 and
  returns the first with a non-zero `+0x14C` liveness halfword; falls through to
  `7` when none is alive. Port: `first_live_monster_slot`.
- **`FUN_801DBA04` / `FUN_801DB81C` - selectable-participant scans.** Both walk
  the pool over `0..ctx[0]` applying the same three predicates - action-state
  byte `!= 4`, alive (`+0x14C`), and no can't-select ailment (`+0x16E & 0xF84`).
  `FUN_801DBA04` starts at slot 0 (first selectable target); `FUN_801DB81C`
  starts at `ctx[+0x13] + 1` (next participant after the current actor). Each
  returns `ctx[0]` when nothing qualifies. Ports: `first_selectable_target` /
  `next_selectable_actor`.
- **`FUN_80019B28` - 12-bit bearing (atan2).** Folds the displacement
  `(p2 - p1)` into a quadrant by sign, divides the shorter leg into the longer
  (`(min << 11) / max`), indexes the retail arctan LUT at `0x8006F4C8`, and adds
  the per-octant `0x000/0x400/0x800/0xC00` base to reassemble a clockwise 12-bit
  heading (`0x000` = `-Z`, `0x400` = `+X`). Port: `bearing_12bit` (LUT is
  caller-supplied Sony data; no table bytes embedded). The motion VM keeps a
  separate `f32` approximation for its face-target ramp.
- **`FUN_801DB124` - dead-target redirect roll.** When a queued action's chosen
  target (`actor[+0x1DD]`) is dead, and the category qualifies (Attack always;
  Magic when the spell class byte `>= 0xA` or the target is an enemy slot; Item
  only for ids `0xFE`/`0x98`), it re-rolls a **living** slot on the same side
  (`rand % party_count`, or `rand % monster_count + 3`), retrying until alive.
  Port: `redirect_dead_target` / `RedirectQuery`.

### Per-frame action-effect update helpers

Battle-overlay functions that run the *visual* side of a chosen action -
projectile flight, action-HUD chrome, side-band asset streaming. All are heavy
GTE / GPU-primitive / CD-IO work reached through the effect + anim path, so
they are documented here rather than lifted whole into `engine-vm`.

- **`FUN_801DA6B4` - target-select cursor tint.** Over the fixed monster-slot
  window `3..=6`, brightens the acting actor's current target (`+0x1DD`) and
  dims the rest: `+0x21C` render flag (`5` / `200` / `0`), `+0x4` colour word
  (`0x20080200` / `0x00401004`), `+0xC` scale word (`0x1000` / `0`). `param_1
  == 0` stamps the highlight; non-zero clears it. Only the alive slots
  (`+0x14C != 0`) are touched. Self-contained; ported as
  `battle_action::target_cursor_highlight`. See
  `overlay_battle_action_801da6b4.txt`.
- **`FUN_801DBDDC` - action banner box.** Gated on `ctx[+0x6CE] == 0`. Emits
  one `0x09`-code quad into the ordering table at `_DAT_1F8003A0` (colour
  `0x2C808080`, rect derived from the three `short` args) and links it via
  `FUN_8003D2C4`. Pure GPU-primitive build. See
  `overlay_battle_action_801dbddc.txt`.
- **`FUN_801DEA50` - action effect-script stepper.** For the acting actor
  (`param_1 == ctx[+0x13]`) walks 8-byte effect-script records at `param_2`
  under the `actor[+0x1F5]` cursor (`< 8`), GTE-rotating each record's offset by
  the actor's `+0x46` facing through the sin/cos LUTs (`_DAT_8007B81C` /
  `_DAT_8007B7F8`) and spawning effects via `FUN_80050ED4` / `FUN_801DFDF0`. On
  a terminator it installs the **move-power record** (`0x801F4F5C +
  map[actor+0x1DF]*0x1A`, map at `0x801F4E64`, `0x1A`-byte stride) at
  `ctx[+0x1014]` and seeds per-target homing state (`+0x1144` position, `+0x252`
  target, `+0x1166` bearing). GTE + effect-spawn; not ported. See
  `overlay_battle_action_801dea50.txt` and
  [move-power.md](../formats/move-power.md).
- **`FUN_801E09F8` - cast-effect census + projectile flight/impact.** Runs two
  jobs each frame. **(1) Census**: it recomputes, from scratch, the outstanding
  effect-count fields the magic/summon exit states poll - `ctx[+0x249]` (actors
  still mid-animation: `+1` per live actor with `+0x1D9 != 0`, less party actors
  whose `+0x1D9 == 8`) and `ctx[+0x24D]` (active spell-children over `ctx[+0x252..]`),
  plus the sole-survivor target indices `ctx[+0x24A]` (party) / `ctx[+0x24B]`
  (monster). These are **live counts, not latched flags**, which is why state
  `0x2E` (magic exit, gated on `ctx[+0x249] == 0`) and state `0x35` (summon
  sustain) wait on them - a stalled effect child that never dies pins the count
  above zero and holds the band. **(2) Flight/impact**: it steps the in-flight
  effect slots (`ctx[+0x24E]` phase, `+0x252` target, `+0x1144` position, `+0x6C6`
  per-slot timer), homing each with the LUT trig and spawning per-effect visuals
  via `FUN_801DFDF0`; on arrival it calls the damage kernel `FUN_801DD0AC`
  (indexed through the `0x801F4E64` map) and applies the roll to the target's HP
  (`+0x14C`), death anim (`+0x1DA`), and the accumulated-damage queue
  (`ctx[+0x83C]`). GTE + effect + damage-application; not ported. See
  `overlay_battle_action_801e09f8.txt`.
- **`FUN_801E0080` - battle particle/sprite-cloud animator.** Gated on
  `DAT_8007BD58 != 0 && DAT_8007BD71 == 0xFF` (battle live, no end signal).
  Advances per-frame animation cursors across two effect pools (a 32-slot
  `0x1C`-stride pool and a 128-slot pool at `_DAT_8007BD30 + 0x10`), applies the
  same sin/cos-LUT rigid rotation (`_DAT_8007B7F8` / `_DAT_8007B81C`, shifts
  `>>4` / `>>0xC`) per part, then in a third pass builds textured-sprite GPU
  primitives (`0x09000000` command word, per-particle brightness) into the OT at
  `_DAT_1F8003A0`, projecting each via `FUN_800195A8` and linking with
  `FUN_8003D2C4`. Pure GTE / GPU-primitive emit; not ported. See
  `overlay_battle_action_801e0080.txt`.
- **`FUN_801DF6B8` - damage-number popup renderer.** Draws a scaling decimal
  number sprite for one actor's accumulated damage `ctx[+0x83C]`: extracts each
  base-10 digit (`* 0x66666667` / `>>0x22` = divide-by-10), indexes the digit
  glyph atlas at `0x801F6..` (`-0x7FE09BA4`), and builds one `0x09`-code sprite
  quad per digit into the OT, ramp-scaling the rect by the per-frame timer
  `ctx[+0x85C]`. Reads actor screen position `+0x3C/+0x3E/+0x40`. Pure
  GPU-primitive build; not ported. See `overlay_battle_action_801df6b8.txt`.
- **`FUN_8005112C` - per-character signature effect trigger.** SCUS-resident
  (`8005112c.txt`), gated on `actor[+0x68] != 0 && actor[+0x5A] < 3` (a party
  slot). Reads the roster char id `DAT_8007BD10[actor[+0x5A]]` (`1`/`2`/`3` =
  Vahn/Noa/Gala) and, when the actor's current anim id (`*(actor[+0x4C]) + 0x77`)
  hits that character's hard-coded frame value (`0x29`/`0x1E`/`0x2A`/`0x64`),
  fires `FUN_80048310(actor, effect_id, 3, rgb)` with a per-character effect id +
  RGB tint - a hand-authored visual accent on a specific animation frame. Effect
  spawn, not a formula; not ported.
- **`FUN_801F17F8` - summon / readef side-band streamer.** A three-phase
  (`ctx[+0x26C]`) CD loader gated on `ctx[+0x26B]`: opens `data\battle\summon`
  (arg `0x37F`) or `data\battle\readef` (arg `0x380`) via `FUN_800558FC`, reads
  a `0x10800`-byte page into `ctx[+0x314]`, and waits on `FUN_8003DE7C`. Pure
  CD-IO; the engine streams these through `SceneAssets`. See
  `overlay_battle_action_801f17f8.txt` and
  [summon-readef.md](../formats/summon-readef.md).

The worklist addresses `0x801F1ED4` / `0x801F2160` have their only clean
self-entry dumps in the **muscle_dome** overlay, but the classifier confirms
those bytes are **byte-identical to the PROT 898 battle-action image**
(`--explain` tags both `REAL`, capture `battle_action(898)`) - the muscle-dome
overlay carries the same battle-action code region, so the dumps *are* the
battle-resident bodies and are decoded [above](#battle-helper-functions) (the
summon and magic effect-class dispatchers). The battle overlay's *own* dumps
show these VAs as interior because the `overlay_0897` extraction is
double-shifted (the aliasing the caution above describes); that shift is also
what produced the earlier - now falsified - "summon actor/camera re-frame"
reading of `0x801F1ED4`. `0x801F69D8` / `0x801F7088` remain per-summon effect
leaves reached through those dispatchers.

## Notes for the engine port

- The state graph is **flat** within each band: `0x14 → 0x15 → 0x16 → 0x17 → 0x18 → 0x1E` is the attack-strike chain. There are no jumps backward except from `0x5A` (which restarts at `0x0A` for the next actor).
- `ctx[+0x6D8]` is a 16-bit signed countdown. Most states that wait do `*(short*)(ctx + 0x6D8) -= DAT_1F800393` and check sign-flip. Engine port: model as `i16` ticks-per-frame counter.
- The state machine does **not** own the animation. It writes `actor[+0x1DA]` (queued anim) and waits on `actor[+0x1D9]` (current anim) to converge. The convergence is performed by the SCUS anim trio - the per-frame anim-node tick `FUN_80047430` (cursor advance + end-of-clip detect) calls the commit `FUN_8004AD80` (id → action-record install, `+0x1D9 = +0x1DA` snap, reaction/end chains), and the decoder `FUN_8004998C` cross-blends the last frame toward the queued clip's frame 0. `FUN_801D5854` never touches the anim fields (see [pose driver](#fun_801d5854---per-actor-pose-driver)); the earlier note attributing the tween to it and to `FUN_80021DF4` was wrong.
- Actions are **interruptible** only at `0x1E` (counter-attack steal). Every other transition is unconditional once the precondition fires.
- Battle-end (`DAT_8007BD71 = 0xFE`) is set from `0x5A` (post-cleanup count of survivors, with `_DAT_8007BD2C` carrying the wipe cause) or `0x66` (the successful-escape teardown - no wipe cause byte). The mode-state-machine then unloads the battle overlay.
- The `0x5A` **monster-wipe victory arm** stages the win pose off the acting actor's party slot, re-picking a living party member only when the acting actor is dead (the alive-skip at `0x801E6690`). Retail is safe because the wipe scan and the scheduler share the `+0x14C != 0 && !(+0x16E & 0x4)` predicate, so an alive acting actor is always a party member - but the randomizer's enemy-ally charm widens that mask to `0x384` and breaks the invariant. Full chain + the randomizer's disc-side fix (`legaia_patcher::charm_fix`, a single-word `0x801E6690` detour widening the keep-condition to a living party slot): [battle.md](battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock).

## Decompile quirks worth knowing

- The decompile shows `_DAT_8007BD24` typed as `int*`. `_DAT_8007BD24[N]` is therefore byte N of the **pointed-to** struct (Ghidra resolves the pointer dereference as part of the indexing) - not byte N of the pointer itself. This trips up first-pass readers; see [battle.md](battle.md) § "Battle context struct" for the decode.
- `ctx[+0x6DA]` and `ctx[+0x6DB]` look like u8 fields but are read as a u16 pair (the `0x6DA` access at line 4147 of the dump uses `*(short *)(_DAT_8007bd24 + 0x6da)`). Treat as packed `(timer_lo, timer_hi)` or `i16`.
- Several states share an exit edge into `0x5A` via fall-through (e.g. `0x6B` → `0x5A`). The C decompile materialises this as explicit assignment; the MIPS source sometimes uses `j 0x801E6814` (function epilogue) directly without a state write.
- `func_0x80056798()` returns the PSX `rand` BIOS call. Its veneer reads `li t2,0xA0; jr t2; li t1,0x2F`, so the vector is **A0 `0x2F`** - not `0x2E`, which is `memchr` and belongs to the separate veneer at `FUN_80057014`. It's used for combat RNG (combo timing, capture chance, run angle).
- Signed-vs-unsigned comparisons appear pervasively (`(int)((uVar10 - uVar16) * 0x10000) < 0` is the idiom for "i16 went negative this frame"). The compiler emitted these as explicit casts to satisfy Ghidra; the underlying MIPS is a `bgez`/`bltz` on a sign-extended halfword.

### Interior addresses cited as if they were entries

The corpus stores mid-function citations as their own `<addr>.txt` files whose
whole body is a pointer at the enclosing dump. Three land in this overlay's
documented functions, and none of the three is even a basic-block head - each
is a single instruction in the middle of an expression, which is why no
prologue and no `jr ra` appears anywhere near it.

| Address | Inside | The instruction |
|---|---|---|
| `0x801EA5C4` | `FUN_801E9FD4` (the [enemy AGL action budget](#enemy-agl-action-budget-fun_801e9fd4)) | One arm of the four-way `andi 0x60` classification of the spell record's byte `+2`: `beq a0,v0,0x801EA7E8` selecting the `0x20` class. |
| `0x801EC784` | `FUN_801EC3E4` (the [physical-attack damage kernel](battle-formulas.md#physical-attack-damage---overlay_battle_action_801ec3e4)) | `addiu a0,a0,0x4140` - the low half of the `lui/addiu` pair forming the character-record base `0x80084140`, immediately before the `slot * 0x414` stride multiply. |
| `0x801EF228` | `FUN_801EED1C` (the [arts queue-builder](#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4)) | `addu v0,v0,v1` - the second step of that same `x*0x414` stride idiom (`((x<<6)+x)<<2 + x)<<2`), here indexing `+0x6BC` of the resolved record. |

The stride idiom is worth recognising on sight: any dump opening inside
`sll/addu/sll/addu` over a small integer, followed by an add of `0x80084140`,
is in the middle of a character-record lookup and is not a function.

## Engine port

`crates/engine-vm/src/battle_action.rs` ports the state graph as a per-frame edge-triggered state machine. Surface:

- `ActionState` - symbolic enum for every named state byte; `from_byte` returns `None` for unmapped values (so the dispatcher can surface them as `StepOutcome::UnknownState` for engine logging).
- `ActionCategory` - symbolic enum for the action-category byte at `actor[+0x1DE]`.
- `BattleActor` - the per-actor fields the state machine reads or writes. Field names mirror the `+0xNNN` byte offsets above so the link to the decompile stays explicit.
- `BattleActionCtx` - the subset of the live ctx struct (`_DAT_8007BD24`-pointed) the state machine touches: `action_state`, `active_actor`, the `+0x6D8` countdown timer, etc.
- `BattleActionHost` - engine callbacks for every cited helper (`FUN_801D5854` → `pose`, `FUN_801D8DE8` → `ui_element`, `FUN_8004E2F0` → `range_check`, `FUN_801DABA4` → `recompute_battle_order`, `FUN_801EFE44` → `camera_bounds`, `FUN_801EED1C` / `FUN_801E7320` → `party_setup` / `monster_setup`, `func_0x80056798` → `rng`, `func_0x8003F2B8` → `previous_action_cleared`, ...). All methods have default impls so a minimal host compiles.
- `step(host, ctx) -> StepOutcome` - runs one frame's worth of dispatch; returns `Stay` (still waiting on a precondition), `Transition { from, to }`, `BattleComplete` (terminal), or `UnknownState { state }` (default-arm fall-through for unmapped bytes).

`crates/engine-core/src/world.rs` composes this with the actor VM, move VM, and effect VM into a single `World` struct that engines drive via `World::tick`.

### Staged-anim playback (the attack band plays in-engine)

The ids the SM stages into `actor.queued_anim` actually play on the battle actors. The id → slot/record ladder of the retail commit `FUN_8004AD80` is `legaia_engine_vm::anim_vm::resolve_staged_anim`: ids `< 0x10` play their action-table entry directly (`0` idle, `1` walk/approach, `0xC..0xF` the equipment-spliced weapon swings); ids `>= 0x10` materialize **art-bank record `id − 0x10`** into dynamic slot `0x10`/`0x11` (ids `0x10` and `0x1A` install at `0x11`) and the staged id is rewritten to the slot number.

`World::commit_staged_battle_anims` (called from `step_battle` pre-step and from `tick_battle_animations`) applies that ladder per actor: a staged swing/art plays as a one-shot `MonsterAnimPlayer` (rate from the record's entry `+0x78` byte through the same `step_for_rate` path as the idle clips), the id pair converges on the committed value, and the in-flight clip outranks the SM's per-frame `pose()` requests (the same precedence rule hit reactions use).
The clip's finish is the engine's anim-end signal: `ADVANCE_DONE` clears (opening the `0x801E370C` read gate for the next strike byte), the id pair converges back to idle `0`, and the idle loop resumes. An actor with no usable clip for a staged id converges immediately (a zero-length swing), so clip-less hosts keep the pre-animation pacing.

Clip sources, decoded at battle entry next to the mesh assembly (`play-window`): the record[0] action streams + `swing_battle_animations` (per equipped item, runtime slots `0xC..0xF`) feed `World::set_actor_battle_action_clips`; the art bank (`art_animation_bank`, streams resolved through the `readef.DAT` `"ME"` archives via `art_me_archive`/`art_animation`) feeds `World::set_actor_battle_art_bank`.
Monsters install no bank, so their staged ids stay plain archive entry indices across the whole range. The art records' `rate_alt` (`+0x84`) byte is used only as the base-archive marker; playback stepping follows the `+0x78` rate like every other entry (see [battle-data-pack.md § Art-animation bank](../formats/battle-data-pack.md#art-animation-bank-record0-0x58)). Engine assumption: the loop-vs-once bit retail derives from the record kind isn't modelled - staged id `1` (the approach walk) loops, every other staged id plays once.

## Action validator (`FUN_8003FB10`)

The 18-arm gate (`0x00..=0x0D` plus `0x80..=0x83`) the menu / battle UI runs against a candidate
slot before committing the player's action. Selects which validation rule fires from the outer
`param_1` arm (jump table at `0x80014D70`, bound `< 0x84`; unhandled slots return invalid) and,
for arm 6, a sub-case `param_2` through a second 7-entry table at `0x80014F80`. Reads HP / MP /
status / stat caps from the active record - the setup loop caches per-slot
`(hp, hp_max, mp, mp_max)` pointer quads from the battle-actor table `DAT_801C9370`
(`+0x14C/+0x14E/+0x150/+0x152`, 7 slots) when `_DAT_8007B83C == 0x15`, else from the character
records `0x80084708 + slot*0x414` (`+0x106/+0x104/+0x10A/+0x108`, 3 slots) - and writes a
per-slot validity bit at `gp + 0x9A8`. Source:
`ghidra/scripts/funcs/8003fb10.txt`.

Arms (ported wholesale as `legaia_engine_vm::battle_action::validate_action` over the
`ActionValidatorHost` trait, with the `gp + 0x9A8` byte modelled as an explicit `validity_bits`
parameter; the target-relevance arms are additionally re-implemented where they are consumed -
liveness/kind gating in `legaia-engine-core`'s `target_picker`, item-benefit arms in
`inventory_use::effect_benefits_target`):

| arm | meaning |
|---|---|
| `0x00` | Alive AND `hp < hp_max` (heal target). |
| `0x01` | Walk party - set bit per slot that's alive-and-not-full. |
| `0x02` | Alive AND `mp < mp_max` (restore-MP target). |
| `0x03` | Status-flag presence. Battle: `actor[+0x16E] != 0`, returned **without touching the validity byte**; field: record `+0x12E != 0` with the usual clear-then-set bit write. |
| `0x04` | Dead target (Revive item validator). |
| `0x05` | Alive (any-action target). |
| `0x06` | Stat-cap walker - alive slot AND sub-case-picked effective record stat(s) still below cap (strict `<`; character records regardless of mode): 0 = HP max (`+0x104` < 9999), 1 = ATK (`+0x112` < 999), 2 = UDF/LDF pair (`+0x114`/`+0x116` < 999), 3 = SPD (`+0x118` < 999), 4 = INT (`+0x11A` < 999), 5 = MP max (`+0x108` < 999), 6 = all of those plus AGL (`+0x110` < `0x118`). Sub-case ≥ 7 is invalid. |
| `0x07` | Alive (synonym of arm 5; separate code path with no upper bound). |
| `0x08` | Alive AND `(status & 3) != 0` ("can apply paralysis / sleep"). Battle branch skips the validity-byte write; the field branch reads the record status word **signed** and tests `& 0xFFFF0003`, so a status word with bit 15 set validates even with bits 0-1 clear (sign-extension quirk, kept by the port). |
| `0x09` / `0x0A` | Always valid; force the bitmask to the literal `0x07`. |
| `0x0B` / `0x0C` / `0x0D` | Per-slot exact match; only valid when `slot == arm - 0x0B`. |
| `0x80` | Out-of-battle; story flag `0x100000` clear AND system flag 5 clear. |
| `0x81` | Out-of-battle; story flag `0x200000` clear AND system flag 6 clear. |
| `0x82` | Out-of-battle; calls the external item-count validator (`FUN_80046898`). |
| `0x83` | Always valid. |

The retail dispatcher writes a per-slot validity bit at `gp + 0x9A8` with per-arm discipline:
most arms clear their slot's bit before testing and set it on success, arm `0x01` zeroes the
whole byte before its walk, arms `0x09`/`0x0A` force the byte to `7` and `0x0B..=0x0D` overwrite
it with the matched slot's mask, while the battle branches of `0x03`/`0x08` and all of
`0x80..=0x83` never touch it. The engine port (`validate_action`) keeps that discipline via its
`validity_bits` parameter; the consuming menu paths (`target_picker` for battle-target cursors,
`inventory_use` for item-menu greying) additionally surface the same signal where it is read.
The dump's only real callees are the system-flag test `FUN_8003CE64` (arms `0x80`/`0x81`, flags
5/6 in the `DAT_80085758` bank, alongside `_DAT_1F800394` bits `0x100000`/`0x200000`) and the
arm-`0x82` inventory gate `FUN_80046898` - a 3-instruction leaf returning
`*(int *)(gp + 0x2E8) < 0xE0` ("the inventory has room", signed compare against the 224-slot
cap; `see ghidra/scripts/funcs/80046898.txt`), ported as
`battle_action::item_count_gate` over `ActionValidatorHost::inventory_count`. The validator
does **not** call the ability bit-test `FUN_800431D0` (an earlier attribution in
[`battle.md`](battle.md) / `reference/functions.md`).

## Action queue and Tactical Arts trigger ordering

Before `FUN_801E295C` reaches the inner-state machinery, the battle code resolves the player's command-input sequence into a flat **action queue** of [`ActionConstant`](../formats/art-data.md#action-constants) bytes. The queue is built incrementally from directional inputs and accumulated arts; once the player commits, the runtime applies two trigger passes in order (retail: both inside the queue-builder `FUN_801EED1C` - see [the retail queue-builder](#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4)):

1. **Miracle Art match** - if the input command sequence equals the character's Miracle Art command string, the entire queue is replaced with the Miracle Art's replacement string (`L`/`R`/`D`/`U` × 4 → `SpecialStarter` → `art1, art2, ...`). The first 4 directional bytes carry the on-disc MSB-set quirk and are masked to `0x0C..=0x0F`.
2. **Super Art find/replace at tail** - for each chained art the runtime walks all the character's Super Art `find` patterns and replaces the matched tail with a `replace` tail ending in the Super Art's finisher action constant. Triggers require: the last art of `find` is the last action in the queue, and all participating arts paid AP.

Both passes are clean-room ports in `legaia_art::MiracleMatcher` / `legaia_art::SuperMatcher`, applied together by `legaia_engine_vm::battle_action::resolve_action_queue`. The engine-vm `BattleActionHost` exposes an `art_record(char_id, art_id)` callback so the SM can fetch the [art record](../formats/art-data.md) for power-byte resolution, hit timing, and status-effect application during the `0x14..0x20` Attack chain.

### Miracle / Super in the live player-driven Arts submenu

The player-driven battle Arts submenu (`legaia_engine_core::battle_arts`) models an art as a saved **directional chain** (`legaia_save::SavedChainRecord`, raw `0x0C..=0x0F`-equivalent command bytes) rather than an in-gauge buffered input. Two trigger paths interact with that model differently:

- **Miracle Arts are wired.** A Miracle Art's trigger *is* an exact directional-string match (`MiracleMatcher::find`), so `battle_arts::miracle_for_chain` recognises a saved chain whose command string equals the caster's Miracle Art and flags the menu row (`ArtRow::miracle = Some(name)`). `World::build_battle_arts_rows` then resolves the row's per-strike profile from the Miracle's finisher-replacement queue via `resolve_action_queue`: each art constant in the replacement contributes its staged [`ArtRecord`](../formats/art-data.md) power bytes + status effect, or one tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the same graceful-degradation fallback the no-disc-data path uses). The native `play-window` HUD shows the Miracle name on the row.
- **Super Arts are wired, with the queue connectors abstracted.** A Super fires when the player chains several named arts ending on a known combination. `SuperMatcher`'s `find` patterns match the **tail** of a queue with the *interleaved* shape `Starter Art <dir> Starter Art <dir> Starter Art` (e.g. Vahn's Tri-Somersault `find` = `19 27 0F 19 1F 0E 19 27` = `Starter Somersault Up Starter Cyclone Down Starter Somersault`; see [art-data.md](../formats/art-data.md#super-arts) § Super Arts). The live submenu reaches that match in two steps:
  1. **Recognize the named-art sequence.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional `Command` string into the ordered named arts it performs, identifying each by its own `ArtRecord::commands` (greedy longest-match). `battle_arts::super_for_chain` runs this over the caster's loaded art catalog.
  2. **Tail-match the pinned art ordering.** `SuperMatcher::trigger_by_art_sequence` compares the recognized ordering against each Super's `SuperArt::art_sequence()` - the `find` pattern projected to its art constants only (`[0x27, 0x1F, 0x27]` for Tri-Somersault), with the `0x19` starters and the interleaved connector directions stripped. A tail match flags the menu row (`ArtRow::super_art = Some(name)`), and `World::build_battle_arts_rows` resolves the per-strike profile from the Super's finisher-replacement queue (`SuperArt::replace`) through the same `art_actions_strike_profile` helper the Miracle path uses. The `play-window` HUD shows the Super name on the row. Super is checked *after* Miracle, matching the retail "Miracle replacement runs before Super tail expansion" order.

  The match is deliberately **connector-abstracted**. The connector direction after each art is *combo-specific* - the same art appears with different connectors across Supers (Vahn's `0x27` is followed by `0F` in Tri-Somersault but `0E` in Power Slash), so it can't be derived from each art's own commands. The connectors are per-combo *data* in the resident trigger table (below); the live submenu matches the named-art ordering because a saved chain carries no connector bytes, not because the byte-exact strings are unknown.

  **The queue location is now pinned by capture:** it is the per-actor action-parameter byte stream at `actor[+0x1DF..+0x1F2]` - **not** `ctx[+0x274]`, which a capture showed is the turn-order active-actor index written by `recompute_battle_order` (`FUN_801DABA4`: `lbu v0,0x11(v1); sb v0,0x274`).
  Direction/connector bytes encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up and `0x1A` = `SpecialStarter`; a Noa Miracle Art capture read that stream and it matched the engine's modeled replacement string byte-exact (probe `autorun_super_art_action_queue.lua`; runbook [`super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)). A Vahn **Tri-Somersault** capture likewise confirmed the Super path: its resident queue tail `19 27 0F 19 1F 0E 1A 2B 2B 2B` is byte-identical to `super_art.rs`'s `Tri-Somersault` `replace`, validating the combo-specific connectors (`0x27 → 0F`, `0x1F → 0E`) and the finisher tail; the dequeue site is pc `0x801D89D8`.

  **All 15 Supers' `find`/`replace` strings are capture-validated.** The battle overlay keeps the whole trigger table resident; read out of live battle RAM (static-recomp endgame battle state, scene `jou ene`, mode `0x15`) it is:

  - `0x801F64F4` / `0x801F6504` / `0x801F6514` - the three Miracle-Art replacement strings ([art-data.md](../formats/art-data.md#miracle-arts)'s pinned trigger-entry VAs), leading `0x8C/0x8D/0x8E/0x8F` masked-direction bytes intact, byte-exact against `miracle.rs`;
  - `0x801F6524` - the 15 Super `find` entries, fixed 13-byte stride (`[len u8][bytes][zero pad]`), in `super_art.rs` table order (Vahn ×5, Noa ×5, Gala ×5);
  - `0x801F65E8` - the 15 Super `replace` strings, 16-byte stride, zero-padded, word-aligned, same order.

  Every resident string is byte-identical to `super_art.rs`'s modeled `find` / `replace` fields, and every resident replace preserves its find minus the final `[19, art]` pair then appends `[1A, finisher…]` - the pairing law locked by `super_art.rs`'s `replace_preserves_find_prefix_and_finisher_tail` test.
  So the byte-exact connector strings are no longer spreadsheet-only: the resident-table read validates the *strings* for all 15, and the *runtime queue effect* is live-executed for all 15 too - the in-the-wild Noa Miracle / Vahn Tri-Somersault captures above plus a per-Super applier-injection sweep
  (probe `autorun_super_art_queue_inject.lua`; each post-`FUN_801EF9E4` queue at `actor[+0x1DF]` is byte-identical to `super_art.rs`'s `replace`, re-checkable via the `super_queue_replace_*` library states + `crates/pcsxr/tests/super_art_queue_replace.rs`; see [`super-art-queue-capture.md`](../tooling/super-art-queue-capture.md#result---all-15-supers-live-executed-injection-probe)).
  The modeled tables feed the live path through `miracle_row_for` / `super_rows_for`, which project them into the resident row shapes; the queue arithmetic itself is the byte applier's, not `SuperMatcher`'s (see [the retail queue-builder](#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4) below).

### The retail queue-builder (`FUN_801EED1C`) and Super applier (`FUN_801EF9E4`)

The function that turns the player's committed directional chain into the final token stream at
`actor[+0x1DF..]` - emitting the art constants over the raw arrows and applying both trigger
passes against the resident tables above - is **`FUN_801EED1C`** in the battle overlay
(PROT 0898, file `+0x20504`; `see ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`).
The ActionSeed state `0x0C` of `FUN_801E295C` calls it for the acting party slot
(`jal 0x801EED1C` at `0x801E2C7C`, slot from `ctx[+0x274]`; a second site at `0x801E369C`
re-invokes it for the next queued actor of a multi-actor turn). The full retail chain:

1. **Preseed from the saved chain.** `FUN_801DA34C` (leaf, no frame; called from the round
   driver `FUN_801D0748` at `0x801D15C8`/`0x801D1734`) copies one of the character's two saved
   16-byte arts-input strings - char record `+0x76F` or `+0x77F` off `0x80084140 + (id-1)*0x414`
   (`lbu v0,0x76f(v1)` `0x801DA3F8`, `lbu v0,0x77f(v0)` `0x801DA4F8`) - byte-for-byte into
   `actor[+0x1DF..+0x1EE]` (`sb v0,0x1df(v1)` at `0x801DA404` / `0x801DA454` / `0x801DA504`),
   or zero-fills the queue when the selected slot is empty (`sb zero,0x1df` at
   `0x801DA490`/`0x801DA540`/`0x801DA584`). Slot pick + fallback are **asymmetric**: the u16
   pair `actor[+0x154]`/`[+0x156]` selects the leg (`sltu` at `0x801DA3A4`) - the
   `[+0x156] < [+0x154]` leg prefers the first string and falls back to the second when its
   head byte is zero, while the other leg reads only the second string and zero-fills on an
   empty head with **no** fallback (`beq` at `0x801DA4CC` lands on the `0x801DA51C` zero-fill,
   never on a `+0x76F` copy). The whole copy is gated on the stage byte `DAT_8007BD04`
   (zero → zero-fill, `0x801DA378`). Live pad edits during the Arts gauge then mutate the
   same bytes in place. Byte-level port: `legaia_engine_vm::battle_action::preseed_action_queue`.
   The **write-back twin** is `FUN_801DA59C`: after an arts action (category `+0x1DE == 3`,
   live actor), it copies `actor[+0x1DF..+0x1EF]` back into the char record's chain slot -
   the same `[+0x156] < [+0x154]` predicate picks `+0x76F` vs `+0x77F` (`sb` loops at
   `0x801DA638`/`0x801DA69C`), with no head-byte fallback: exactly one slot is overwritten.
   That is what the next preseed replays. Port:
   `legaia_engine_vm::battle_action::save_action_queue`.
2. **Normalize arrows into art constants.** `FUN_801EED1C`'s player path walks the queue,
   matches each token run against the character's art command table (token compare via
   `addiu v1,v1,-0xb` at `0x801EF3E8` - the queue's `0x0C..0x0F` arrows against the art table's
   `0x01..0x04` direction bytes), and rewrites a fully-matched run to its art action constant:
   `addiu v1,t3,0x18; sb v1,0x1df(v0)` at `0x801EF6F0`/`0x801EF6F8` (art row index + `0x18` →
   the `0x1B..` constant band), compacting the remaining bytes down and keeping the 16-entry
   per-token side array at `0x801F6990` in sync (shift loops `0x801EF69C..0x801EF6B0` and
   `0x801EF730..0x801EF744`). Each accepted art is validated against the character's learned
   list by `FUN_801EFBFC` (`jal` at `0x801EF44C`; count at char record `+0x74D`, ids at
   `+0x74E..`), pays its AP (`lhu/subu/sh +0x170` at `0x801EF490..0x801EF49C`) and accrues the
   spent counter `+0x224` (`0x801EF4B4`). `FUN_801EFBFC` is more than a membership check - it is
   also the **arts learn-on-use inserter**: when the id is absent it returns `2` after an
   ascending-sorted insert into `+0x74E..` (shift loop `0x801EFD64..0x801EFDB0`, count bump
   `0x801EFE24`), but only for ids **above** the per-character innate cap at
   `0x801F686C + char_id - 1` (`sltu` at `0x801EFD14`; the zero id passes the gate as an edge)
   and only when the learn gate opens: `actor[+0x266] == 0`, **or** a 1/512 roll
   (`FUN_80056798() & 0x1FF == 0` at `0x801EFCC4`), **or** the debug byte `DAT_8007BD0C == 'O'`
   (`0x801EFCD4`). Returns `1` when already known, `0` when unknown and not learnable.
   Byte-level port: `legaia_engine_vm::battle_action::check_and_learn_art`.
3. **Miracle replacement (inline).** When the slot's Miracle marker `ctx[+0x25F + slot]` is set
   (`lbu v0,0x25f(v0)` at `0x801EF4C8`), the builder overwrites the whole 16-byte queue from the
   character's Miracle replacement string - the loop at `0x801EF4E8..0x801EF524` copies from
   `0x801F64F4 + (char_id-1)*0x10` (`addiu a1,v0,0x64f4` at `0x801EF4EC`; `sb v0,0x1df(v1)` at
   `0x801EF518`), i.e. the three resident strings at `0x801F64F4/0x6504/0x6514`, then flags
   `ctx[+0x28D + slot] = 1` and the shared trigger flag `0x801F696C = 1` (`0x801EF5A8`/`0x801EF5B4`).
4. **MSB clear + marked-starter reorder.** After the build loop the builder sweeps the 16-byte
   queue window clearing bit 7 of every byte (`0x801EF85C..0x801EF898`). It does it with a
   signed load and an *add*, not an AND: `lb v0,0x1df(a0)` / `lbu v1,0x1df(a0)` /
   `bgez v0, skip` / `addiu v0,v1,0x80` / `sb v0,0x1df(a0)` - adding `0x80` to a byte that
   already has bit 7 set wraps it off, so the effect is `& 0x7F`. This is the runtime half of
   the on-disc MSB quirk: it is what turns the Miracle row's leading `0x8C..0x8F` direction
   bytes back into `0x0C..0x0F`, and it runs **after** the Miracle copy of step 3 and **before**
   the Super applier of step 5. A second pass at `0x801EF8A0..0x801EF968` then walks the side
   array `0x801F6990` and, for each marked index `i > 0` whose queue byte is a `SpecialStarter`
   (`0x1A`), scans `j < i` and swaps `queue[j]` with `queue[i]` wherever
   `queue[j + 1] == queue[i + 1]` - no early exit, so the swap can fire more than once per `i`.
   The marks it reads come from the build loop, not from the Super applier, which runs later.
   Port: `legaia_engine_vm::battle_action::clear_queue_msb` (the reorder is not ported).
5. **Super find→tail-replace (helper call).** At its end (`jal 0x801EF9E4` at `0x801EF9AC`) the
   builder invokes **`FUN_801EF9E4`** (file `+0x211CC`;
   `see ghidra/scripts/funcs/overlay_battle_action_801ef9e4.txt`), which measures the queue
   (zero-terminator scan over `+0x1DF..` at `0x801EFA14..0x801EFA30`), then for each of the
   character's five Super rows compares the `find` pattern - `0x801F6524 + row*13 + char*65`
   (`addiu t6,v0,0x6524` at `0x801EFA3C`; 13-byte `[len][bytes...]` entries) - against the
   queue **tail** (`queue[len - find_len + j]`, the `subu v0,t4,a3` indexing at `0x801EFAC8`).
   On a full match it overwrites that tail from the `replace` table `0x801F65E8 + row*16 + char*80`
   (`addiu t8,v0,0x65e8` at `0x801EFA5C`; `sb a1,0x1df(v0)` at `0x801EFB7C`), marks the side
   array `0x801F6990[pos] = 4` for each written `0x1A` SpecialStarter (`0x801EFB84..0x801EFBA8`)
   and sets `0x801F696C = 1` (`0x801EFBD4`). Miracle-before-Super ordering is therefore
   structural: the Miracle branch runs inside the builder body, the Super applier only at its end.
   Two more of its laws matter to a byte-faithful mirror: rows are scanned **in table order and
   the first full match wins** (the match path exits the row loop by forcing the counter to 5),
   and the replace copy stops at the replace string's own terminator **without re-terminating
   the queue** - a replace longer than its find legally spills past byte 16 of the 19-byte
   stream. Byte-level port: `legaia_engine_vm::battle_action::apply_super_tail_replace`
   (equivalence with the structural `SuperMatcher` over the shipped tables is test-asserted).
   The applier is called **unconditionally**, including after a Miracle replacement - the
   Miracle row's tail matches no `find` row, so the two do not interact - and it applies at most
   one replace per builder invocation: the row loop exits on the first full match, and the
   builder calls it once.

The engine's entry point `legaia_engine_vm::battle_action::resolve_action_queue` - what
`engine-core` calls once per committed arts input - runs steps 3, 4 and 5 in that order on a raw
`ACTION_QUEUE_CAP`-wide byte window, so the live path is the byte applier's arithmetic rather
than the structural `legaia_art` matchers'. Two retail laws that reach the simulation through
that change: the Super scan takes the **first matching row in resident-table order** (not the
longest `find`), and it applies **once** (not to a fixpoint). Retail's Miracle gate is the
per-slot marker `ctx[+0x25F + slot]`, armed by the input recognizer; the engine's stand-in is a
whole-string match against the character's Miracle command table, because that recognizer
(`FUN_801E91E8`'s caller) is not ported.

The consuming side is unchanged: the strike loop reads `actor[+0x1DF + +0x15]` and the round
driver's queue clear runs the `sb zero,0x1df(v0)` loop at `0x801D89D8` inside `FUN_801D88CC`
(called from `FUN_801D0748` at `0x801D0E84`/`0x801D0ED0`). `FUN_801EED1C`'s non-player heads
(the Tetsu-tutorial forced chain `0E 0F 0E 0F` at `0x801EEDE0..0x801EEE04`, the char-id-4
auto-AI block below) share the same emission sites.

When the active actor's `chosen_art` is set and `art_record` returns a record, `attack_chain` (state `0x1A`) calls a second host hook `apply_art_strike(ArtStrikeInfo)` alongside the existing `apply_damage`. `ArtStrikeInfo` carries the strike-indexed power byte, dmg_timing, hit cue, and the art's flat status effect. Engines drive HP deduction, status application, sound-effect scheduling, and visual hit-cue dispatch off this struct; tests feed synthetic `ArtRecord` instances and assert the per-strike `(power, timing, effect, cue)` resolution rather than going through `apply_damage`'s legacy `(icon, page, target, slot)` parameter pack.

The engine-side translator at `crates/engine-core/src/art_strike.rs` (`apply_art_strike(attack, defense, info) -> ArtStrikeOutcome`) folds an `ArtStrikeInfo` into a concrete HP delta + status flag + scheduled SFX cues using the `art_strike_damage` formula in `legaia_engine_vm::battle_formulas`. The world's `BattleActionHost::apply_art_strike` impl resolves the per-slot weapon attack from `World::battle_attack` and the right defense (UDF or LDF, picked from `World::battle_defense_split`) before calling the translator, then emits a `BattleEvent::ApplyArtStrike` with the resolved `ArtStrikeOutcome`. Engines apply each strike's `damage` / `enemy_effect` / `cues` through whatever runtime they have for HP / status / SFX dispatch.

`World::fold_battle_event` folds the `ApplyArtStrike` outcome: HP / status into the target, and the outcome's **sound cues** (`cue.is_sound()`, the `HitCue::kind` SfxBank ids - distinct from the move-power `+0x0d` `FUN_8004fcc8` namespace) into a per-frame `BattleSfxCue` queue the host drains via `World::drain_battle_sfx_cues` (the audio sibling of `drain_battle_hit_fx`). The host plays each through `SfxBank::play_one_shot` at the cue's `timing_frames` delay. The live battle loop wires this end to end: the SFX bank is decoded from the user's executable at boot and the cues key on through the per-scene VAB (see [`battle.md`](battle.md#sfx-bank--scheduler)).

### Spirit / Run in the live command menu

The live player-driven command menu (`legaia_engine_core::battle_input::BattleCommand`) carries
all six commands: Attack (target cursor + physical strike), Arts / Magic / Item (host-submenu
hand-offs), **Spirit** and **Run**. Spirit resolves without a target: the live loop charges the
caster's AP gauge (`ApGauge::charge_spirit`, the retail Square-press +5) and raises a per-slot
guard stance - the engine model of the retail pending-action byte `+0x1DE == 4` the damage
finisher's guard-halve stage reads (`DamageFinish::defender_guarding`, `over >>= 1`) - held
until that actor's next turn starts. Run arms the ported run band (`Begin` -> category 5 ->
`RunBegin`/`RunWait`/`RunEscape`) with the roll outcome staged on `multi_cast_gate`; a success
tears the battle down `Escaped` (no loot, downed members floored alive at 1 HP), a failure
consumes the turn through the Done band. The escape *probability* is the retail
[`FUN_801E791C` roll](#the-escape-roll-fun_801e791c) - party vs enemy speed/missing-HP scores
plus the two Chicken accessory bits - ported as `battle_formulas::escape_roll` and rolled by
`World::roll_battle_escape`.

## Pose-slot table `0x80076C10` and its copy helpers

The battle animation dispatcher [`FUN_801D388C`](../reference/functions.md)
keeps its per-slot animation state in a **24-byte-stride record array
based at `0x80076C10`**. Two small overlay leaves move records around
inside it; both take `(dst_index, src_index)`, address `0x80076C10 +
index * 0x18` by the `(i*2 + i) << 3` idiom, and return nothing.

| Function | Fields written into `dst` |
|---|---|
| `FUN_801D57E8` | straight clone of `+0x02`, `+0x04`, `+0x06`, `+0x0A`, `+0x0C` (u16) and `+0x14` (u32) |
| `FUN_801D5778` | re-mapped clone - see below |

`FUN_801D57E8` is the plain copy: five halfwords plus the word at `+0x14`.
It deliberately leaves `dst`'s `+0x00`, `+0x08`, `+0x10` and `+0x12`
alone, so it is a **partial** clone, not a `memcpy`.

`FUN_801D5778` copies the same record but permutes and biases three of the
fields: `dst[+0x02] = src[+0x0A]`, `dst[+0x04] = src[+0x0C]`,
`dst[+0x06] = src[+0x06]`, `dst[+0x0A] = src[+0x0A] - 0x140`,
`dst[+0x0C] = src[+0x0C]`, `dst[+0x14] = src[+0x14]`. The literal `0x140`
is 320, the PSX display width.

The camera / view director `FUN_801D5854` performs the *same* moves
inline between the two adjacent records at `0x80076C10 + 0x3D8` and
`+ 0x3F0` (records 41 and 42), which is what fixes the 24-byte stride
independently of the two helpers, and it is also where `+0x14` is shown
to hold a pointer: `FUN_801D5854` stores `actor + 0x1BC` there before
feeding it to the per-actor animation lookup `FUN_80035F04`.

Both helpers are called only from `FUN_801D388C` - `FUN_801D57E8` from its
`0x801D4414` / `0x801D4434` sites, `FUN_801D5778` from `0x801D50A0` /
`0x801D50F8`. Evidence grade: **Confirmed** for the field moves and the
call sites (disassembled from PROT entry 0898 at base `0x801CE818`);
**Unknown** for what the individual halfwords mean, beyond `+0x14` being
the animation-descriptor pointer.

## Overlay-local PRNG `FUN_801D0290`

The battle-action overlay carries a second random-number generator,
distinct from the SCUS PsyQ-shape `rand()` at `FUN_80056798` that
[battle-formulas.md](battle-formulas.md#rng-primitive) documents. It is
twelve instructions with no frame, and its whole state is the word at
`0x801F6950` (the overlay's own data tail):

```
s = *0x801F6950
v = s * 12 + 2              ; (s << 2) + (s << 3) + 2
s = (v << 16) + (v >> 16)   ; 32-bit rotate by 16
*0x801F6950 = s
return s
```

The multiply-add is done with shifts, and the "rotate" is an `addu` of the
two shifted halves rather than an `or`, so a carry out of the low half
propagates - the result is not a pure rotate. Five call sites, all in the
overlay's leading function, none in SCUS.

Because its state lives in overlay memory rather than the SCUS RNG seed,
draws from this generator do **not** perturb the `FUN_80056798` stream the
determinism oracles follow. Which battle quantities it feeds is
**Unknown**; the arithmetic and the state address are **Confirmed** from
the disassembly of PROT entry 0898 at base `0x801CE818`.

## Open work

- **Unlisted `ctx[7]` states are inert/reserved, not a crash (`FUN_801E295C`).** The state byte dispatches through a 256-entry `jr` jump table at `0x801CED44` with **no `default`** (`sltiu v0,ctx[7],0x100; jr v0`; see `ghidra/scripts/funcs/overlay_0898_801e295c.txt`). The handled states are `0x00`, `0x0A`–`0x0C`, `0x14`–`0x19`, `0x1E`–`0x20`, `0x28`–`0x2E`, `0x32`–`0x38`, `0x3C`–`0x40`, `0x46`–`0x48`, `0x50`–`0x52`, `0x5A`, `0x64`–`0x66`, `0x68`–`0x6B`, `0x6E`–`0x71`, `0xFD`, `0xFF`. Every other byte value in `0x00`–`0xFF` has no case body: its table slot falls straight to the shared post-switch epilogue (the knockback/shove settle at `0x801E6814`), a safe no-op advance, never an out-of-bounds jump.
- The inert indices are the inter-band gaps (`0x07`, `0x21`–`0x27`, `0x39`–`0x3B`, `0x41`–`0x45`, `0x49`–`0x4F`, `0x53`–`0x59`, `0x5B`–`0x63`, `0x6C`–`0x6D`, `0x72`–`0xFC`) plus the low-band ones. No path in the dumped battle-overlay corpus writes any of them into `ctx[7]` (corpus-scoped: a value injected by an un-dumped overlay would still dispatch safely). **One exception:** state `0x67` **is** written (case `0x66` sets `ctx[7] = 0x67`) yet has no case body — a genuine written-but-inert state that also lands on the epilogue.
- State `0x47` (spirit-arts sustain): the `actor[+0x1F9] != 0` "spirit shield" branch is **resolved**. `+0x1F9` is set by the damage-application primitive `FUN_800402F4` case 5 (spirit-shield spirit → `+0x1F9 = 1`, gated on a non-zero target roll) and cleared by case 4 (cleanse → `+0x1F9 = 0`). Which case runs is selected by `actor[+0x1E8]`, seeded at [state `0x3C`](#state-table) from the spell table's class byte (`DAT_800754C8 + spell_id*0xC + 0`): class `== 5` routes to the shield write, class `== 4` to the cleanse. So the specific spirit that raises the shield is disc-side spell-table data, not a runtime constant. See [`spell-table.md`](../formats/spell-table.md).
- **The `0x51` HP-bar settle gate is decoded and its softlock is reproducible; its retail trigger is not.** Mechanism and measurements: [the section above](#the-0x51-exit-gate-and-the-hp-bar-settle-invariant). Both first-stated generators are measured out on the Gaza 2 fight (clamp asymmetry = amplifier; the revive race starved by phased crediting - twelve retail revives, every assign on a drained accumulator); the narrowed open questions live in [open-rev-eng-threads.md](../reference/open-rev-eng-threads.md#endless-orbit---what-remains-open). Every park captured so far needed an external HP write to set up.
- `FUN_801E7250` (`0x51`) and `FUN_801E7824` (`0x68`) are decoded from their `overlay_battle_action_*` dumps: the former is the **HP-bar drain settle check** (the `0x51` arm freezes the `ctx[+0x6D8]` countdown while any relevant actor's live HP `+0x14C` differs from its bar display value `+0x172`), the latter the **captured-monster takedown** (queued anim from the monster record, HP pair + facing zeroed, retarget to `8`, run-UI banner opened). Both ported in `crates/engine-vm/src/battle_action.rs`; see [`reference/functions.md`](../reference/functions.md).

## See also

**Reference** -
[Battle scene loader](battle.md) ·
[Damage / accuracy formulas](battle-formulas.md) ·
[Move-table VM](move-vm.md) ·
[Effect VM](effect-vm.md) ·
[Art records](../formats/art-data.md)
