# Baka Fighter minigame

A timed one-on-one fighting minigame. The player controls a battle-form party
member (Vahn / Noa / Gala) in a side-view duel against a CPU opponent, choosing
one of three attacks per exchange in a rock-paper-scissors matchup with damage
scaled by stats and combo length. The whole minigame is one RAM overlay,
`overlay_baka_fighter`, which it shares with the other minigame-hub members
(fishing / slot / dance) and with the field, world-map and move/actor VMs. The
field-VM (`FUN_801de840`), actor VM, move VM, world-map controller
(`FUN_801e76d4`) and summon/effect paths that also appear in this overlay are
documented elsewhere and are **not** Baka-Fighter-specific; see
[`script-vm.md`](script-vm.md), [`actor-vm.md`](actor-vm.md),
[`move-vm.md`](move-vm.md) and [`world-map.md`](world-map.md). This page covers
only the fight logic, which lives in the `0x801d4c50`-`0x801d6f44` band of the
overlay.

## Character meshes (PROT 1204 reuse)

Baka Fighter lets you fight *as* a battle-form party member, so it reuses the
**battle-form character pack** rather than shipping its own roster. The overlay
initializer `overlay_baka_fighter_801cf00c.txt` loads `data_field/other5.lzs`
(the LZS-compressed battle pack, PROT 1204) when the streaming-mode flag
`_DAT_8007b8c2 == 0`, otherwise it loads the equivalent uncompressed PROT entry
`0x4b5` (1205) directly. It then calls the per-fighter mesh installer
`overlay_baka_fighter_801d4c50.txt` twice (`FUN_801d4c50(0)` for the player,
`FUN_801d4c50(1)` for the opponent). That installer either streams a
`data_field` body or loads PROT entry `char_index + 0x4b6` (1206, 1207, ...) and
walks the resulting asset pack, registering each TMD chunk through the standard
asset dispatcher (`FUN_8001f05c`, type byte → handler). The decoded meshes are
the same high-detail party TMDs the turn-based battle uses; the pack layout,
provenance and its byte-match to live battle saves are described in
[`../formats/character-mesh.md`](../formats/character-mesh.md) (battle form,
PROT 1204) and [`battle.md`](battle.md). The Baka Fighter actors are sprite-like
billboards drawn from this geometry via the quad emitter described below, not
the full 3D battle renderer.

The per-fighter installer folds the party slots away (`if (idx >= 3) idx -= 3`)
and then loads **extraction entry `1204 + k`**: `1204`/`1205` are the party pack
(meshes + the 8 party atlases), and `1206..=1219` are the **fourteen ladder
fighters**, one entry each, in roster order (roster id `3 + n` -> entry
`1206 + n`). Each is a raw `[u32 (type<<24)|size][payload]` chain of
`[TIM 256x256][TMD][anim]` walked by `FUN_801D4C50` and dispatched through
`FUN_8001F05C` with the "already decompressed" flag. The count lines up exactly:
fourteen paying roster records, fourteen entries, and `1220` breaks the pattern.

The minigame's **HUD / banner art** is a separate load: **extraction PROT 1203**,
descriptor 0 (`TIM_LIST`) -> LZS -> a pack of **9 TIMs** (the "STAGE" / "HIT!" /
"KO." / "PERFECT!!" / "ALL STAGE CLEAR!" banner sheets, the digit font, the
"Fighter" logo, the arena ellipse, the impact-burst sheet and the face parts).
Every image block and CLUT row byte-matches a retail VRAM dump taken at the
cabinet.

Confidence: **Confirmed** for the load path and PROT-entry indices (traced in
`overlay_baka_fighter_801cf00c.txt` + `overlay_baka_fighter_801d4c50.txt`); the
battle-pack identity is pinned independently in `character-mesh.md`.

### Action / motion table

Each fighter is driven by a per-character **action table** reached through the
pointer array `PTR_DAT_801db8b8[char]`. Each table is a run of `0x60`-byte
action records (idle, walk, the three attacks, hit-reaction, knockdown, win/lose
poses). Per record the fields that the fight code reads are:

