# Battle action state machine

A two-level finite state machine that drives the per-actor execution of a chosen battle action - the layer between "the player picked Attack" and "the actor's body has finished swinging the sword and HP has been deducted." Lives in the battle overlay (`0898`, RAM-resident at `0x801C0000+`). The driver is `FUN_801E295C`, dumped as `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt` (16 KB / 4099 MIPS instructions / 155 outgoing calls - the largest function in the battle overlay).

## Contents

- [One-paragraph overview](#one-paragraph-overview)
- [Outer dispatch - `ctx[7]` action-state cursor](#outer-dispatch---ctx7-action-state-cursor) · [state table](#state-table)
- [Inner dispatch - actor action category](#inner-dispatch---actor-action-category) · [per-actor sub-state surface](#per-actor-sub-state-surface)
- [Cross-references with other battle helpers](#cross-references-with-other-battle-helpers) - [range/LOS](#fun_8004e2f0---battle-range--line-of-sight) · [stat aggregator](#fun_80042558---per-frame-stat-aggregator) · [effect spawn API](#fun_801dfdf8---effect-bundle-public-spawn-api) · [summon-overlay dispatch](#seru-magic-summon-overlay-dispatch) · [pose driver](#fun_801d5854---per-actor-pose-driver) · [party/monster setup](#fun_801eed1c--fun_801e7320---party--monster-setup-hooks) · [camera bounds](#fun_801efe44---battle-camera-bounds) · [escape roll](#the-escape-roll-fun_801e791c) · [helper functions](#battle-helper-functions)
- [Notes for the engine port](#notes-for-the-engine-port) · [decompile quirks](#decompile-quirks-worth-knowing) · [engine port](#engine-port)
- [Action validator (`FUN_8003FB10`)](#action-validator-fun_8003fb10) · [action queue + Tactical Arts trigger ordering](#action-queue-and-tactical-arts-trigger-ordering) · [Miracle / Super in the live Arts submenu](#miracle--super-in-the-live-player-driven-arts-submenu) · [open work](#open-work)

## One-paragraph overview

`FUN_801E295C` runs every frame from the [battle main dispatcher `FUN_801D0748`](../reference/functions.md). It picks up the global battle context (`_DAT_8007BD24`, a pointer to the live ctx struct at `0x800EB654`), resolves the **active actor** via `(&DAT_801C9370)[ctx[0x13]]` (the [8-slot battle actor pointer table](battle.md) - slots 0..2 party, 3..7 monsters), and pumps the action through three nested keys:

1. **Action category** - the actor's `+0x1DE` byte: 0=Martial Arts (Tactical Arts), 1=Item, 2=Magic, 3=Attack, 4=Spirit, 5=Run/Defend. Read once at the action-start case (`ctx[7] == 0xC`) and used to seed the next state.
2. **Execution phase** - `ctx[7]`, the action-state cursor. The outer `switch (ctx[7])`. States are numbered to bin by action category: `0x14..0x20` = Attack chain, `0x28..0x2E` = Magic / Item, `0x32..0x38` = Summon, `0x3C..0x40` = Spirit, `0x46..0x48` = Spirit-arts variant, `0x50..0x52` = Done / cleanup, `0x5A` = end-of-action gate, `0x64..0x6B` = Run / capture-fail, `0x6E..0x71` = Capture sequence.
3. **Per-actor sub-state** - `actor[+0x1DC]` (flag bits - `0x01` "windup done", `0x02` "advance done", `0x04` "exit"), and several per-actor scratch fields (`+0x1DA` queued anim ID, `+0x1D9` current anim ID, `+0x1DF..+0x1F2` action-parameter byte stream).

The function is not a bytecode VM. There is no opcode table, no PC stride. It is a **per-frame edge-triggered state machine**: each `case ctx[7]` body waits on a per-actor condition (animation matched, timer expired, distance check passed) and writes the next `ctx[7]` value when ready. Actions that need multiple frames (most) do nothing on the frames where their condition isn't met yet.

## Outer dispatch - `ctx[7]` action-state cursor

`ctx[7]` is the **execution phase** byte at `_DAT_8007BD24[7]`. The runtime models it as a `byte`, but the value range is sparse: 47 distinct cases fall into 7 contiguous bands (one per action category). The dispatcher is a single MIPS jump table at `0x801E29A8 + (ctx[7] << 2)` with `sltiu` bound `0x100`.

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
| `0x00` | Action begin | Resets ctx counters at `+0x6DA..+0x6DB`; copies `ctx[+0x274]` (the active-actor index set by `recompute_battle_order`) → `actor[+0x1A]`; clears `ctx[+0x290]`. | `0x0A` (or `0x0B` if `ctx[+0x276] != 0` is set, i.e. action queued from menu). |
| `0x0A` | Pre-action wait | Calls `func_0x8003F2B8(1)` (likely a "pause until previous animation cleared" gate). | `0x0C` when ready, else stays. |
| `0x0B` | Action queued from menu | Holds while `ctx[+0x276] != 0` (menu still open). | `0x0A` once cleared. |
| `0x0C` | **Action seed** - reads `actor[+0x1DE]` (action category) and dispatches into the appropriate band. Calls `FUN_801EED1C` (party setup; slot < 3 unconditionally) or, for a monster slot with the `+0x16E & 0x380` bits, `FUN_801E7320` (random-retarget: the rolled action - including a Magic cast - is kept, only its target re-rolls to the opposite side; see the [`0x380` notes](#ai-delegated-0x380-party-members---what-is-and-isnt-pinned)). Reads RNG via `func_0x80056798()`. Calls `FUN_801EFE44` (camera bounds) and `FUN_801D5854(actor_id, 6)` (idle pose) unless `+0x1DE == 5` (run). The inner switch on `actor[+0x1DE]` is the "action category" dispatch - see [Inner dispatch](#inner-dispatch---actor-action-category). | `0x14`/`0x28`/`0x3C`/`0x46`/`0x50`/`0x64`/`0x68` per category. |
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
| `0x34` | Summon - actor freeze | `FUN_801DC0A0(party, 0x12)`. When `actor[+0x1D9] == 0`: OR's the fade primitive bit `8`, clears `ctx[+0x278/+0x279]`, sets `ctx[+0x6D8] = 0x78` (timer), calls `func_0x801F1ED4` (the [summon actor/camera re-frame](#battle-helper-functions)), iterates the 8-actor table to clear `actor[+0x4]` and set `+0x21C = 0xFF` (actor-hidden marker). Writes a second fade descriptor (`0x78` time, alpha `0xFF→0`). | `0x35`. |
| `0x35` | Summon - sustain | Decrements `ctx[+0x6D8]`; ramps screen brightness `_DAT_8007B910` down by `DAT_1F800393` per frame, clamped at `(_DAT_8008457C * 0x4B) / 100` (75%) for spells < 0x99 or 50% for higher. If `+0x6D8 < 0` and `ctx[+0x276] != 0`, force-clamp `+0x6D8 = 1`. | `0x36` when timer expires. |
| `0x36` | Summon - return-from-fade | Runs `func_0x801F1ED4` (the [actor/camera re-frame](#battle-helper-functions); void) again while the fade settles. Then iterates 8-actor table clearing `+0x21C = 0` and resetting `+0x8 = 0x81000000` for actors with `+0x4 == 0`. Calls `FUN_801E70BC` (the summon-magic level-up check - see [`reference/functions.md`](../reference/functions.md); engine `World::accrue_summon_spell_xp` + `battle_formulas::summon_magic_levels_up`). | `0x37`. |
| `0x37` | Summon - verify all alive | `FUN_801D5854(actor, 6)`. Iterates the 8-actor table (party + active monsters); checks each is alive (`+0x14C != 0` AND `+0x1D9 != 0`). Sets a 4-byte fade-back-in sentinel at `ctx[+0x890..+0x893]` (`84 10 42 08`). | `0x38`. |
| `0x38` | Summon - done | OR's the fade primitive bit `8`; clears `DAT_801C938C[+0x22C]`. | `0x50`. |
| `0x3C` | **Spirit / Item - pre-arm** | `FUN_801D5854(actor, 6)`. Sets `actor[+0x1DA] = actor[+0x1E7]` (queued anim). Sets `ctx[+0x243] = 1` ("action in progress" marker). For `+0x1DE == 1` (Item): looks up item record at `ctx[+0x1DF]*0xC + -0x7FF8BC97` for label/icon; writes `actor[+0x1E8/+0x1E9]` (icon page/x); writes HUD via `_DAT_80077332..+0x35C`. Special case: `actor[+0x1DF] == 0xFE` (Pomander) → label = `s_Points_returned_801CED34`. For non-Item (Magic/Spirit, `+0x1DE != 1`): does the same write of `+0x1E8/+0x1E9` from the spell table at `actor[+0x1DF]*0xC + -0x7FF8AB38`, computes MP cost (with ability-bit half/quarter), subtracts from `actor[+0x150]`; for party_id < 3 fires `FUN_801D8DE8(7, 0)` (UI element). Always fires `FUN_801D8DE8(0x4C, 0)` (HUD label). | `0x3D`. |
| `0x3D` | Spirit - wait | `FUN_801D5854(actor, 6)`. Holds while `actor[+0x1DA] != actor[+0x1D9]`. When matched, clears `actor[+0x1DA]`, calls `func_0x801F3990` (the [per-move damage roll](#battle-helper-functions) `FUN_801F3894` - move-power table + RNG → damage). | `0x3E`. |
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
| `0x70` | Magic-capture - phase 2 | Same brightness ramp as `0x6F`. Waits on `func_0x801F2160`. When done, calls `func_0x801F0348` (the [widget-pool teardown](#battle-helper-functions) `FUN_801F02D0`). | `0x71`. |
| `0x71` | Magic-capture - finalize | `FUN_801D5854(actor, 6)`; checks all 8 slots are settled (alive with non-zero `+0x4`, or non-`8` `+0x1D9`). Once stable: clears ctx buffers, writes the 4-byte fade sentinel (`84 10 42 08`), iterates resetting per-actor `+0x21C = 0` and `+0x8 = 0x81000000`. | `0x50`. |
| `0xFD` | Idle hold (battle paused?) | `FUN_801D5854(actor, 8)`. No state change. | (stays). |
| `0xFF` | **Battle complete** | Sets `ctx[+0x6] = 0x14`, increments `ctx[+0x28A]` (battle-count?), calls `func_0x801F45A4` (the [end-of-action damage/HP-bar settle](#battle-helper-functions) `FUN_801F452C`). | (terminal - exits the battle overlay). |

States `0x67` (post-fade hold), `0x07` (unused?), and several gaps in the table are not present as case labels - they fall into the `default` no-op arm (the dispatcher's `sltiu v0, v1, 0x100` bound is 256 and the JT slot for unhandled values points at the function epilogue at `0x801E6814`).

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
| `+0x14C` | u16 | Liveness flag (non-zero = alive). Read by every state's "is target valid" check. |
| `+0x16E` | u16 | Per-actor flag bank. Bit `0x4` = "non-targetable", bit `0x380` = "AI-controlled", bit `0x404` = "AI + non-targetable". Read at state-`0x0C` to decide between `FUN_801EED1C` and `FUN_801E7320`. |
| `+0x172`/`+0x174` | u16 | HP / MP (or current / max - see [battle.md](battle.md)). |
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

The single most-cited helper inside `FUN_801E295C` (~30 call sites). Signature `FUN_801D5854(actor_id, pose_id)`. Pose IDs surfaced:
- `6` = idle / breathing
- `7` = ready / pre-action
- `8` = action-end / hit-recovery
- `9` = defeat / down

It is a **camera/presentation program driver**, not the animation system: its body dispatches `pose_id` 0..9 through a jump table at `0x801CEA00` computing three i16[3] tween-target vectors handed to `0x801D7130` (with a secondary dispatch on `actor[+0x1DB]` values `0x11..0x18` - per-art camera variants for the dynamically-installed art anims). It never writes `+0x1D9/+0x1DA`; the same-numbered **anim** ids 7/8/9 are staged separately (by the SM's own `+0x1DA` stores and the `FUN_8004AD80` end-of-clip chains), and the anim system's idle id is `0` - pose 6 has no anim counterpart (record[0] entry 6 is empty in every player file). The two id spaces are designed to align numerically at 7/8/9, which is what made the conflated reading stick.

Note that `FUN_801D5854` for `param_2 == 9 && param_1 == 7` (the only path that calls the special-case) writes pose 9 unconditionally and triggers the run-side animation lookup `FUN_801DB9C4`.

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
the scripted no-escape flag `ctx[+0x287]` still blocks it, which is why Chicken King is
"assured escape (non-boss)" (see the
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
(BIOS-rand) roll - so `ctx[+0x291] == 2` (which forces the party roll equal to the enemy roll) is
a per-formation "escape assured" flag set at battle setup (`see
ghidra/scripts/funcs/80051d84.txt`).

On success the routine also stages the flee scene: every party actor is marked fleeing
(`+0x1DA`/`+0x1DC` = 1, facing `+0x46` = `0x800`, pose byte `+0x1DD` = 9), positions are
pulled toward the camera and spread at least 200 units apart, live HP/MP are written back to
the character records with downed members **floored at 1 HP** (the record-side half of the
state-`0x64` floor), and the camera move fires via `FUN_801D829C`. Ported:
`engine-vm::battle_formulas::escape_roll` (+ `escape_party_score` / `escape_enemy_score` /
`EscapeFlags`), rolled live by `engine-core::World::roll_battle_escape` when the command
menu resolves Run.

### Battle helper functions

Four helpers `FUN_801E295C` reaches by their mid-body label addresses (`0x801F0348` /
`0x801F1ED4` / `0x801F3990` / `0x801F45A4`); each label sits inside a function whose entry is
earlier (`0x801F02D0` / `0x801F1CC8` / `0x801F3894` / `0x801F452C`). All decoded from their dumps.

**`FUN_801F02D0` (label `0x801F0348`) - battle-UI widget-pool teardown.** `see
ghidra/scripts/funcs/overlay_0897_801f0348.txt`. Walks the 40-slot (`0x28`) tracked-widget table
at ctx `+0x11B4` (stride `0xC`): for each slot whose flag byte `+0x11B7` is set and whose parallel
live-widget pointer at ctx `+0x1074 + slot*4` is non-null, releases the widget via
`func_0x800319A8(widget[+8])` (the UI-element free call, id at `widget+0x8`), then zeroes the
widget pointer and the `+0x11B4`/`+0x11B7` flag bytes; finally clears 16 words of scratch at
`0x801C8FA0`. Called at action-begin (`0x0C`) and capture-finalize (`0x70`/`0x71`) to drop
leftover damage-popup / label widgets. It is a general widget-pool sweep, not a capture-specific
routine.

**`FUN_801F1CC8` (label `0x801F1ED4`) - summon actor/camera re-frame.** `see
ghidra/scripts/funcs/overlay_0897_801f1ed4.txt`. Computes the bounding box of all live actors'
ground positions (`actor[+0x34]` = X, `actor[+0x38]` = Z) across the 8-slot table (party slots
`0..2` unconditionally, monster slots gated on `+0x14C` alive), subtracts the box center from every
actor's position, and adds the center to the world/camera anchor globals `_DAT_80089118` (X) /
`_DAT_80089120` (Z) - re-centering the whole cast on its centroid. When the caller's angle/zoom
delta (`in_t1 - in_t2`) exceeds `0x800` it additionally pre-divides each actor's Z and
`_DAT_80089120` by that delta (a Z compression). It returns void, so the state-`0x36` "waits on it
returning 0" reading was an inference - it is the per-frame summon framing pass. The summon
**creature spawn** is a separate mechanism (the `summon.dat` applier `FUN_801F12D0` /
`FUN_801F19EC`, see [summon-readef](../formats/summon-readef.md)).

**`FUN_801F3894` (label `0x801F3990`) - per-move damage roll.** `see
ghidra/scripts/funcs/overlay_0897_801f3990.txt`. `int f(move_id, attacker_slot, defender_slot)`.
Indexes the move-power table `DAT_801F4F5C` (26-byte / `0x1A` stride) by `move_id & 0xFF`, draws
several `func_0x80056798()` (BIOS-rand) rolls, forms an attacker score from the move-power record's
bitfields (`>> 0x10`/`0x11`/`0x12`/`0x13`) plus attacker stats (`+0x14C`, `+0x168`) and a defender
score from the target's stats (`+0x14C`, `+0x15C`, `+0x160`, `+0x168`), runs them through
`FUN_801DD864` (scale) + `FUN_801DDB30` (finish/clamp), and returns `attacker_score -
defender_score` = the damage. The `attacker_slot == 7` arm additionally formats a decimal digit
string into the caller's buffer via `FUN_801EC964` (an on-screen number). So the state-`0x3D`
call is the spirit/magic **damage computation**, not an "init / Originals flash".

**`FUN_801F452C` (label `0x801F45A4`) - end-of-action damage / HP-bar settle.** `see
ghidra/scripts/funcs/overlay_0897_801f45a4.txt` (disasm only; the Ghidra decompile times out). Per
action category (`actor[+0x1DE]` `1..6`) it tests the actor's ability bits in the character record's
`+0xF4`/`+0xF8` bitfield (base `0x80084140 + (char_id-1)*0x414`, fields `+0x6BC`/`+0x6C0` = record
`+0xF4`/`+0xF8`) and, when set, ramps a value pair `*s0` toward `*s2` by half per pass (`*s0 += (*s2
- *s0) >> 1`) - the HP-bar / damage-number settle. It applies the AP-boost bits (`+0x200`/`+0x100`)
to `actor[+0x170]` and clamps it at 100 (`0x64`) - the same adjust-and-clamp the `0x50` Done arm
performs - clears status-word `actor[+0x16E]` bits, resets brightness/screen globals, and ends in a
per-actor jump-table dispatch keyed on `actor[+0x1D]`. Called at battle-complete (`0xFF`); it is the
final damage/HP settle + ability-effect application, not a bare teardown.

## Notes for the engine port

- The state graph is **flat** within each band: `0x14 → 0x15 → 0x16 → 0x17 → 0x18 → 0x1E` is the attack-strike chain. There are no jumps backward except from `0x5A` (which restarts at `0x0A` for the next actor).
- `ctx[+0x6D8]` is a 16-bit signed countdown. Most states that wait do `*(short*)(ctx + 0x6D8) -= DAT_1F800393` and check sign-flip. Engine port: model as `i16` ticks-per-frame counter.
- The state machine does **not** own the animation. It writes `actor[+0x1DA]` (queued anim) and waits on `actor[+0x1D9]` (current anim) to converge. The convergence is performed by the SCUS anim trio - the per-frame anim-node tick `FUN_80047430` (cursor advance + end-of-clip detect) calls the commit `FUN_8004AD80` (id → action-record install, `+0x1D9 = +0x1DA` snap, reaction/end chains), and the decoder `FUN_8004998C` cross-blends the last frame toward the queued clip's frame 0. `FUN_801D5854` never touches the anim fields (see [pose driver](#fun_801d5854---per-actor-pose-driver)); the earlier note attributing the tween to it and to `FUN_80021DF4` was wrong.
- Actions are **interruptible** only at `0x1E` (counter-attack steal). Every other transition is unconditional once the precondition fires.
- Battle-end (`DAT_8007BD71 = 0xFE`) is set from `0x5A` (post-cleanup count of survivors, with `_DAT_8007BD2C` carrying the wipe cause) or `0x66` (the successful-escape teardown - no wipe cause byte). The mode-state-machine then unloads the battle overlay.

## Decompile quirks worth knowing

- The decompile shows `_DAT_8007BD24` typed as `int*`. `_DAT_8007BD24[N]` is therefore byte N of the **pointed-to** struct (Ghidra resolves the pointer dereference as part of the indexing) - not byte N of the pointer itself. This trips up first-pass readers; see [battle.md](battle.md) § "Battle context struct" for the decode.
- `ctx[+0x6DA]` and `ctx[+0x6DB]` look like u8 fields but are read as a u16 pair (the `0x6DA` access at line 4147 of the dump uses `*(short *)(_DAT_8007bd24 + 0x6da)`). Treat as packed `(timer_lo, timer_hi)` or `i16`.
- Several states share an exit edge into `0x5A` via fall-through (e.g. `0x6B` → `0x5A`). The C decompile materialises this as explicit assignment; the MIPS source sometimes uses `j 0x801E6814` (function epilogue) directly without a state write.
- `func_0x80056798()` returns the PSX rand BIOS call (`A0 0x2E`). It's used for combat RNG (combo timing, capture chance, run angle).
- Signed-vs-unsigned comparisons appear pervasively (`(int)((uVar10 - uVar16) * 0x10000) < 0` is the idiom for "i16 went negative this frame"). The compiler emitted these as explicit casts to satisfy Ghidra; the underlying MIPS is a `bgez`/`bltz` on a sign-extended halfword.

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

The 16-arm gate the menu / battle UI runs against a candidate slot before committing the player's action. Selects which validation rule fires from the outer `param_1` arm and (for arm 6) a sub-case `param_2`. Reads HP / MP / status / item-count / stat caps from the active record (battle-actor pointer table when `_DAT_8007B83C == 0x15`, character record array otherwise) and writes a per-slot validity bit at `gp + 0x9A8`. Source: [`ghidra/scripts/funcs/8003fb10.txt`](../../ghidra/scripts/funcs/8003fb10.txt).

Arms (the target-relevance arms are re-implemented where they are consumed - liveness/kind gating in `legaia-engine-core`'s `target_picker`, item-benefit arms in `inventory_use::effect_benefits_target`; there is no standalone validator module):

| arm | meaning |
|---|---|
| `0x00` | Alive AND `hp < hp_max` (heal target). |
| `0x01` | Walk party - set bit per slot that's alive-and-not-full. |
| `0x02` | Alive AND `mp < mp_max` (restore-MP target). |
| `0x03` | Status-flag presence. Battle: `actor[+0x16E] != 0`. |
| `0x04` | Dead target (Revive item validator). |
| `0x05` | Alive (any-action target). |
| `0x06` | Stat-cap walker - sub-case picks which stat to check. |
| `0x07` | Alive (synonym of arm 5; separate code path with no upper bound). |
| `0x08` | Alive AND `(status & 3) != 0` ("can apply paralysis / sleep"). |
| `0x09` / `0x0A` | Always valid; force the bitmask to the literal `0x07`. |
| `0x0B` / `0x0C` / `0x0D` | Per-slot exact match; only valid when `slot == arm - 0x0B`. |
| `0x80` | Out-of-battle; story flag `0x100000` clear AND system flag 5 clear. |
| `0x81` | Out-of-battle; story flag `0x200000` clear AND system flag 6 clear. |
| `0x82` | Out-of-battle; calls the external item-count validator (`FUN_80046898`). |
| `0x83` | Always valid. |

The retail dispatcher writes a per-slot validity bit at `gp + 0x9A8`; the engine surfaces the same signal through the consuming paths (`target_picker` for battle-target cursors, `inventory_use` for item-menu greying) rather than a single exported byte.

## Action queue and Tactical Arts trigger ordering

Before `FUN_801E295C` reaches the inner-state machinery, the battle code resolves the player's command-input sequence into a flat **action queue** of [`ActionConstant`](../formats/art-data.md#action-constants) bytes. The queue is built incrementally from directional inputs and accumulated arts; once the player commits, the runtime applies two trigger passes in order:

1. **Miracle Art match** - if the input command sequence equals the character's Miracle Art command string, the entire queue is replaced with the Miracle Art's replacement string (`L`/`R`/`D`/`U` × 4 → `SpecialStarter` → `art1, art2, ...`). The first 4 directional bytes carry the on-disc MSB-set quirk and are masked to `0x0C..=0x0F`.
2. **Super Art find/replace at tail** - for each chained art the runtime walks all the character's Super Art `find` patterns and replaces the matched tail with a `replace` tail ending in the Super Art's finisher action constant. Triggers require: the last art of `find` is the last action in the queue, and all participating arts paid AP.

Both passes are clean-room ports in `legaia_art::MiracleMatcher` / `legaia_art::SuperMatcher`, applied together by `legaia_engine_vm::battle_action::resolve_action_queue`. The engine-vm `BattleActionHost` exposes an `art_record(char_id, art_id)` callback so the SM can fetch the [art record](../formats/art-data.md) for power-byte resolution, hit timing, and status-effect application during the `0x14..0x20` Attack chain.

### Miracle / Super in the live player-driven Arts submenu

The player-driven battle Arts submenu (`legaia_engine_core::battle_arts`) models an art as a saved **directional chain** (`legaia_save::SavedChainRecord`, raw `0x0C..=0x0F`-equivalent command bytes) rather than an in-gauge buffered input. Two trigger paths interact with that model differently:

- **Miracle Arts are wired.** A Miracle Art's trigger *is* an exact directional-string match (`MiracleMatcher::find`), so `battle_arts::miracle_for_chain` recognises a saved chain whose command string equals the caster's Miracle Art and flags the menu row (`ArtRow::miracle = Some(name)`). `World::build_battle_arts_rows` then resolves the row's per-strike profile from the Miracle's finisher-replacement queue via `resolve_action_queue`: each art constant in the replacement contributes its staged [`ArtRecord`](../formats/art-data.md) power bytes + status effect, or one tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the same graceful-degradation fallback the no-disc-data path uses). The native `play-window` HUD shows the Miracle name on the row.
- **Super Arts are wired, with the queue connectors abstracted.** A Super fires when the player chains several named arts ending on a known combination. `SuperMatcher`'s `find` patterns match the **tail** of a queue with the *interleaved* shape `Starter Art <dir> Starter Art <dir> Starter Art` (e.g. Vahn's Tri-Somersault `find` = `19 27 0F 19 1F 0E 19 27` = `Starter Somersault Up Starter Cyclone Down Starter Somersault`; see [art-data.md](../formats/art-data.md#super-arts) § Super Arts). The live submenu reaches that match in two steps:
  1. **Recognize the named-art sequence.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional `Command` string into the ordered named arts it performs, identifying each by its own `ArtRecord::commands` (greedy longest-match). `battle_arts::super_for_chain` runs this over the caster's loaded art catalog.
  2. **Tail-match the pinned art ordering.** `SuperMatcher::trigger_by_art_sequence` compares the recognized ordering against each Super's `SuperArt::art_sequence()` - the `find` pattern projected to its art constants only (`[0x27, 0x1F, 0x27]` for Tri-Somersault), with the `0x19` starters and the interleaved connector directions stripped. A tail match flags the menu row (`ArtRow::super_art = Some(name)`), and `World::build_battle_arts_rows` resolves the per-strike profile from the Super's finisher-replacement queue (`SuperArt::replace`) through the same `art_actions_strike_profile` helper the Miracle path uses. The `play-window` HUD shows the Super name on the row. Super is checked *after* Miracle, matching the retail "Miracle replacement runs before Super tail expansion" order.

  The match is deliberately **connector-abstracted**. The connector direction after each art is *combo-specific* - the same art appears with different connectors across Supers (Vahn's `0x27` is followed by `0F` in Tri-Somersault but `0E` in Power Slash), so it can't be derived from each art's own commands, so the live path matches only the pinned named-art ordering - faithful to *which* combination triggers *which* Super, without yet reproducing the byte-exact queue.

  **The queue location is now pinned by capture:** it is the per-actor action-parameter byte stream at `actor[+0x1DF..+0x1F2]` - **not** `ctx[+0x274]`, which a capture showed is the turn-order active-actor index written by `recompute_battle_order` (`FUN_801DABA4`: `lbu v0,0x11(v1); sb v0,0x274`).
  Direction/connector bytes encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up and `0x1A` = `SpecialStarter`; a Noa Miracle Art capture read that stream and it matched the engine's modeled replacement string byte-exact (probe `autorun_super_art_action_queue.lua`; runbook [`super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)). A Vahn **Tri-Somersault** capture likewise confirmed the Super path: its resident queue tail `19 27 0F 19 1F 0E 1A 2B 2B 2B` is byte-identical to `super_art.rs`'s `Tri-Somersault` `replace`, validating the combo-specific connectors (`0x27 → 0F`, `0x1F → 0E`) and the finisher tail; the dequeue site is pc `0x801D89D8`.
  The byte-exact matcher itself (`SuperMatcher::try_trigger_at_tail`) is also ported and exercised by `resolve_action_queue`'s tail pass + `battle.rs`'s `commit_turn`.

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

## Open work

- The `0x07` and a handful of intermediate values (`0x21..0x27`, `0x39..0x3B`, `0x41..0x45`, `0x49..0x4F`, `0x53..0x59`, `0x5B..0x63`, `0x6C..0x6D`, `0x72..0xFC`) have no case bodies. Confirm they are reserved padding versus reachable-via-other-overlay.
- State `0x47` (spirit-arts sustain): the `actor[+0x1F9] != 0` "spirit shield" branch is **resolved**. `+0x1F9` is set by the damage-application primitive `FUN_800402F4` case 5 (spirit-shield spirit → `+0x1F9 = 1`, gated on a non-zero target roll) and cleared by case 4 (cleanse → `+0x1F9 = 0`). Which case runs is selected by `actor[+0x1E8]`, seeded at [state `0x3C`](#state-table) from the spell table's class byte (`DAT_800754C8 + spell_id*0xC + 0`): class `== 5` routes to the shield write, class `== 4` to the cleanse. So the specific spirit that raises the shield is disc-side spell-table data, not a runtime constant. See [`spell-table.md`](../formats/spell-table.md).
- `FUN_801E7250` (`0x51`) and `FUN_801E7824` (`0x68`) are decoded from their `overlay_battle_action_*` dumps: the former is the **HP-bar drain settle check** (the `0x51` arm freezes the `ctx[+0x6D8]` countdown while any relevant actor's live HP `+0x14C` differs from its bar display value `+0x172`), the latter the **captured-monster takedown** (queued anim from the monster record, HP pair + facing zeroed, retarget to `8`, run-UI banner opened). Both ported in `crates/engine-vm/src/battle_action.rs`; see [`reference/functions.md`](../reference/functions.md).

## See also

**Reference** -
[Battle scene loader](battle.md) ·
[Damage / accuracy formulas](battle-formulas.md) ·
[Move-table VM](move-vm.md) ·
[Effect VM](effect-vm.md) ·
[Art records](../formats/art-data.md)
