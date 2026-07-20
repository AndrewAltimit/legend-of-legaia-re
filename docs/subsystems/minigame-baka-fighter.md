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
PROT 1204) and [`battle.md`](battle.md). The fighters render as full 3D TMDs
through the standard registered-mesh pipeline; the quad emitter described below
draws the HUD and the banner sprite *actors*, not the fighters.

The per-fighter installer folds the party slots away (`if (idx >= 3) idx -= 3`)
and then loads **extraction entry `1204 + k`**: `1204`/`1205` are the party pack
(meshes + the 8 party atlases), and `1206..=1219` are the **fourteen ladder
fighters**, one entry each, in roster order (roster id `3 + n` -> entry
`1206 + n`). Each is a raw `[u32 (type<<24)|size][payload]` chain of
`[TIM 256x256][TMD][anim]` walked by `FUN_801D4C50` and dispatched through
`FUN_8001F05C` with the "already decompressed" flag. The count lines up exactly:
fourteen paying roster records, fourteen entries, and `1220` breaks the pattern.

Chunk types within the chain: `0` = a standard PSX TIM (the fighter's 256x256
4bpp atlas; image rects `(448, 256)` / `(512, 256)` / `(576, 256)` with CLUT
strips on rows 496 / 497 / 498 - the rungs past the first two all share one
slot, loaded one at a time), `9` = the fighter's Legaia TMD (the same `TMD2`
streaming class as the PROT 1204 party slots), `11` = the fighter's
**animation bank**: a canonical ANM container (`[u32 count][u32 offsets]` +
records per [`../formats/anm.md`](../formats/anm.md), marker `0x080C`), 8
records with `bone_count` equal to the TMD's `nobj` (record 0 = idle, the
attack / special / knockdown records after it in the action-table order).
Parser [`legaia_asset::baka_opponents::parse_fighter_pack`]; disc-gated
structural oracle `crates/asset/tests/baka_presentation_real.rs` (all fourteen
packs decode, every anim rig covers its TMD, entry 1220 is refused).

The minigame's **HUD / banner art** is a separate load: **extraction PROT
1203**, a 4-descriptor container. Descriptor 0 (`TIM_LIST`) -> LZS -> a pack of
**9 TIMs** (the banner sheets, the digit fonts, the pip / combo / attack-icon
glyph page, the "Baka" / "Fighter" logo pieces, the boxing-glove halves and the
title flame ellipse); the widget pages sit at `(320, 0)` `(384, 0)` `(448, 0)`
`(320, 256)` `(384, 256)` `(832, 256)` with their CLUT strips on rows 477 / 478
/ 479 / 485 / 502..505. Every image block byte-matches the parked title-screen
VRAM capture (`minigame_baka_fighter` scenario) except the `(832, 0)` sheet
(partially overwritten live) - the live CLUT rows differ because the engine
merges several sources onto them. Descriptor 1 (type `0x02`) is a **pack of 4
Legaia TMDs** - the stage set (a single-object arena wall/room whose floor
plane is `y = 0`, two single-object props, and a 10-object piece whose objects
are object-local and need placement transforms). Descriptor 2 (type `0x05`) is
the 30-record battle-form ANM bank ([`../formats/anm.md`](../formats/anm.md)).

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
(`DAT_801dbee4`) and into the player's gold (`_DAT_80084440`).

The drain is paced by `FUN_801d6710`, which draws nothing - it returns the
per-frame step for a given remainder. The step is proportional, so a counter
empties fast and then ticks out: `> 5` moves a fifth per frame, `3..=5` a half,
and `< 3` exactly one, which is what lands the counter on zero instead of
approaching it. The fast-forward flag `DAT_801dbf00` short-circuits it to the
whole remainder, so holding the button snaps the tally to its end state. Port:
`engine-core::baka_fighter::tally_drain_step`. **Confirmed.** The port is not
wired - the engine settles a match and awards the gold in one step, so there
is no frame-paced tally for the drain rate to drive.

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

