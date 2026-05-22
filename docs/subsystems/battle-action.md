# Battle action state machine

A two-level finite state machine that drives the per-actor execution of a chosen battle action - the layer between "the player picked Attack" and "the actor's body has finished swinging the sword and HP has been deducted." Lives in the battle overlay (`0898`, RAM-resident at `0x801C0000+`). The driver is `FUN_801E295C`, dumped as `ghidra/scripts/funcs/overlay_battle_action_801e295c.txt` (16 KB / 4099 MIPS instructions / 155 outgoing calls - the largest function in the battle overlay).

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
| `0x00` | Action begin | Resets ctx counters at `+0x6DA..+0x6DB`; copies `ctx[+0x274]` (queued action) → `actor[+0x1A]`; clears `ctx[+0x290]`. | `0x0A` (or `0x0B` if `ctx[+0x276] != 0` is set, i.e. action queued from menu). |
| `0x0A` | Pre-action wait | Calls `func_0x8003F2B8(1)` (likely a "pause until previous animation cleared" gate). | `0x0C` when ready, else stays. |
| `0x0B` | Action queued from menu | Holds while `ctx[+0x276] != 0` (menu still open). | `0x0A` once cleared. |
| `0x0C` | **Action seed** - reads `actor[+0x1DE]` (action category) and dispatches into the appropriate band. Calls `FUN_801EED1C` (party setup) or `FUN_801E7320` (monster setup). Reads RNG via `func_0x80056798()`. Calls `FUN_801EFE44` (camera bounds) and `FUN_801D5854(actor_id, 6)` (idle pose) unless `+0x1DE == 5` (run). The inner switch on `actor[+0x1DE]` is the "action category" dispatch - see [Inner dispatch](#inner-dispatch--actor-action-category). | `0x14`/`0x28`/`0x3C`/`0x46`/`0x50`/`0x64`/`0x68` per category. |
| `0x14` | **Attack - face target** | `FUN_801D5854(actor, 6)` (ready pose); computes target bearing via `func_0x80019B28(s8 X/Z, actor X/Z)` and writes facing into `actor[+0x46]`; iterates the 8-actor table at `0x801C9370` writing AI-side facing offsets at `ctx[+0x6E6 + i*2]`; calls `FUN_8004E2F0(actor, target)` for [range/LOS](battle.md). If range = 0 → `0x1E` (skip approach). Else looks up swing-arc table at `(&DAT_801C9348)[party_id - 3]+0x4C` (per-character attack-anim-frame table) via `func_0x80050E2C`. | `0x15` if attack records exist; `0x19` if party slot < 3; `0x1E` if out of range. |
| `0x15` | Attack - windup | Same idle pose + facing update; advances anim cursor `actor[+0x1DA]` until it matches `actor[+0x1D9]`, then re-queries swing table. | `0x16`. |
| `0x16` | Attack - advance | Same pose; rechecks range with `FUN_8004E2F0`. While out of range, advances `actor[+0x38/+0x34]` along bearing using sin/cos LUTs `_DAT_8007B7F8` / `_DAT_8007B81C` (steps both attacker and target s8 by `>> 9`); re-tests every iteration. When in range, queries the next entry from the per-character swing table. | `0x17`. |
| `0x17` | Attack - close-range | Anim/facing update; matches `actor[+0x1DA]` against `actor[+0x1D9]`. | `0x18`. |
| `0x18` | Attack - strike | Final anim match → falls into the swing apex frame. | `0x1E`. |
| `0x19` | Attack - short-step (party slot < 3 only) | Idle pose + facing + range recheck. While range > 0 → stays. Range == 0 → bumps `actor[+0x1DC] |= 1` (windup-done flag) and `actor[+0x16] = 0`. | `0x1E`. |
| `0x1E` | **Attack chain - strike loop** | Counters `actor[+0x15]` (per-strike index) + `actor[+0x16]` (combo bit). Reads the per-actor attack-script byte stream at `actor[+0x1DF + +0x15]`. The inner step writes `actor[+0x1DA] = next_anim_id` and OR's `+0x1DC |= 2`. Counter-attack handling: if `_DAT_801F6970 != 0` and the *target's* sub-state byte at `s8[+0x1DE] == 3` (was attacking), redirects active actor to the counterattacker - sets `s_Counterattack_successful_801CED18` text, fires effect `FUN_801D8DE8(0x66, 0)`, swaps `actor[+0x13]`. Per-frame physics: target/attacker drift along bearing scaled by `actor[+0x21D]` (impact-step magnitude) when ability flags `0x10/0x20` are set in the character record at `0x80084708 + (party_id-1)*0x414`. Reads `actor[+0x1DF + +0x15]` until terminator `-1` is hit. | `0x1F` once the strike-script terminator is hit. |
| `0x1F` | Attack - recovery wait | `FUN_801D5854(actor, 7 or 8)` (recover-pose; pose 8 if target's anim matched a counter trigger at `s8[+0x1F1]/+0x1F2`). Waits for `actor[+0x1DC] & 2 == 0`. | `0x20`. |
| `0x20` | Attack - return | Decides if combat continues by inspecting target liveness (`s8[+0x14C] != 0` for monster-slot, plus `s8[+0x1D9]` == `0` or `8`, plus `actor[*0x22C][+0x74] & 0xFFFFFF`), and counter-attack trigger flags (`ctx[+0x287] != 0 && DAT_8007BD0D == 0 && ctx[+0x288] != 0`). If combat ended → `0x50`. Else loops `FUN_801D5854(actor, 7 or 8)` per liveness. | `0x50` (done) or stays. |
| `0x28` | **Magic / Item - cast begin** | If `+0x1DE == 9` (item-target re-route), reseats `actor[+0x1DD]` to `ctx[+0x24B]` (item-target). If `+0x1DE == 8`, similar reroute via `ctx[+0x24A] - 1`. Then resolves bearing to target and writes facing. Sets `ctx[+0x6D8] = 0x14` (frame timer). For party (`actor_id < 3`), looks up the spell-name string via `&DAT_800754D0 + actor[+0x1DF]*0xC`, computes centered X for HUD, writes `_DAT_80077332/+0x33A/+0x344/+0x352/+0x35C` (HUD label slots), fires UI element `FUN_801D8DE8(0x4C, 0)` (spell label). If the spell's first table byte is `'c'` (capture-class spell) → `ctx[7] = 0x6E` (capture path) + queues capture archive load via `func_0x8003EC70`. Reads MP cost from `&DAT_800754D0 + spell_id*0xC + 3` (entry +3); halves it if character's ability bitmask has `0x20` ("MP-half") or quarters if `0x10`. Subtracts from `actor[+0x150]` (MP). | `0x29` (or `0x6E` for capture). |
| `0x29` | Magic - pre-cast wait | Decrements `ctx[+0x6D8]` by `DAT_1F800393` (frame dt). When it goes negative: if party_id < 3 calls `FUN_801DBF9C(party, spell_id)` (spell anim trigger). If `actor[+0x1E0] == 9` → routes to `0x32` (summon path). Pulls next anim from `actor[+0x1DF + +0x15]`; if `-1`, finishes → `0x50`. Else if spell_id < 0x81 → `FUN_801DC0A0(party, anim_id)` and special-case sound triggers (`func_0x8004FCC8(0x14C / 0x144 / 0x15E)` for spell IDs `0x3F / 0x2C / 0x6A`). | `0x29` loop or `0x32` (summon) or `0x50` (done). |
| `0x2A` | Magic - animation chain | Reads next byte from `actor[+0x1DF + +0x15]`. If not terminator: stages `actor[+0x1DA] = next_byte`, calls `FUN_801DC0A0`, sets `actor[+0x1FA] = 1`. On terminator (`-1`): if `actor[+0x15] == 2` sets `actor[+0x1FA] = 1` and OR's `actor[+0x1DC] |= 4`. | `0x2B`. |
| `0x2B` | Magic - sustained anim | Continues `FUN_801DC0A0` calls; checks `actor[+0x1FA] == 0`. | `0x2C` (and OR's `actor[+0x1DC] |= 4`). |
| `0x2C` | Magic - hit-frame loop | `FUN_801DC0A0` per frame; condition: `actor[+0x1D9] == 0` OR `(ctx[+0x24C] >= actor[+0x21B] && actor[+0x21B] != 0)` (hit-counter reaches script bound). | `0x2D`. |
| `0x2D` | Magic - recovery | If `ctx[+0x24D] == 0`: clears `actor[+0x176]` and `actor[+0x21B]`. Item-class spells (target == 9) set `DAT_8007B64C = 0x78` (UI flash). | `0x2E` once `+0x24D == 0`. |
| `0x2E` | Magic - exit | Gated on `ctx[+0x249] == 0`. Resets screen-shake (`_DAT_8007B790` if > 400, sets to 0; `_DAT_800840BC = 0x500`). | `0x50`. |
| `0x32` | Summon - invoke | `FUN_801D5854(actor, 6)` + waits on `func_0x8003DE7C(1)` (sound bank ready). When ready, computes summon-frame index `bVar5` from `actor[+0x1DF]` (if < 0x9A: `(actor[+0x1DF] + 0x7F) * 3 + 0x80`, else `actor[+0x1DF] * 4 + 99`); writes `ctx[+0x277] = bVar5`, `ctx[+0x276] = 1`, `ctx[+0x278] = 1`. Sets `actor[+0x1DA] = 9`, `actor[+0x1DC] |= 1`, `actor[+0x1FA]++`. | `0x33`. |
| `0x33` | Summon - fade in | `FUN_801DC0A0(party, 0x12)`. When `actor[+0x1F5] != 0` (anim cue): writes a 16-byte BG fade descriptor (`DAT_801C9070..0x9086`: time `0x14`, RGB `(0xFF,0xFF,0xFF)`, alpha 0→`0x14`); calls `func_0x80024E80` (push fade primitive). | `0x34`. |
| `0x34` | Summon - actor freeze | `FUN_801DC0A0(party, 0x12)`. When `actor[+0x1D9] == 0`: OR's the fade primitive bit `8`, clears `ctx[+0x278/+0x279]`, sets `ctx[+0x6D8] = 0x78` (timer), calls `func_0x801F1ED4` (?), iterates the 8-actor table to clear `actor[+0x4]` and set `+0x21C = 0xFF` (actor-hidden marker). Writes a second fade descriptor (`0x78` time, alpha `0xFF→0`). | `0x35`. |
| `0x35` | Summon - sustain | Decrements `ctx[+0x6D8]`; ramps screen brightness `_DAT_8007B910` down by `DAT_1F800393` per frame, clamped at `(_DAT_8008457C * 0x4B) / 100` (75%) for spells < 0x99 or 50% for higher. If `+0x6D8 < 0` and `ctx[+0x276] != 0`, force-clamp `+0x6D8 = 1`. | `0x36` when timer expires. |
| `0x36` | Summon - return-from-fade | Waits on `func_0x801F1ED4` returning 0. Then iterates 8-actor table clearing `+0x21C = 0` and resetting `+0x8 = 0x81000000` for actors with `+0x4 == 0`. Calls `FUN_801E70BC` (?). | `0x37`. |
| `0x37` | Summon - verify all alive | `FUN_801D5854(actor, 6)`. Iterates the 8-actor table (party + active monsters); checks each is alive (`+0x14C != 0` AND `+0x1D9 != 0`). Sets a 4-byte fade-back-in sentinel at `ctx[+0x890..+0x893]` (`84 10 42 08`). | `0x38`. |
| `0x38` | Summon - done | OR's the fade primitive bit `8`; clears `DAT_801C938C[+0x22C]`. | `0x50`. |
| `0x3C` | **Spirit / Item - pre-arm** | `FUN_801D5854(actor, 6)`. Sets `actor[+0x1DA] = actor[+0x1E7]` (queued anim). Sets `ctx[+0x243] = 1` ("action in progress" marker). For `+0x1DE == 1` (Item): looks up item record at `ctx[+0x1DF]*0xC + -0x7FF8BC97` for label/icon; writes `actor[+0x1E8/+0x1E9]` (icon page/x); writes HUD via `_DAT_80077332..+0x35C`. Special case: `actor[+0x1DF] == 0xFE` (Pomander) → label = `s_Points_returned_801CED34`. For non-Item (Magic/Spirit, `+0x1DE != 1`): does the same write of `+0x1E8/+0x1E9` from the spell table at `actor[+0x1DF]*0xC + -0x7FF8AB38`, computes MP cost (with ability-bit half/quarter), subtracts from `actor[+0x150]`; for party_id < 3 fires `FUN_801D8DE8(7, 0)` (UI element). Always fires `FUN_801D8DE8(0x4C, 0)` (HUD label). | `0x3D`. |
| `0x3D` | Spirit - wait | `FUN_801D5854(actor, 6)`. Holds while `actor[+0x1DA] != actor[+0x1D9]`. When matched, clears `actor[+0x1DA]`, calls `func_0x801F3990` (spirit init / Originals flash). | `0x3E`. |
| `0x3E` | Spirit - fire | `FUN_801D5854(actor, 6)`. Holds while `actor[+0x1D9] != 0`. Calls `func_0x800319A8(0x21)` and `FUN_801D8DE8(0x4C, 1)`. For spirit-type 4 (Originals) on party, fires `FUN_801D8DE8(0x34, 1)`. For type 5 (Spirit-arts variant), invokes the Damage UI: writes `_DAT_80076D7E` (damage value) from target HP+formula, calls `FUN_801D8DE8(0xF, 0)` (damage popup) and `FUN_801D8DE8(0x52, 0)` (damage text); RNG via `func_0x80056798`; computes damage scaling: `((target_HP * 7) / 5) + 8`, capped at 0x120 or 100. Otherwise re-fires UI elements 6/0x4E/0x4F (monster effect) or 7 (party effect) per slot. Sets `ctx[+0x6D8] = 0x20` (post-cast timer). | `0x3F`. |
| `0x3F` | Spirit - wait & fire damage | Decrements `ctx[+0x6D8]`. On expiration: calls `func_0x800402F4(actor[+0x1E8], actor[+0x1E9], target, party_id-1)` - the **damage application primitive**. Sets `ctx[+0x6D8] = 0x80` (post-damage cooldown). | `0x40`. |
| `0x40` | Spirit - post-damage | `FUN_801D5854(target, 6)`. Iterates HP-bar widget at `ctx[+0x1080]+0xE`: ramps it toward `ctx[+0x6DC]` (target HP) by `DAT_1F800393` per frame; mirrors damage-popup widget at `_DAT_801F6968+0x10`. When `ctx[+0x6D8] < 0` and target is no longer valid (dead or out of slot), sets `actor[+0x1DE] = 0` and clears HUD. | `0x50`. |
| `0x46` | **Spirit super-arts - entry variant** | `FUN_801D5854(actor, 6)`. Sets `actor[+0x1DC] = 2` (overrides flags). Stages anim `actor[+0x1DA] = actor[+0x1E7]`. Computes damage = `((target_HP * 7) / 5) + 8` (capped 0x120 / 100); HP-bar target = `actor[+0x170] + 0x20` (or `+0x28`/`+0x23` per ability-flag bits). | `0x47`. |
| `0x47` | Spirit-arts - sustain | `FUN_801D5854(actor, 6)`. When `actor[+0x1D9] != 0`, clears `actor[+0x1DA]`. Decrements `ctx[+0x6D8]`. While running: ramps damage-popup HP/widget; when expired and `actor[+0x1F9] == 0` (no spirit-shield), advances HP-bar at `ctx[+0x1074]+0xE`. | `0x48` once exit-flag (`actor[+0x1DC] == 0`) and timers settle. |
| `0x48` | Spirit-arts - flush | Final ramp of HP-bar / damage-popup. When all targets read zero AND timer expired AND anim flags clear → done. | `0x50`. |
| `0x50` | **Done - cleanup phase** | The universal "action concluded, clean up" arm. Calls `FUN_801E6968` (?), counts living party + monster actors (`+0x14C != 0 && (+0x16E & 4) == 0`); if any survivors → `FUN_801DABA4` (recompute battle ordering). Resets `actor[+0x224] = 8` (or `0x20` for spirits/`+0x1DE == 4`). Adjusts `actor[+0x170]` (HP-bar target) by ability-flag bits `0x100`/`0x200`. Clamps `actor[+0x170]` at 100. OR's `actor[+0x1DC] |= 4`. Per category: `+0x1DE == 5` (run) → screen-shake; `+0x1DE == 3` (attack) or party with dead s8 → pose 8; otherwise pose 6. Sets `ctx[+0x6D8] = 0x3C` (or `0x96` if shake, `+0x26 != 0`). If `ctx[7] == 0x50`, advances to `0x51`. | `0x51`. |
| `0x51` | Done - fade-down | Ramps `_DAT_8007B910` back up to `_DAT_8008457C` (full brightness). Per-category pose updates. Calls `FUN_801E7250` (?); decrements `ctx[+0x6D8]`. When < 0 and `ctx[+0x276] == 0`: if `ctx[+0x269] == 0` → `0x5A` (next-actor / end-of-action); else → `0x52` (continue queue). When timer < 0xC, calls `FUN_801D99BC` and unloads all UI elements: `FUN_801D8DE8(actor[+0x18], 1)` (anim), `+0x4E/+0x4F` if anim was 6, `actor[+0x26]`, `+0xF/+0x52` (damage), `+0x44`, `+0x59` (queue marker), `+0x51`/`+0x50` (banner). For multi-cast (`_DAT_801F6974 != 0`), iterates queue at `((+0x6974)-1)*4 + -0x7FE097CC` firing every queued effect's terminate marker. | `0x52` or `0x5A`. |
| `0x52` | Done - multi-cast continuation | `FUN_801D5854(actor, 8)` (action-end pose). Decrements `ctx[+0x6D8]`. If timer > 0x13 and screen-shake active (`_DAT_8007B874 != 0`), clamps timer at 0x13. When < 0: clears `ctx[+0x269]`, advances to `0x5A`. When < 0x14 and `actor[+0x17] != 0` (was running), unloads the queue marker. | `0x5A`. |
| `0x5A` | **End-of-action gate** | Iterates 8-actor table clearing per-actor anim flag bits (`+0x8 &= 0x7CFFFFFF`, `+0x21F = 0`). Resets dead/inactive actors' `+0x36 = 0`, `+0x21C = 0`, `+0x225 = 0`. Counts living actors per side: if all party or all monsters dead, sets `DAT_8007BD71 = 0xFE` (battle-end signal) + `_DAT_8007BD2C = 5` (party wipe) or `0` (monster wipe), AND's `DAT_8007BD60 &= 0x7F`. Otherwise, picks the next active actor: bumps `actor[+0x1A]++`; if `+0x1A < (party_count + monster_count - +0x25)`, advances to `0x0A` (next action); else → `0xFF` (battle complete). | `0x0A` (next actor) / `0xFF` (battle ends). |
| `0x64` (100) | **Run - flee anim begin** | Calls `FUN_801E791C` (?). Sets `ctx[+0x6D8] = 0x3C`. Fires `FUN_801D8DE8(0x43, 0)` (run UI). Iterates monster slots: if monster has rotation trigger (`+0x16C != 0`) and isn't immune (`(&DAT_8007BD10)[i] != 4`), bumps `actor[+0x1A]++`. If party-side ran (`_DAT_8007726C != ctx + 0x189`): screen-shake `_DAT_800840C0 -= 0x20` per frame, sets all party `+0x14C = 1` (revive); else screen-shake. | `0x65`. |
| `0x65` | Run - wait | If side ran → screen-shake `_DAT_8007B792` rotates. Decrements `ctx[+0x6D8]`. When < 0: success path → `0x50`; failure path (still on field) → `0x66`. | `0x50` or `0x66`. |
| `0x66` | Run - failed (battle continues) | Stages a 0x40-time fade `(0xFF,0xFF,0xFF) → (0,0,0,0xFFFF)`; calls `func_0x80024E80(&DAT_801C9070, 0)`. Sets `DAT_8007BD71 = 0xFE` (signal). | `0x67` (terminal hold; no case body - falls through to default no-op). |
| `0x68` | **Capture - start** | RNG via `func_0x80056798`. Adjusts `ctx[+0x6DA] += 0x780 + (rand%2)*0x80`. `FUN_801D5854(actor, 6)`, `FUN_801E7824(actor)` (?), `FUN_801DABA4`. Sets `ctx[+0x6D8] = 0x1E`. | `0x69`. |
| `0x69` | Capture - wait | `FUN_801D5854(actor, 6)`. Decrements `ctx[+0x6D8]`. When < 0: sets `ctx[+0x6D8] = 0x5A`, sets `actor[+0x225] = 2`, `+0x21C = 2`. | `0x6A`. |
| `0x6A` | Capture - sustain | Decrements `ctx[+0x6D8]`; if `ctx[+0x276] != 0` clamps timer at 1. When < 0: ctx[+0x6D8] = 0x3C, calls `FUN_801D99BC`, `FUN_801D8DE8(0x43, 1)` (run-UI close), `actor[+0x4] = 0`, `FUN_801D5854(0, 9)` (defeat pose). Screen rotates. | `0x6B`. |
| `0x6B` | Capture - end | `FUN_801D5854(0, 9)`; screen rotates; decrements timer. When < 0 → `0x5A` (end-of-action). | `0x5A`. |
| `0x6E` | **Magic-capture branch** | `FUN_801D5854(actor, 6)`; waits on `func_0x8003DE7C(1)` (CD ready). When ready: calls `func_0x8003EAE4(0, capture_index)` (load capture archive); sets `_DAT_8007BDB0` to capture-monster index. | `0x6F`. |
| `0x6F` | Magic-capture - fade | If `ctx[+0x287] != 0`: ramp `_DAT_8007B910 -= DAT_1F800393`, clamp to `(_DAT_8008457C * 0x4B) / 100`. Adjusts ctx-buffer X position. Waits on `func_0x8003F2B8(1)`. | `0x70`. |
| `0x70` | Magic-capture - phase 2 | Same brightness ramp as `0x6F`. Waits on `func_0x801F2160`. When done, calls `func_0x801F0348` (?). | `0x71`. |
| `0x71` | Magic-capture - finalize | `FUN_801D5854(actor, 6)`; checks all 8 slots are settled (alive with non-zero `+0x4`, or non-`8` `+0x1D9`). Once stable: clears ctx buffers, writes the 4-byte fade sentinel (`84 10 42 08`), iterates resetting per-actor `+0x21C = 0` and `+0x8 = 0x81000000`. | `0x50`. |
| `0xFD` | Idle hold (battle paused?) | `FUN_801D5854(actor, 8)`. No state change. | (stays). |
| `0xFF` | **Battle complete** | Sets `ctx[+0x6] = 0x14`, increments `ctx[+0x28A]` (battle-count?), calls `func_0x801F45A4` (battle teardown). | (terminal - exits the battle overlay). |

States `0x67` (post-fade hold), `0x07` (unused?), and several gaps in the table are not present as case labels - they fall into the `default` no-op arm (the dispatcher's `sltiu v0, v1, 0x100` bound is 256 and the JT slot for unhandled values points at the function epilogue at `0x801E6814`).

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
| `+0x1DF..+0x1F2` | u8 × N | Per-action parameter byte stream (item ID / spell ID / strike-anim list, terminated by `0xFF`). Read sequentially via `actor[+0x1DF + actor[+0x15]]`. |
| `+0x1F5` | u8 | Anim-cue flag (read at state `0x33` for fade-in trigger). |
| `+0x1F9` | u8 | "Spirit shield" flag - gates spirit-arts variant path. |
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

- State `0x28` halves/quarters MP cost based on character record bits `0x10`/`0x20` (the bit indices match the bitmask layout `FUN_80042558` populates).
- States `0x1E` (attack drift) and `0x46` (spirit-arts HP-bar) read character record bits `0x100`/`0x200` for impact-magnitude scaling.

The bitmask is cited via `*(uint *)(((byte)(&DAT_8007BD10)[ctx[+0x13]] - 1) * 0x414 + -0x7FF7B804)` - i.e. the active character's record at `0x80084708 + (party_id - 1) * 0x414 + 0xF4`, which is exactly the per-character `+0xF4..0x100` block that `FUN_80042558` OR-aggregates into the global bitmask.

### `FUN_801DFDF8` - effect-bundle public spawn API

`FUN_801E295C` does **not** call `FUN_801DFDF8` directly. Effect spawning happens through one of two indirections:

- **`FUN_801D8DE8(effect_id, mode)`** - the hottest battle utility (3 KB / 77 incoming refs), called 30+ times across the state machine. This is the wrapper that lays out a battle UI element (damage popup, weapon-slash trail, spell-icon banner, run-status banner, etc.) and internally schedules its visuals. Effect IDs surfaced in this function: `0x07` (party weapon-slash), `0x0F` (damage popup setup), `0x34` (Originals burst), `0x43` (run banner), `0x44` (terminate banner), `0x4C` (spell-name HUD), `0x4E`/`0x4F` (monster effect pair), `0x51` (combo continue), `0x52` (damage text), `0x59` (queue marker), `0x66` (counter-attack flash). The `mode` argument is `0` for "spawn / reset" and `1` for "terminate / unload."
- **`FUN_801DBF9C(party, spell_id)`** + **`FUN_801DC0A0(actor, anim_id)`** - chained from state `0x29` and `0x2A..0x2D` to drive spell visuals. These ultimately fan out to the [effect VM](effect-vm.md) which uses `FUN_801DFDF8` for the actual sprite-anim spawn.

So the dataflow is `FUN_801E295C` → `FUN_801D8DE8` / `FUN_801DBF9C` / `FUN_801DC0A0` → effect VM (`FUN_801DE914` / `FUN_801E0088`) → `FUN_801DFDF8`. The state machine never names an effect ID directly; it names *UI element* IDs which the effect VM resolves. Note this path drives the **2D UI/sprite** layer (`FUN_801DFDF8` emits `POLY_FT4` billboard quads into the effect pool); the 3D summon model is a separate mechanism (next).

### Seru-magic summon-overlay dispatch

The 3D visual of a player Seru-magic cast (the summoned Seru and its attack mesh - e.g. Gimard's *Tail Fire* flame) is **not** spawned by an opcode and does **not** live in `befect_data`. It is a **per-summon code overlay** paged in on demand. In outer state **case `0x29`**, when the queued action's spell id `actor[+0x1df]` is in the player Seru-magic block `0x81..0x8b`:

```c
_DAT_8007bd24[7] = 0x32;                                   // advance to the cast band
_DAT_8007ba2c = (&PTR_s_re_check_801f6734)[id - 0x81];     // per-summon effect-data pointer
FUN_8003ec70(id - 0x79, 0);                                // overlay loader B: PROT (id - 0x79 + 0x381)
```

`FUN_8003EC70(param)` (overlay loader B) loads PROT index `param + 0x381` into `*DAT_80010390` (= `0x801F69D8`, above the resident battle overlay), so the summons map to **PROT 905..915** (Gimard *Tail Fire* `0x81` → param `8` → **PROT 905**; byte-verified MIPS-code overlays). The capture-class (`'c'`) spell branch loads from a different base: `FUN_8003EC70(spell_record[+1] + 0x28)`.

#### Inside a summon overlay (PROT 905, decoded)

The summon overlay carries **no embedded TMD geometry** (no `0x80000002` magic). The summon's meshes are the separately-loaded `DAT_8007C018` model library: **PROT entry 871** (`etmd.dat`), a 30-entry `asset::pack` of Legaia TMDs that the battle scene loader `FUN_800520F0` pulls at battle init (debug index `0x367`, retail dev path `h:\prot\battle\etmd.dat`) and registers via `FUN_80026B4C`, populating `DAT_8007C018[3..32]` (`[0..2]` are the party battle meshes). Despite its CDNAME label `sound_data`, PROT 871 is the effect-model library; its texture sibling PROT 870 (a 256×256 flame-frame atlas, also `sound_data`) is loaded by a separate path. What the overlay supplies is a **move-VM scene-graph** that poses and animates those meshes:

- The overlay init calls **`FUN_80021B04(ctx, position, record_ptr, 0x1000)` ~22 times** - one per body part of the summon - walking a record table (file offset `0x180C..~0x1E5C`, ~17 unique records of stride ~`0x58`, some reused for symmetric/repeated parts).
- Each record is `[i16 model_sel @+0][u16 flags @+2][move-VM u16 bytecode @+0x4 …]`. `FUN_80021B04` stages it as an actor: `actor[+0x48] = record` (move-buffer base), `actor[+0x70] = 2` (move-VM PC, u16 units → bytecode begins at `record+0x4`), `actor[+0x58] = 0x7f`, then `jal FUN_80023070` to run the part's [move-VM](move-vm.md) animation. So the summon is a hierarchy of move-VM-driven animated parts; the per-frame animation is the standard actor-tick + move-VM path, which is why the overlay code itself need not stay resident.
- `record[+0]` is the mesh selector: `≥0` → `DAT_8007C018[record[+0] + gp[0x754]]` (a per-summon base offset into the global table), `-1` = model-less transform/pivot node, `0x4000`/`0x4001` = special render-mode nodes. In PROT 905 **all 22 records are `-1`** - the parts are transform nodes whose mesh is bound from the move bytecode's animation-bank opcodes (move-VM `0x00` → `actor[+0x3C..40]`, `0x04` → `actor[+0x80..84]`), not from `record[+0]`.

The flame renders as Gouraud-textured (`POLY_GT3`/`POLY_GT4`) prims sampling the resident `etim` page (832,256) 4bpp; `cba`/`tsb` are applied at render. In a live Tail-Fire capture the summon library occupies `DAT_8007C018[3..32]`; ten of those (`[23..32]`) are fire-textured meshes (cba row 478 `0x778B` baked), and the **active Gimard flame is `DAT_8007C018[26]`** - the only rendered model baking etim, with both rendering actors carrying `actor[+0x64]=26` and `actor[+0x56]=5` (full-TMD mode → `FUN_8002735C`). The flame mesh is **static**; its fire flicker is **CLUT/palette animation**, not model cycling - PROT 905 uploads animated CLUT frames each frame via `LoadImage` (`FUN_800583C8`, source palette frame `base + phase*480`, a 240×1 strip), which is why the rendered prims' cba column cycles (0/16/32) within row 478. The absolute index is load-relative. **Residual:** the load path + VRAM target of the PROT 870 flame-texture atlas. See [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

### `FUN_801D5854` - per-actor pose driver

The single most-cited helper inside `FUN_801E295C` (~30 call sites). Signature `FUN_801D5854(actor_id, pose_id)`. Pose IDs surfaced:
- `6` = idle / breathing
- `7` = ready / pre-action
- `8` = action-end / hit-recovery
- `9` = defeat / down

Note that `FUN_801D5854` for `param_2 == 9 && param_1 == 7` (the only path that calls the special-case) writes pose 9 unconditionally and triggers the run-side animation lookup `FUN_801DB9C4`.

### `FUN_801EED1C` / `FUN_801E7320` - party / monster setup hooks

Called from state `0x0C`:
- Party (`actor_id < 3`): `FUN_801EED1C()` - initialises per-character action data.
- Monster with AI flag (`+0x16E & 0x380 != 0`): `FUN_801E7320()` - initialises monster-AI action.
- Otherwise: neither - actor inherits from previous frame.

### `FUN_801EFE44` - battle camera bounds

Called from state `0x0C` for non-flee actions. Walks the 8-slot actor table computing min/max X and Z to set the battle camera's frustum. Read-only with respect to the action state machine; pure rendering helper.

## Notes for the engine port

- The state graph is **flat** within each band: `0x14 → 0x15 → 0x16 → 0x17 → 0x18 → 0x1E` is the attack-strike chain. There are no jumps backward except from `0x5A` (which restarts at `0x0A` for the next actor).
- `ctx[+0x6D8]` is a 16-bit signed countdown. Most states that wait do `*(short*)(ctx + 0x6D8) -= DAT_1F800393` and check sign-flip. Engine port: model as `i16` ticks-per-frame counter.
- The state machine does **not** own the animation. It writes `actor[+0x1DA]` (queued anim) and waits on `actor[+0x1D9]` (current anim) to converge. The animation tween is run by `FUN_801D5854` and the per-frame actor tick (`FUN_80021DF4`).
- Actions are **interruptible** only at `0x1E` (counter-attack steal). Every other transition is unconditional once the precondition fires.
- Battle-end (`DAT_8007BD71 = 0xFE`) is set from `0x5A` (post-cleanup count of survivors) or `0x66` (run-failed fade). The mode-state-machine then unloads the battle overlay.

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

## Action validator (`FUN_8003FB10`)

The 16-arm gate the menu / battle UI runs against a candidate slot before committing the player's action. Selects which validation rule fires from the outer `param_1` arm and (for arm 6) a sub-case `param_2`. Reads HP / MP / status / item-count / stat caps from the active record (battle-actor pointer table when `_DAT_8007B83C == 0x15`, character record array otherwise) and writes a per-slot validity bit at `gp + 0x9A8`. Source: [`ghidra/scripts/funcs/8003fb10.txt`](../../ghidra/scripts/funcs/8003fb10.txt).

Arms (clean-room port at [`crates/engine-vm/src/action_validator.rs`](../../crates/engine-vm/src/action_validator.rs)):

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

The retail dispatcher's `gp + 0x9A8` byte is exposed via [`ActionValidatorHost::target_valid_bits`](../../crates/engine-vm/src/action_validator.rs); engines wire it to whatever cursor / slot-grey state the menu reads.

## Action queue and Tactical Arts trigger ordering

Before `FUN_801E295C` reaches the inner-state machinery, the battle code resolves the player's command-input sequence into a flat **action queue** of [`ActionConstant`](../formats/art-data.md#action-constants) bytes. The queue is built incrementally from directional inputs and accumulated arts; once the player commits, the runtime applies two trigger passes in order:

1. **Miracle Art match** - if the input command sequence equals the character's Miracle Art command string, the entire queue is replaced with the Miracle Art's replacement string (`L`/`R`/`D`/`U` × 4 → `SpecialStarter` → `art1, art2, ...`). The first 4 directional bytes carry the on-disc MSB-set quirk and are masked to `0x0C..=0x0F`.
2. **Super Art find/replace at tail** - for each chained art the runtime walks all the character's Super Art `find` patterns and replaces the matched tail with a `replace` tail ending in the Super Art's finisher action constant. Triggers require: the last art of `find` is the last action in the queue, and all participating arts paid AP.

Both passes are clean-room ports in `legaia_art::MiracleMatcher` / `legaia_art::SuperMatcher`. The engine-vm `BattleActionHost` exposes an `art_record(char_id, art_id)` callback so the SM can fetch the [art record](../formats/art-data.md) for power-byte resolution, hit timing, and status-effect application during the `0x14..0x20` Attack chain.

When the active actor's `chosen_art` is set and `art_record` returns a record, `attack_chain` (state `0x1A`) calls a second host hook `apply_art_strike(ArtStrikeInfo)` alongside the existing `apply_damage`. `ArtStrikeInfo` carries the strike-indexed power byte, dmg_timing, hit cue, and the art's flat status effect. Engines drive HP deduction, status application, sound-effect scheduling, and visual hit-cue dispatch off this struct; tests feed synthetic `ArtRecord` instances and assert the per-strike `(power, timing, effect, cue)` resolution rather than going through `apply_damage`'s legacy `(icon, page, target, slot)` parameter pack.

The engine-side translator at `crates/engine-core/src/art_strike.rs` (`apply_art_strike(attack, defense, info) -> ArtStrikeOutcome`) folds an `ArtStrikeInfo` into a concrete HP delta + status flag + scheduled SFX cues using the `art_strike_damage` formula in `legaia_engine_vm::battle_formulas`. The world's `BattleActionHost::apply_art_strike` impl resolves the per-slot weapon attack from `World::battle_attack` and the right defense (UDF or LDF, picked from `World::battle_defense_split`) before calling the translator, then emits a `BattleEvent::ApplyArtStrike` with the resolved `ArtStrikeOutcome`. Engines apply each strike's `damage` / `enemy_effect` / `cues` through whatever runtime they have for HP / status / SFX dispatch.

## Open work

- The `0x07` and a handful of intermediate values (`0x21..0x27`, `0x39..0x3B`, `0x41..0x45`, `0x49..0x4F`, `0x53..0x59`, `0x5B..0x63`, `0x6C..0x6D`, `0x72..0xFC`) have no case bodies. Confirm they are reserved padding versus reachable-via-other-overlay.
- States `0x32..0x38` (summon flow): the `func_0x801F1ED4` call inside `0x34`/`0x35`/`0x36` is opaque; its dump is needed to resolve the summon-creature spawn path.
- State `0x47` (spirit-arts sustain): the `actor[+0x1F9] != 0` "spirit shield" branch and its interaction with the HP-bar at `ctx[+0x1074]` needs cross-referencing with the spirit definitions table to identify exactly which spirit triggers it.
- `FUN_801E791C` (`0x64`), `FUN_801E7824` (`0x68`), `FUN_801E7250` (`0x51`), `FUN_801F0348` (`0x0C`/`0x71`), `FUN_801F3990` (`0x3D`), `FUN_801F45A4` (`0xFF`) are all opaque battle helpers - their semantics here are inferred from caller context, not their own decompile.