| Offset | Meaning |
|---|---|
| `+0x04` | per-action motion speed (scaled by the global frame-rate divisor) |
| `+0x18` | base attack power for this action (used by the damage formula) |
| `+0x1c` | sub-keyframe count for this action |
| `+0x20`/`+0x22`/`+0x24` | per-keyframe XYZ translation (TRS), `0x08`-byte stride |
| `+0x26` | keyframe's frame index (`<< 4` fixed point) |

`FUN_801d6e5c` finds the sub-keyframe whose frame index falls in a `[from,to]`
range; `FUN_801d57bc` / `FUN_801d58e0` are leftover developer "add frame" /
"delete frame" editor helpers, and `FUN_801d553c` writes a human-readable dump
of the whole action table to a debug file (`ot5stat.txt`, "ot5" = `other5`).
There are 0x11 (17) table entries of 9 actions each in the dump loop.

Confidence: **Confirmed** record-field usage; **Inferred** action-slot meanings
(idle/attacks/etc.) from how the per-fighter controller indexes them.

## Round / match state machine

The match driver is `overlay_baka_fighter_801d3468.txt`. It is gated by a phase
global `DAT_801dbf78` (0 = teardown/exit, 1 = paused, 2 = active) and only runs
the resolution body while the match-active timer `DAT_801dbf44 == 100`. Per
frame, while active, it:

1. Decays the two per-fighter cooldown timers `DAT_801dbea0` / `DAT_801dbea4`
   by the frame-rate step, clamped at 0.
2. Records who is on the left (smaller world X at actor `+0x14`): the
   facing/orientation flags `DAT_801dc08c` and `DAT_801dbfe4`.
3. Calls `FUN_801d3a14` to resolve the current exchange (see below).
4. On a decided exchange, plays the loser's knockdown (`FUN_801d4df8`), applies
   damage (`FUN_801d3b18`), rolls a critical (`FUN_801d6660`), and resets per-
   fighter exchange state. A double-KO / draw replays both. Round wins are
   counted in the per-fighter records (`&DAT_801dbff0[...]`), and the round
   index `DAT_801dbf20` advances.
5. Drives the round-end banners: a per-state machine `DAT_801dbf84` spawns
   "KO" / round-result banner sprites (`FUN_801d6e04`). (`func_0x8003d53c` is
   **not** a cue queuer - it is the XA/CD streaming player, `xa_play_warning_1` /
   `xa_play_err_2`. An earlier reading had it queueing sound cues.)

A separate timer/ready sequence lives in `overlay_baka_fighter_801d21fc.txt`
("READY/FIGHT" banner + countdown via the state global `DAT_801dc134` and timer
`DAT_801dc138`; uses `DAT_801dc110 == 0xe` to detect the final round). The
per-frame sprite-actor draw callback is `FUN_801d67f0`, installed into the
overlay's actor-draw hook `_DAT_8007ba2c` during init.

A match is **best of 3 - first to 2 round wins**. `FUN_801cf00c` init writes the
target `DAT_801dbed0 = 2`, and the match-over check in `FUN_801d0fe4` tears the
match down (`DAT_801dbf78 = 0`) once a fighter's round-win count
(`DAT_801dbff0` for the player, `DAT_801dc098` for the opponent) equals
`DAT_801dbed0`. **Confirmed.**

Confidence: **Confirmed** state globals, the call graph, and the best-of-3
target.

### Exchange resolution (`FUN_801d3a14`)

The win condition is a **rock-paper-scissors matchup** between the two fighters'
chosen attack types: P1's type `DAT_801dbfe0` vs P2's type `DAT_801dc088`.