## HUD widget table + traced draw geometry

Every HUD glyph and banner goes through the overlay's textured-quad emitter
`FUN_801d5ed0(x, y, id, brightness, size)`, which indexes a **51-record widget
descriptor table at `DAT_801d7160`** (0x14 stride - the same family as the slot
machine's `DAT_801d347c`, plus per-quad gradient colour fields):

| Offset | Field |
|---|---|
| `+0x00` | base-size scale, 20.12 fixed point (`0x1000` = pixel-exact) |
| `+0x04` | texpage attribute |
| `+0x06` | CLUT id |
| `+0x08`..`+0x0B` | cell `u, v, w, h` |
| `+0x0C` / `+0x10` | top / bottom gouraud RGB (the glyphs' vertical tint) |
| `+0x0F` | semi-transparency enable (`(bit << 1) \| 0x3C` = the poly code) |
| `+0x13` | ABR rate, folded as `texpage + abr * 0x20` (`1` = additive `B + F`) |

The quad is **centred** on `(x, y)`; half-extent = `w * scale >> 13 * size
>> 12`. Record 51 onward is string rodata, which bounds the table. Two records
are patched live: widget 5 (the 24px stage digit, `DAT_801d71cc = stage * 0x18`
written by the banner-actor draw callback `FUN_801d67f0`) and widget `0x13`
(the 8px digit, `u = digit * 8`, patched by the digit drawer `FUN_801d69e4`;
`FUN_801d6a18` draws right-aligned numbers with it). Parser
[`legaia_asset::baka_opponents::parse_baka_hud`].

The HUD renderer `FUN_801d2afc` draws, per frame (retail 320x240 frame):

- **VITAL fill bars** - raw `POLY_GT4`s, y `0x26..0x2B`, one pixel per 32 HP:
  the player's is right-anchored at x `0x89` and fills leftward, the
  opponent's left-anchored at `0xB8` filling rightward. The quad's gouraud
  runs `(0xBC, hp >> 5, 0)` at the far end to `(0xBC, 0, 0)` at the anchor -
  the bar reddens toward the anchor and dims as HP drops.
- **Bar frames** - 3 texture cells per side at x `0x1C` / `0xB0`, y
  `0x20..0x30`, read from a **runtime-built** cell table at `DAT_801dbc34`
  (not recoverable from the overlay's rodata).
- **Round-win pips** - 16x16 cells at `u = 0x30` (filled) / `0x40` (empty),
  `v = 0` on the `(320, 0)` page; player at `x = 0x70 + i*16`, opponent at
  `0xC0 - i*16`, y `0x30`; `DAT_801dbed0` pips per side.
- **Combo counters** - digit cells `(digit*16, 0x20)` and the "HIT!" label
  cell `(0, 0x10)` (32x16), y `0x40`, descending from x `0x30` / `0x100`. The
  sides are **crossed**: the `(uVar8 & 1)` fold draws the *opponent's*
  hits-taken counter on the player's side (your streak) and vice versa.
- **Attack-icon columns** - the three 16x16 cells `(i*16, 0x30)` at x `0x20` /
  `0x110`, y `0x60 + i*16`; the row matching the fighter's current action
  index (record `+0x24`) brightens with the combo count.
- **Top strip** - the "STAGE" label (widget `0x12`) at `(0x30, 0x1E)` with the
  stage digits beside it, and "PRESS SELECT TO MENU" (widget `0x14`) at
  `(0xEA, 0x1E)`.

The round-start banner path `FUN_801d21fc` draws widget `0x1C` and the
round-result banners are sprite actors (spawned by `FUN_801d6e04`, widget id
in `actor + 0x50`, drawn by the `FUN_801d67f0` hook) over the "YOU" / "WIN!" /
"LOSE..." / "DRAW" / "ROUND" / "FIGHT!" / "PERFECT!!" / "GAME OVER" cells of
the widget table.

### Site presentation

The site's minigames page draws the duel from exactly these sources, decoded
from the visitor's disc in the browser (`crates/web-viewer/src/minigames_baka.rs`
+ `site/js/minigame-baka.js`): the player mesh from PROT 1204 posed by the
PROT 1203 bank (`char*9 + action`), the opponent mesh + anim bank from its own
pack, the arena from the 1203 TMD pack, and the HUD from the widget table
at the `FUN_801d2afc` positions above. Traced vs fitted is stated on the page:
the 3D camera, the fighters' spacing and the select/tally screen layouts are
fitted by eye (the duel's GTE matrices live in COP2 and the parked capture sits
at the title screen), and the bar-frame cells fall back to an outline because
their cell table is runtime-built.

The **arena** is stage TMD 0 (the pack's only world-framed piece): the tall
patterned backdrop wall with lattice fences and two ceiling lamps, base on the
`y = 0` floor plane and face authored at `z 44..225` - drawn at its authored
placement (spun 180° to the page camera's behind-the-fighters side). The
stage set carries **no floor mesh**, so the page tiles a floor from the wall's
own dominant textured face (its exact uv cell + CLUT, repeated on
`y = 0`); the tiling is a stated fit. The three prop meshes (two identical
single-object pieces + the 10-object figure) need placement transforms the
static page hasn't traced and stay out.

The run opens on the retail **PLAYER SELECT** screen - the three party
fighters' battle-form models idling in front of the arena under the sheet's
own "PLAYER SELECT" banner (widget 12) with the cursor arrows (widgets 48/49)
picking one. On a match win the winner plays a short **victory flourish**
(swings from the same attack anim slots; the slot order is the `FUN_801d3f44`
action-id fold reading), the loser holds its knockdown frame, and the retail
**tally menu** comes up: "NEXT GAME" / "PAY OUT" (widgets 44/45) beside
"GET COIN" (widget 46) and its coin-digit strip (widget 47's cell row,
`u = 88 + digit*16` - inferred from the sheet layout the way the traced digit
widgets step). Fighting on risks the accumulated prize pot on the next rung;
paying out banks it; the page treats a mid-run loss as forfeiting the whole
pot and the final rung as an automatic payout (stated readings - the menu
cells are the cabinet's, the forfeit grain is not overlay-pinned). The
pot/choice bookkeeping is `engine-core::baka_fighter::LadderRun`, reached
through the `baka_run_*` WASM surface; the per-rung prizes are the roster
records' gold column, so a full 14-rung clear pays the 460-coin total
(disc-gated oracle `crates/web-viewer/tests/baka_presentation_wasm_api.rs`,
`ladder_run_cash_out_over_real_prizes`).

The **duel facing** is the retail arrangement: the player stands on the LEFT
of the arena and faces RIGHT toward the opponent, the opponent stands on the
RIGHT and faces LEFT toward the player, so the two look at each other. Because
both mesh families (the battle-form party pack and the opponent packs) are
authored with the **same** intrinsic facing, they take **opposite** world yaws
(`facing * PI/2`) rather than one shared yaw. The layout is data, exposed as
`baka_duel_facing_json()` (`{ player: { side: -1, facing: 1 }, opponent:
{ side: 1, facing: -1 } }`, `side`/`facing` = the sign of the fighter's X
placement / heading) so the page reads it instead of hard-coding a yaw and the
facing is testable off the WASM surface.

The site plays the cabinet's **ladder**, not single fights: a fighter-select
entry (choose the player character) then successive opponent rounds served in
the disc's own order (`baka_ladder()` = roster ids `5..=16` then the two
second-lap rungs `3`, `4`). A best-of-three match win advances to the next
rung and banks the opponent's parsed prize; a loss ends the run; clearing all
fourteen rungs reaches an all-clear state. The fourteen paying prizes sum to
the full-clear total.

## Sound

Baka Fighter fires **no** runtime-bank cue (`>= 0x200`) at all - every cue it
uses is a **static** descriptor (`DAT_8006F198 + id*8`, see
[`sfx-table.md`](../formats/sfx-table.md)). It also does not go through the cue
dispatcher `FUN_8004FCC8`: it writes the cue **ring** `_DAT_8007B6D8` directly,
and the ring value is the descriptor index the drainer `FUN_80016B6C` looks up.
A sweep of every ring write in the whole overlay finds exactly **four** cue ids:

| Event | Cue | Written by |
|---|---|---|
| hit - an exchange's damage lands | `0x09` | `FUN_801D3B18` (top of the damage kernel) |
| confirm / cursor / cancel | `0x20` / `0x21` / `0x37` | the menu SM (`FUN_801CF388` family) |
| score-tally tick | `0x21` | `FUN_801D239C` |

So the duel is **quieter than it looks**: the round-start READY/FIGHT banner,
the KO, a drawn exchange's trade, the round-result banners and the victory
flourish all fire **no cue at all** - the only fight sound is the hit. Because
the ring write sits at the *top* of `FUN_801D3B18`, before the damage
arithmetic, a double-KO draw (which applies damage twice) queues the hit cue
twice.

The samples come from the class-2 VAB the init loads at **extraction PROT 0869**
(raw `0x367`, `FUN_8001FC00(0x367, 2, ...)` + `FUN_8001E54C(2, ...)`) - the same
bank the **battle scene loader** `FUN_800520F0` loads (also class 2, swapping to
raw `0x36D` when `DAT_8007BD11 == 4`), so it is the shared battle/minigame SFX
bank rather than a Baka-private one. All four cue descriptors resolve in it
(they use programs `0` and `3`). The minigame's **BGM** is **extraction PROT
1043** (`music_01`), loaded by `FUN_8001FC00(0x415, ...)` + `FUN_8001E54C(5, ...)`.
**Confirmed.**

### Site presentation

The minigames page plays exactly these cues, decoded from the visitor's disc in
the browser: SCUS → the descriptor table, PROT 869 → the VAB, each descriptor →
a one-shot through the clean-room SPU (`crates/web-viewer/src/sfx_view.rs`,
`site/js/legaia-sfx.js`). The page fires cues by *event name*, so the ids stay
next to their provenance in Rust; the two events retail leaves silent but the
page sounds anyway - a round-start sting and a match-loss sting, reusing the
confirm / cancel blips - are flagged `"source": "site"` in the event map, as
against `"disc"` for the four traced ones. Playback is gated on the site-wide
`LegaiaSound` mute toggle, and the `AudioContext` is built on the first cue so
nothing can sound before a user gesture.

The rules engine emits the hit cue itself: `BakaFight::take_cues` drains
`BAKA_CUE_HIT`, queued from inside `apply_damage` - the same place the retail
ring write sits (`engine-core::baka_fighter`).

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
| `DAT_801d7160` | HUD **widget descriptor table** (51 records, 0x14 stride; see [HUD widget table](#hud-widget-table--traced-draw-geometry)). Parser `legaia_asset::baka_opponents::parse_baka_hud` |
| `DAT_801d71cc` | widget 5's `u` field, patched to `stage * 0x18` (the 24px stage digit) |
| `DAT_801dbc34` | runtime-built VITAL bar-frame cell table (3 cells per side) |
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
| `FUN_801d5ed0` | textured-quad GPU emitter for every HUD glyph / banner sprite (indexes the widget table `DAT_801d7160`) |
| `FUN_801d69e4` / `FUN_801d6a18` | 8px digit drawer (widget `0x13`, `u = digit * 8`) / right-aligned number drawer |
| `FUN_801d6710` | end-of-match tally drain step (not a drawer - see above) |
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