| Type value | Meaning |
|---|---|
| `0` | no input this exchange (undecided) |
| `1` / `2` / `3` | the three attacks: **2 beats 1, 3 beats 2, 1 beats 3** (the pairwise ladder in the dump - all six ordered pairs are consistent with this relation; an earlier reading had the cycle reversed) |
| `4` | special / guard-break (an immediate win for whoever throws it; P1 checked first when both do) |

Return value: `0` = P1 wins the exchange, `1` = P2 wins, `3` = draw (both chose
the same type), `-1` = still undecided (e.g. both `0`, or the per-exchange settle
timer `DAT_801dbf54` has not elapsed - see below; in retail it is always `0`, so
this guard never actually stalls an exchange). The function also short-circuits on the
state flags `DAT_801dbfe8` / `DAT_801dc090` (one side already committed) and on
both sides idle.

Confidence: **Confirmed** (the matchup table and special-type handling are fully
visible in `overlay_baka_fighter_801d3a14.txt`).

### Damage (`FUN_801d3b18`)

Damage to the loser is computed from the winning action's base power (action
record `+0x18`), the attacker's ATK tier and the defender's DEF tier. Both are
read from the per-fighter stat pointer (`&DAT_801dc060[slot]`, which points at
the fighter's roster record - see the record table below) at three HP-keyed
thresholds: **ATK from the winner's `+0x38`/`+0x3c`/`+0x40`, DEF from the
loser's `+0x28`/`+0x2c`/`+0x30`** (the `atk %d def %d` debug printf receives
the `+0x38`-family value as `atk`, pinning the labels; an earlier reading had
them swapped). Tier `[0]` applies at HP `>= 0x8c1`, `[1]` in
`[0x3c1, 0x8c0]`, `[2]` below - fighters hit / guard differently as HP drops.
The kernel is

```text
hit  = power + power*ATK/100
dmg  = (hit * (200 - (mod + mod*DEF/100)) * 0x20) / 100  +  (combo-1)*0x40
```

where `combo` is the **loser's consecutive-hits-taken counter**
(`&DAT_801dbfec[loser]`, incremented after each application and cleared when
that fighter wins an exchange) and `mod` the loser's roster-record damage
modifier (`+0x24`). A pending critical on the winner (`&DAT_801dc05c[winner]`,
set by `FUN_801d6660`) replaces the formula with `power << 7`. HP
(`&DAT_801dbfc4[slot]`) is decremented and floored at 0; the loser is pushed
back 0x20 world units and switched to the knockdown action. A type-4 special
landing on its **final sub-keyframe** (`DAT_801dc054[winner] ==
record[+0x1c] - 1`) additionally credits the winner a round win outright.
The debug string is `atk %d def %d dm %d`.

`FUN_801d6660` is the **critical / lucky-hit roll**: `rand()%100 <`
record-`+0x34` (a per-action critical-chance byte), only while HP is in a mid
band (`< 0x280`).

Confidence: **Confirmed** the inputs and structure; the constants (`0x20`, `0x40`,
`<<7`, the HP thresholds) are read straight from the dump.

## Player input + actions

The per-fighter combat tick is `overlay_baka_fighter_801d3f44.txt`, called once
per actor with the fighter's actor pointer. The fighter's slot is `*(actor +
0x50)` (0 = player, 1 = opponent). Each frame it picks at most one of the three
attack types into a local mask:

| mask bit | button | attack type written to `DAT_801dbfe0[slot]` | action frame |
|---|---|---|---|
| `0x80` | **Square** | `1` | base + 2 |
| `0x20` | **Circle** | `2` | base + 3 |
| `0x40` | **Cross** | `3` | base + 4 |
| (none - auto) | - | `4` | base + 5 (spawns the special-effect actor) |

For the **player** (slot 0) the mask comes from the edge-triggered pad word
`_DAT_8007b874`: Square (`0x80`) → type 1, Circle (`0x20`) → type 2, Cross
(`0x40`) → type 3 (`overlay_baka_fighter_801d3f44.txt`, slot-0 branch). Type 4
(the **special**) is **not a button**: it is an auto-finisher gated on
own-HP `!= 0` / opponent-HP `== 0` / the round not already decided; when that
gate fires it sets `DAT_801dbf50 = 1`, plays action frame base + 5, spawns a
dedicated effect actor (template `DAT_801d7684`), and copies the fighter's
transform onto it. The "mirror / reaction" remap (gated by `_DAT_8007b9b0`,
the held pad `_DAT_8007b850 & 2`, and the difficulty/mode global
`DAT_801dbf94 == 2`) lives in the **slot != 0 opponent** branch, not the
player branch - it re-derives the *opponent's* type through `DAT_801dc124` so
the CPU counters the player's committed input rather than leads.

When an attack commits, the controller seeds the fighter's action id
(`actor + 0x5c`), zeroes the frame counter (`actor + 0x68`), seeds motion speed
from the action record, and records the combo step. The action-record vs
display-anim split is pinned: the display anim id `actor + 0x5c` = fighter_base
+ `{idle 1, t1 = 2, t2 = 3, t3 = 4, special = 5}` (the `base + N` frames in the
table above), while the damage kernel's record index is derived back as
`anim - (fighter_base + 1)` = `{idle 0, t1..t3 = 1..3, special = 4}` = the
attack type, and stored to the fighter record `+0x10` (disasm at
`overlay_baka_fighter_801d3f44.txt` `0x801d4534`). So record `+0x10` =
`anim - base - 1` = attack type - the record index and the attack type are the
same value. Debug strings emitted under
`DAT_801dbf94`: `%d %d`, `stat no %d %d %d`, `hit frame %d fo %d fn %d`,
`mot speed %d`. The knockdown / launch playback (after a lost exchange) is
`FUN_801d4df8`, and `FUN_801d49e8` drives the multi-frame launch arc.

Confidence: **Confirmed** the button-bit → type mapping, the named physical
buttons (Square/Circle/Cross from the slot-0 branch of
`overlay_baka_fighter_801d3f44.txt`), and the special-attack auto-finisher
gate.

## Opponent + scoring

The CPU move picker is `overlay_baka_fighter_801d487c.txt`. It rolls `rand()%6`:
on `< 3` it returns a uniformly random attack type (`rand%6 % 3`); otherwise it
advances a **per-opponent scripted pattern** - a null-terminated byte list at
`DAT_801d76e8` indexed `opponent_id * 0x6c` (opponent id from
`DAT_801dc050[slot]`), stepping a cursor in `&DAT_801dc044[slot]`. So each
opponent mixes random throws with a canned sequence. The return is always
reduced `% 3` into one of the three attack types; the picker's result index
`0`/`1`/`2` maps to the same mask bits the player uses (`0` → `0x80` → type 1,
`1` → `0x20` → type 2, `2` → `0x40` → type 3).

`DAT_801d76e8` is a field of the **per-fighter roster record table** based at
`0x801d769c` (`0x6c` stride, **17 records** = the same `0x11` count the action
table walks). **Each record opens with the fighter's name**: a 32-byte NUL-padded
ASCII string at `+0x00`, in the bytes ahead of the stat block - which is why a
parser that starts at the `+0x20` gold field never sees them. Reader
[`legaia_asset::minigame_art::baka_roster_names`]; the strings themselves decode
from the disc and are not reproduced here. The fighter-setup path installs the stat pointer as
`DAT_801dc060[slot] = 0x801d769c + id*0x6c`, so the "stat block" IS the roster
record; the historical `0x801d76bc` table view is the same records at `+0x20`.
Record layout (base `0x801d769c`): `+0x20` **gold reward** (`FUN_801d0fe4`
loads the prize on a player win as `DAT_801dbee8 = *(u32*)(opp*0x6c +
0x801d76bc)`), `+0x24` damage modifier, `+0x28/+0x2c/+0x30` DEF tiers,
`+0x34` critical chance, `+0x38/+0x3c/+0x40` ATK tiers, `+0x44` actor anchor,
`+0x4c` AI pattern (= `DAT_801d76e8`). The picker consumes the pattern
**backward**: an idle-cursor roll `>= 3` seeds the cursor to the pattern
length and each pick steps it down, returning `pattern[cursor-1] - 1` (`% 3`).
Parser [`legaia_asset::baka_opponents`] (`parse` → the 17 records with stats;
`parse_actions` → the 17 action tables; `BakaOpponent::attack_at` is the
forward convenience view). The gold values, stats + patterns decode from the
disc (`baka_opponents_real`); they are not reproduced here.

The **HUD** is rendered by `overlay_baka_fighter_801d2afc.txt`: two HP bars
(per-fighter record `&DAT_801dbfbc[slot*0xa8]`, HP at `+0x08`, drawn left at X
0x1c and right at X 0xb0), round-win pips, a combo counter (record `+0x8c`,
flashing as it grows), the round timer digits (`DAT_801dc110`) and the
running high score (`DAT_801dbee4`). The **end-of-match tally** is
`overlay_baka_fighter_801d239c.txt`: it animates four accumulating score
counters (`DAT_801dbee0`/`ed8`/`edc`/`ee8`) draining into the total
(`DAT_801dbee4`) and into the player's gold (`_DAT_80084440`), via the digit
drawer `FUN_801d6710`.

Confidence: **Confirmed** AI roll + scripted-pattern table, the HUD/tally draw
paths, and the gold payout (a flat per-opponent prize from the record table's
`+0x00`, drained into `_DAT_80084440`); the other three tally counters
(`DAT_801dbee0`/`ed8`/`edc`) feed the on-screen score total `DAT_801dbee4`, not
gold.

### The ladder - which roster id each round serves

The stage counter `DAT_801DC10C` is **seeded to 2**, not 0 (`FUN_801CF00C`),
incremented once per stage, and on reaching `0xE` sets the all-clear flag and
**wraps to 0** (`FUN_801D0748`). Every consumer folds it the same way:

```text
roster_id     = stage + 3
mesh_name_idx = stage + 5
```

So the twelve rungs the cabinet actually serves first are roster ids **5..=16** -
and across exactly those the prize gold is **strictly monotonic**. Roster ids `3`
and `4` are reachable only on the **second lap**, after the all-clear wraps the
counter; they are the post-clear opponents the victory art promises ("ALL STAGE
CLEAR! ... IT'S NOT OVER YET"). This is the whole explanation for the roster's
gold column looking out of order when read straight down: it is not sorted by
prize, it is sorted so that the two secret opponents sit in the wrap-around slots.
Stages `5` and `0xD` (the last rung) are special-cased in the SM. **Confirmed.**

Helper [`legaia_asset::minigame_art::baka_ladder`].

## Sound

Baka Fighter fires **no** runtime-bank cue (`>= 0x200`) at all - every cue it
uses is a **static** descriptor (`DAT_8006F198 + id*8`, see
[`sfx-table.md`](../formats/sfx-table.md)):

| Event | Cue | Site |
|---|---|---|
| hit / exchange resolved | `0x09` | `FUN_801D3B18` |
| confirm / cursor / cancel | `0x20` / `0x21` / `0x37` | `FUN_801CF388` |
| score-tally tick | `0x21` | `FUN_801D239C` |

The samples come from the class-2 VAB the init loads at **extraction PROT 0869**
(raw `0x367`); the shared UI cues come from the always-resident class-0 bank
(extraction PROT 0868). The minigame's **BGM** is **extraction PROT 1043**
(`music_01`), loaded by `FUN_8001FC00(0x415, ...)` + `FUN_8001E54C(5, ...)`.
**Confirmed.**

## RAM state

All addresses are overlay-resident globals (Sony bytes not committed; values
described, not pasted). The fighter cluster sits around `0x801dbf00` and
`0x801dc040`. Per-fighter arrays are strided either `0x2a` words
(`* 0x2a` in C) or `0xa8` bytes (`* 0xa8`) by slot (0 = player, 1 = opponent).

| Global | Role |
|---|---|
| `DAT_801dbf78` | match phase (0 teardown / 1 paused / 2 active) |
| `DAT_801dbf44` | match-active gate (`== 100` while a round runs) |
| `DAT_801dbf94` | difficulty / debug-verbosity mode (enables `func_0x8001a068` traces; `== 2` = mirror input mode) |
| `DAT_801dbf50` | special-attack-in-progress latch |
| `DAT_801dbf54` | per-exchange settle timer (guards `FUN_801d3a14`); **vestigial** - only ever decremented / zeroed in `FUN_801d3a14`, never positively stored anywhere in the overlay, so it stays `0` and the guard is a no-op. Exchange pacing comes from the cooldown timers `DAT_801dbea0` / `DAT_801dbea4` instead. |
| `DAT_801dbf20` | round index |
| `DAT_801dbf84` | round-end banner sub-state |
| `DAT_801dbfa0` / `DAT_801dbfa4` | player / opponent actor pointers |
| `&DAT_801dbfac[slot]` | per-fighter actor-pointer table |
| `&DAT_801dbfbc[slot*0xa8]` | per-fighter record (HP `+0x08`, combo `+0x8c`, win count) |
| `&DAT_801dbfc4[slot*0x2a]` | per-fighter HP |
| `&DAT_801dbfe0[slot*0x2a]` | chosen attack type this exchange (0/1/2/3/4) |
| `DAT_801dc088` | opponent's chosen attack type (P2 side of the matchup) |
| `&DAT_801dbfc8[slot*0x2a]` | exchange phase per fighter (0 idle / 1 windup / 2 committed) |
| `&DAT_801dbfe8[slot*0x2a]` | "already committed this exchange" flag |
| `&DAT_801dc05c[slot*0x2a]` | critical-hit-pending flag (set by `FUN_801d6660`) |
| `&DAT_801dc060[slot*0x2a]` | per-fighter stat POINTER → the fighter's roster record (`0x801d769c + id*0x6c`; ATK/DEF tiers, crit chance, damage mod live in the record) |
| `&DAT_801dc050[slot*0x2a]` | opponent id (indexes the AI pattern table) |
| `&DAT_801dc044[slot*0xa8]` | AI scripted-pattern cursor |
| `&DAT_801dc048[slot*0x2a]` | per-fighter hold-timer for the chosen type |
| `DAT_801dc124` | queued / last directional input (player) |
| `DAT_801dbea0` / `DAT_801dbea4` | per-fighter action cooldown timers |
| `DAT_801dc110` | round timer (digit value; `0xe` flags the last round) |
| `DAT_801dbee4` | running high score |
| `DAT_801dbee0` / `DAT_801dbed8` / `DAT_801dbedc` / `DAT_801dbee8` | end-of-match score counters drained into the total / gold |
| `DAT_801dc134` / `DAT_801dc138` | round-start banner state / timer |
| `DAT_801dbed0` | round-win **target = 2** (best of 3); also the drawn round-win-pip count (init `FUN_801cf00c`) |
| `PTR_DAT_801db8b8[char]` | per-character action-table base |
| `DAT_801d76bc` | the `+0x20` view into the **roster record table** at `0x801d769c` (`0x6c` stride, 17 records; gold, stats, anchor, AI pattern - see [Opponent + scoring](#opponent--scoring)). Parser `legaia_asset::baka_opponents`. |
| `DAT_801d7684` | special-attack effect-actor template |
| `_DAT_8007b8c2` | streaming-mode flag (selects LZS `other5` vs raw PROT load) |
| `_DAT_8007ba2c` | actor-draw hook (set to `FUN_801d67f0`) |

## Key functions

| Address | Role |
|---|---|
| `FUN_801cf00c` | overlay init: loads `other5`/PROT 1204 battle pack + BGM, installs both fighter meshes (`overlay_baka_fighter_801cf00c.txt`) |
| `FUN_801d4c50` | per-fighter mesh installer (data_field or PROT `idx+0x4b6`, walks the pack, registers TMDs) |
| `FUN_801d3468` | round / match resolution state machine |
| `FUN_801d3a14` | exchange win-condition (rock-paper-scissors + special) |
| `FUN_801d3b18` | damage application (ATK/DEF tiers + combo + critical) |
| `FUN_801d6660` | critical / lucky-hit roll |
| `FUN_801d3f44` | per-fighter combat tick (input/AI → attack type, action sequencing) |
| `FUN_801d487c` | opponent AI move picker (random + scripted pattern table) |
| `FUN_801d2afc` | HUD renderer (HP bars, combo, round pips, timer, high score) |
| `FUN_801d239c` | end-of-match score tally → gold payout |
| `FUN_801d21fc` | round-start READY/FIGHT banner + countdown |
| `FUN_801d4df8` / `FUN_801d49e8` | knockdown / launch-arc playback |
| `FUN_801d6e5c` | action-table keyframe lookup by frame range |
| `FUN_801d67f0` | per-frame fighter sprite-actor draw callback (`_DAT_8007ba2c`) |
| `FUN_801d5ed0` | textured-quad GPU emitter used for every fighter sprite + HUD glyph |
| `FUN_801d553c` | developer dump of the action table (`ot5stat.txt`) |

Provenance: each row corresponds to `ghidra/scripts/funcs/overlay_baka_fighter_<addr>.txt`.

## Engine port

The fight rules run clean-room as `legaia_engine_core::baka_fighter`
(`BakaFight`): the exchange resolver (`FUN_801d3a14` - settle timer, special
priority, the 2>1/3>2/1>3 relation), the damage kernel (`FUN_801d3b18` -
HP-tiered ATK/DEF, combo bonus, crit override, the special's keyframe-gated
round win), the comeback-crit roll (`FUN_801d6660`), the backward-pattern CPU
picker (`FUN_801d487c`, BIOS-rand stream) and the best-of-3 bookkeeping,
built from the parsed roster + action tables
(`legaia_asset::baka_opponents::parse` / `parse_actions`). The world hosts it
as the suspending `SceneMode::BakaFighter` (play-window `B` key;
Left/Right/Up = the three attacks, Down charges the special); a player match
win banks the opponent's parsed gold prize into the party money, mirroring
the retail tally drain into `_DAT_80084440`. Disc-gated oracle:
`engine-core/tests/baka_minigame_real.rs` (counter-play through the world
tick beats a real ladder opponent and banks the parsed prize). Host
simplifications, documented in the module: exchange recovery is immediate
(cooldowns pace re-entry) and the special's final-keyframe gate is modelled
as a held charge.

## Open

No open items.

- The physical pad-button → attack-type binding is pinned: Square (`0x80`) →
  type 1, Circle (`0x20`) → type 2, Cross (`0x40`) → type 3, from the slot-0
  branch of `overlay_baka_fighter_801d3f44.txt`; type 4 (special) is an
  auto-finisher, not a button. See [Player input + actions](#player-input--actions).
- The settle timer `DAT_801dbf54` has no seeder because it has no *use*: it is
  only ever decremented / zeroed in `FUN_801d3a14` and never positively stored
  anywhere in the ~180-function overlay, so it is vestigial (always `0`) and
  exchange pacing comes from the cooldowns `DAT_801dbea0` / `DAT_801dbea4`.
- Action-record vs display-anim indexing is resolved: record index
  = `anim - fighter_base - 1` = the attack type, written to the fighter record
  `+0x10` at `overlay_baka_fighter_801d3f44.txt` `0x801d4534`. See
  [Player input + actions](#player-input--actions).

## See also

**Reference** -
[Battle character mesh](../formats/character-mesh.md) ·
[Battle scene loader](battle.md) ·
[Tile-board grid](tile-board.md) ·
[Move VM](move-vm.md) ·
[Actor VM](actor-vm.md)
