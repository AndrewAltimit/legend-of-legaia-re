# Casino slot machine

The casino's slot-machine minigame: three reels of pictographic symbols, a flat 3-coin spin played across **five** paylines (three straight, two diagonal), a per-spin win evaluation against a payout table, and a running coin balance the player can cash back out to the casino coin bank. It lives in the minigame-hub overlay (the binary shared with fishing / Baka Fighter / dance), so the locomotion, sprite, actor-VM and SDK helpers it leans on are documented elsewhere - this page covers only the slot-specific logic.

**The machine is a 3D scene.** Not a sprite collage: the reels are textured cylinders, the paylines are 3D line segments, and the medallions / lamps / pedestals / marquee are billboards - all projected through the GTE and depth-sorted into the ordering table. See [Rendering - a 3D scene](#rendering---a-3d-scene) below.

**The bonus round is the same machine.** Matching a line of either jackpot symbol
opens it, and the reels then *rotate* onto the init's second strip - the numerals
`1..=10`, their own artwork on the art pack's second page - one row per frame. The
player stops all three with no help from the machine, the round pays the **product
of the three numbers**, and the tally across the top (`0 x 0 x 0`, filling in as
each column is claimed) is the machine's own dot-matrix marquee reading out the
same latch the payout multiplies. See
[The bonus game](#the-bonus-game---the-two-jackpot-symbols).

**This is the slot *gameplay*, not the prize exchange.** Cashing casino coins for items is a separate static table (`DAT_801e4518` / PROT 899, debiting `_DAT_800845A4`'s sibling coin counter) covered by the randomizer's `casino::CasinoExchange`. The slot machine pays out *into* the coin balance; the exchange spends it.

Provenance: the dumps are `ghidra/scripts/funcs/overlay_slot_machine_<addr>.txt`. Confidence is marked per-claim; the reel layout, RNG, bet charge, feature odds, payout lookup, entry seed and coin commit are **Confirmed** from the disassembly. Table *values* (payout bytes, HUD descriptors) decode from the user's disc and are not reproduced here.

## Reel state machine

The machine is a single per-frame handler, `FUN_801cf0d8` (`overlay_slot_machine_801cf0d8.txt`), dispatched on the state word `DAT_801d3c84` through a jump table (the overlay-resident table just below `0x801d2ac0`; the `< 0x65` bound on the index is in the dispatcher prologue). **Confirmed** states:

| `DAT_801d3c84` | Role |
|---|---|
| `0` | **init**: reseed (`func_0x80056798`), build the three reel strips, clone them into the display copy, fade in |
| `1` | **attract / idle**: wait for input; pressing a face button (`_DAT_8007b874 & 0xe0`) charges the flat bet (3 coins, or 1 in feature modes 4..6) and advances to spin; the menu/select edge (`& 0x110`) routes to state `0x32` (cash-out) |
| `2` | **spin-up**: ramp the three reel velocities (`DAT_801d3cd0..` added into the reel positions `DAT_801d3cc0..` each frame, wrapping mod `0x1400`) until the spin timer `DAT_801d3c90` expires |
| `3` | **stopping**: each Stop input (pad bits `0x80`/`0x40`/`0x20` → reels 0/1/2) calls `FUN_801d2114` to choose where that reel lands; once all three reels are stopped (`DAT_801d3d2c == 3`) it runs `FUN_801d13e8` (win eval) and advances to `4` |
| `4` | **payout tally**: animates the credited win (`DAT_801d3d38`) ticking from the win counter into the player balance `DAT_801d4114`; on completion returns to `1` |
| `0x32`..`0x39` | **cash-out / quit submenu**: a 3-option picker (`DAT_801d4110 % 3`) with fade-in/out states; option `1` commits the balance (route to state `100`), the others return to play or leave |
| `0x5a` | **not-enough-coins** prompt (reached from state `1` when `DAT_801d4114 < 3`) |
| `100` | **commit + exit**: fades out, writes `_DAT_800845A4 = DAT_801d4114`, returns to the casino field |

The tail of `FUN_801cf0d8` (after the switch) always advances the three reel positions, redraws the visible symbols via `FUN_801d0fa8`, and refreshes the marquee/HUD. There is **no bet-line selection anywhere in the machine** - every spin plays all five paylines for the flat cost (the earlier "bet-line selector" reading of `DAT_801d4110` conflated it with the cash-out submenu cursor, which is that word's only role).

### Entry init - `FUN_801cec94`

The overlay's init entry (mode-24 warp target `0x801CEC94`) seeds the session before state `0` runs. **Confirmed** from the disassembly (the function sits below the first dumped handler): the slot LCG `DAT_801d3c80` is written the literal seed `0x6C0A2AF0`; the playing balance is **assigned from the casino coin bank** (`DAT_801d4114 = _DAT_800845A4` - the symmetric counterpart of the state-`100` commit); when the battle-return flag `_DAT_8007B8B8` is zero (the overlay launched outside the casino door path) the balance instead defaults to `0x46` = 70 coins - a dev-launch fallback, printed by the adjacent `"battle_return_flag %d"` / `"game_coin %d"` debug strings; and the state word is cleared to `0`.

### Reel strips - two of them, and a display strip between

Each of the 3 reels is a 20-slot (`0x14`) strip, and the init builds **two** of
them per reel:

| Array | Contents | Probe step |
|---|---|---|
| `DAT_801d3e90` | the ten reel **symbols**, ids `0..=9` (`slot / 2`) | `+0xd` |
| `DAT_801d3fd0` | the ten bonus **numerals**, as values `0x10..=0x19` (`slot / 2 + 0x10`) | `+1` |
| `DAT_801d3d50` | the **display strip** - the only one the win eval and the renderer read | (copied) |

Each is `3 × 0x14` ints at `0x50` stride. Init (`FUN_801cf0d8` case 0) fills both
in one interleaved pass: for each of the 20 slots, draw an RNG value, reduce it
mod `0x14`, probe forward by the array's step until an unused position turns up,
and place the slot's value there - a collision-resolving permutation that
scatters each value (two strip positions each) around the reel. Then the symbol
strip is cloned into the display strip. **Confirmed** from
`overlay_slot_machine_801cf0d8.txt` (the `% 0x14` / `(uVar1 + 0xd) % 0x14`
placement loops and the `+ 0x10` on the second array).

The live reel position `DAT_801d3cc0[reel]` is a fixed-point angle; the on-screen
symbol index is `(pos >> 8)` reduced mod `0x14`, and adjacent rows are read at
`±1`/`±0x10`/`±0x11` offsets (the three pay rows).

#### The display strip is refilled one row per frame - and that IS the bonus swap

The reel SM never rewrites a strip wholesale. Its render tail copies exactly
**one row per reel per frame** into the display strip, from whichever source
strip the feature mode names (**Confirmed**, the tail of `FUN_801cf0d8`):

```c
row = ((pos >> 8) + 0x19) % 0x14;                       // 9 rows AHEAD of the payline
display[reel][row] = (feature_mode == 6 ? bonus : symbols)[reel][row];
```

So a bonus round does not relabel the symbols and does not swap a strip in a
frame: the numerals **rotate into** the reels from off-screen as they turn, a row
at a time, and rotate back out again when the round ends. Three consequences fall
out of the arithmetic and all three are visible on the machine:

- the refilled row is `0x19 - 0x10 = 9` rows ahead of the payline row
  (`(pos >> 8) + 0x10`), so a row is converted well before it can be paid on;
- a reel therefore has to **travel** ~9 rows for the conversion to reach the
  payline - which is exactly why state 1 forces `DAT_801d3c90 = 0x18` (24 extra
  spin frames) when `DAT_801d3cac == 6` **or** when the "bonus just ended" flag
  `DAT_801d3798` is set. The long spin-up on both edges of the bonus round is
  load-bearing, not decoration;
- during the rotation the strip is legitimately **mixed** - some rows symbols,
  some numerals - and the renderer copes because it switches per *row*, not per
  mode (below).

### Symbol art - computed, not tabled

The reel quads are drawn by `FUN_801d0fa8` with **arithmetic UVs from the strip
value itself** - no descriptor table is involved (**Confirmed**,
`overlay_slot_machine_801d0fa8.txt`). The value's `>= 0x10` test is the whole
symbol/numeral switch, and it is applied **per row**, not per feature mode:

```c
tpage = 0x0C + (v >= 0x10);                       // 0x0D = the (832, 0) page
clut  = (v >= 0x10 ? 0x7AC0 : 0x7A80) + (v & 0xF);
U, V  = (v & 3) * 0x40, (v & 0xC) * 0x10;         // a 4x4 grid of 64x64 cells
```

So the ten symbols live on art page 0 (CLUT row 490, column = symbol) and the ten
bonus numerals `1..=10` on art page 1 (CLUT row 491, column = `value & 0xF`), each
with **its own palette column** - which is why every numeral on the retail bonus
reels is a different colour, and why one strip can carry both mid-rotation.
Decoders: [`legaia_asset::minigame_art::slot_symbol`] /
[`slot_bonus_number`](../../crates/asset/src/minigame_art.rs).

The gouraud shade fades each row by distance from the payline (`0xB4` cap),
curling the reel.

`FUN_801d2cc0` (the earlier "symbol rasteriser" attribution) is actually the **HUD widget rasteriser**: it draws a `POLY_GT4` from the 20-byte-stride descriptor table at `DAT_801d347c` indexed by the *low 10 bits* of its id argument, with the id's high bits overriding the semi-transparency mode. The table has exactly **3 records** (PROT 0975 file offset `0x4C64`; layout **Confirmed** from the field-by-field prim writes):

| Offset | Field |
|---|---|
| `+0x00` | `i32` base-size scale, 20.12 fixed point (multiplies the w/h bytes, then the caller's per-axis scale args) |
| `+0x04` | `u16` texpage attribute (written to the prim tpage slot `+0x1A`, ORed with `semi_mode << 5`) |
| `+0x06` | `u16` CLUT id (prim `+0x0E`; overridden with `0x7D0F` when the id's high field is `2`) |
| `+0x08` | `u8 u, v` texture origin |
| `+0x0A` | `u8 w, h` cell size |
| `+0x0C` | `u8 r, g, b` primary shade (scaled by the caller's brightness arg) |
| `+0x0F` | `u8` semi-transparency enable (into the GP0 command byte) |
| `+0x10` | `u8 r2, g2, b2` far-edge shade |
| `+0x13` | `u8` semi-transparency mode (0..3, into tpage bits 5-6) |

Record 0 is the 127x239 **paytable board** on the right of the machine (8bpp - see [the screen-space draws](#the-two-screen-space-draws---fun_801d2cc0)), record 1 the 64x16 **"COIN" label** (the cell immediately left of digit `0` on the `0x0C` page - not a marquee), record 2 the 16x16 cash-out-cursor arrow (`FUN_801cf0d8` state `0x32` positions it at `DAT_801d4110 * 0x10 + 0x6C`). Immediately after the third record (`0x801D34B8`) the region becomes a **14-entry pointer table** - the attract-mode instruction-text lines, pointing at the string block at the head of the overlay ("To spin the wheels, insert 3 coins by pressing the ... buttons", the earlier "transitions into a pointer array" observation).

## RNG

Two independent generators feed the machine:

- **`FUN_801d30cc`** (`overlay_slot_machine_801d30cc.txt`) - the slot's own deterministic LCG over `DAT_801d3c80`: `x = x*5 + 1`, then the 16-bit halves are folded (`x = (x << 16) + (x >> 16)`). **Confirmed.** Used for reel-strip construction, reel-landing selection, and the "feature stays on / turns off" rolls. Because it is a self-contained word, the reel outcomes are reproducible from the seed state.
- **`func_0x80056798`** - the BIOS `rand` (A0(0x2F) sibling, the same source the tile-board filler uses). **Confirmed** call site. Used for the per-spin *feature*/bonus rolls in `FUN_801d258c` and `FUN_801d2440`'s landing jitter, i.e. the parts that should not be replayable from the visible reel state alone.

### Reel landing - `FUN_801d2114` / `FUN_801d2440`

When a reel is told to stop, `FUN_801d2114(reel)` (`overlay_slot_machine_801d2114.txt`) picks the target *symbol* and a search count keyed on the current **feature mode** `DAT_801d3cac` (Confirmed switch 0..6):

| `DAT_801d3cac` | Behaviour (search depth + biased target symbol) |
|---|---|
| `0` | normal: scan `rand%3 + 2` rows, target `DAT_801d3cb8` |
| `1` / `2` | "reach"/tease modes: scan `(rand&3)+6` rows, target symbol `9` / `8` (the two jackpot symbols) |
| `3` | hot mode: random target `rand%7 + 0x18`, landing offset `rand%7 + 10` |
| `4` | guaranteed-hit mode: drives the reel to a winning symbol, decrementing a guarantee counter `DAT_801d3c90` |
| `5` | hold variant: depth `0x14`, target `rand%10 + 8` |
| `6` | **the bonus round: depth `0`, target `-1`** - see below |

`FUN_801d2440(reel, depth, target_symbol)` (`overlay_slot_machine_801d2440.txt`) is the actual landing search: starting from the current reel position it walks up to `depth` rows looking for `target_symbol` on the display strip; if found it returns a stop offset that lands the symbol on the payline, otherwise it returns the next natural row (no forced result). The `+ DAT_801d4134 * 0x10` term (a per-spin 0..4 jitter chosen in `FUN_801d258c`) nudges the exact landing so the stop does not look mechanical. **Confirmed.**

**The bonus round steers nothing.** Mode 6 passes `depth = 0` with `target = -1`,
and the search is guarded by `0 < depth`, so it runs zero iterations and returns
the next natural row. The reel stops where the player stopped it: the three
numbers a bonus round multiplies are the player's timing and nothing else.
(An earlier reading had mode 6 reusing the guaranteed-hit plan so the free spins
would land a matched line - that is **falsified**; the retail case is the least
steered of the seven, not the most.)

### Feature roll - `FUN_801d258c`

`FUN_801d258c` (`overlay_slot_machine_801d258c.txt`) runs once at spin start. It seeds `DAT_801d4134` (`rand%5` landing jitter) and `DAT_801d3cb8` (`rand%6 + 2` normal-mode target), rolls the optional widen amount **once** (`rand%100 + 200` when the richer-odds flag `DAT_801d3790` is set - added to every denominator below), then - only when no feature is already active (`DAT_801d3cac == 0`) - rolls `rand % (widen + N) == 0` probabilities to *enter* a feature mode. The denominators are **bracketed on the net-take counter** `DAT_801d3d40` (NOT the balance):

| `DAT_801d3d40` | mode-1 / mode-2 denominators |
|---|---|
| `< 1000` | `700` / `500` |
| `1001..=1999` | `0x15E` (350) / `0xFA` (250) |
| `> 2000` | `0xAF` (175) / `0x7D` (125) |

plus a final flat `widen + 600` roll for mode 3. Exactly `1000` or `2000` falls in **no** bracket - only the mode-3 roll runs. **Confirmed** arithmetic - and the tuning direction is the *opposite* of the earlier "make the player feel lucky when poor" reading: a **high** net take gets the small denominators, so features become roughly 4x more likely once the machine has taken 2000+ net. The machine pays back what it has taken (the counter accrues `+6`/`+1` per spin and each bonus payout is *subtracted* - see the coin economy below).

## Payout / win evaluation

After all three reels stop, `FUN_801d13e8` (`overlay_slot_machine_801d13e8.txt`) evaluates the win. **Confirmed** structure:

1. For each of the **five** paylines it reads the three on-screen symbols (display strip `DAT_801d3d50` at the per-reel row offsets below) and checks all-three-equal. It keeps the **highest-value matching line** (`DAT_801d3d34` = winning symbol id, `DAT_801d3c8c` = winning line index, which is also the medallion / lamp the machine lights).

   The absolute row reads are `+0x11 / +0x10 / +0x0F` from `(pos >> 8)`; the centre row is `+0x10`, so relative to it:

   | line | reel 0 | reel 1 | reel 2 | on screen |
   |---|---|---|---|---|
   | 0 | `+1` | `+1` | `+1` | top row |
   | 1 | `0` | `0` | `0` | middle row (the payline proper) |
   | 2 | `-1` | `-1` | `-1` | bottom row |
   | 3 | `-1` | `0` | `+1` | diagonal, bottom-left to top-right |
   | 4 | `+1` | `0` | `-1` | diagonal, top-left to bottom-right |

   The line index doubles as the medallion / lamp index, and lines 3 / 4 terminate on the `y = ±336` medallions - exactly where the two diagonal segments in the payline geometry table end.
2. If a line wins, the credited amount is `DAT_801d3d38 = payout_table[winning_symbol]`, read as a byte from a table at `DAT_801d3598` indexed by symbol id (so payout scales with symbol rarity). **Inferred** values (not reproduced).
3. Symbol ids `8` and `9` are the **bonus/jackpot symbols**: matching them sets `DAT_801d3cac` to feature mode `6` and seeds a free-spin / multiplier counter `DAT_801d3cb0` (3 spins for id 9, 1 for id 8), kicking off the bonus round and a celebratory actor (`func_0x800653c8`).
4. During an active feature (`DAT_801d3d30 != 0`) the payout is instead the
   **product of the three centre-row values' `(value - 0xf)` factors** - computed
   **unconditionally**, with no all-equal check and no payout-table lookup. Every
   bonus spin pays. The rows read are the centre payline of each reel
   (`display[reel][((pos >> 8) + 0x10) % 0x14]`), the winning line is forced to
   the centre (`DAT_801d3c8c = 1`, so the middle lamp lights), the multiplier
   counter decrements, and the payout is **subtracted from the net-take counter**
   `DAT_801d3d40` (a big bonus win knocks the feature odds back down); when the
   counter hits zero the feature ends and `DAT_801d3798` latches so the next spin
   runs long enough to rotate the symbols back on.

   The strip carries `0x10..=0x19` in a bonus round, so each factor is `1..=10`
   and the payout is `1` (`1×1×1`) to `1000` (`10×10×10`) coins - the same
   `value - 0xf` number the reel *draws* and the marquee *tallies*. The three
   readings are one byte read through one bias; they cannot disagree.
5. In feature mode 3 a `rand % 0x96 == 0` roll can spontaneously clear the feature.

`FUN_801d1af4` (`overlay_slot_machine_801d1af4.txt`) is the **bonus-symbol scanner** run while reels are still settling (state 3): it sweeps the same paylines looking specifically for adjacent `8`/`9` symbols under the active-line masks (`DAT_801d3d10`/`14`/`18`) and, on the first sighting, fires the bonus-anticipation effect (`DAT_801d3ca4`/`DAT_801d3ca8` latches + `_DAT_8007b6dc = 0x200` SFX cue). It sets no payout - it only triggers the "reach" presentation. **Confirmed.**

### The bonus game - the two jackpot symbols

The two jackpot symbols are the **blue "kick"** (id `8`) and the **red "punch"** (id `9`), told apart by their per-symbol reel-art cell in PROT 1200 (the average opaque hue of the `0x0C`-page cell is blue-dominant for symbol 8, red-dominant for symbol 9). Which symbol matches sets how many bonus rounds are earned, pinned in `FUN_801d13e8`:

| line symbol | colour / art | bonus rounds |
|---|---|---|
| `8` | blue "kick" | 1 |
| `9` | red "punch" | 3 |

A bonus round runs as feature mode `6`. The reels rotate onto the **numeral strip**
(`DAT_801d3fd0`, values `0x10..=0x19` - see [Reel strips](#reel-strips---two-of-them-and-a-display-strip-between)),
the player stops each reel with no help from the machine, and the round pays the
**product of the three numbers on the centre payline** - `1` (`1×1×1`) to `1000`
(`10×10×10`) - credited into the balance and **subtracted from the net-take
counter**. `DAT_801d3cb0` counts the earned rounds down; when it reaches zero,
feature mode returns to `0` and the normal slot game resumes. Every bonus spin
still costs 1 coin.

#### The claimed-column tally

The strip across the top of the machine - `0 x 0 x 0` at the start of a round,
filling in each column's number as that reel's stop is taken, then `48 coin` when
the round pays - is not a caption drawn over the machine. It is the **dot-matrix
marquee** ([below](#the-marquee-is-a-dot-matrix-display---fun_801d0e1c)), the same
1014 sprites that scroll the attract legend during the normal game, recomposed
every frame by `FUN_801cfff0`. Two globals carry it, both **Confirmed**:

- **the latch** - `FUN_801d0554` (the per-frame reel integrator) writes, on the
  frame a reel snaps to its landing row:

  ```c
  DAT_801d3d20[reel] = display[reel][((pos >> 8) + 0x10) % 0x14] + 1;   // payline value + 1
  ```

  and state 1 clears all three with the bet charge. The `+ 1` is what makes
  "unclaimed" (`0`) distinguishable from a landed value of `0`.

- **the print** - `FUN_801cfff0`, in feature modes 4..=6 and reel states 3 / 4,
  blits one message per reel at dot columns `reel << 5` (`0`, `0x20`, `0x40`) with
  the multiplication glyph between them (`0x10`, `0x30`):

  ```c
  msg = (claimed > 0xf ? claimed - 0x10 : 0) + 6;   // the numeral, or the "0" glyph
  ```

So a claimed column prints `claimed - 0x10` = `value - 0xf` - **the same number
the reel draws and the same factor the payout multiplies**. The tally is not a
display copy that could drift from the result: it is the result, read one frame
earlier.

The other two faces of the same matrix, also `FUN_801cfff0`: between spins of a
round (states 1 / 2) it shows the **rounds still owed** as three pips (message
`0x12` filled / `0x13` hollow), and once a paying spin tallies it shows the
**payout figure** - digits at fixed right-aligned columns `0 / 0xd / 0x1a / 0x27`,
each drawn only once the figure reaches its place, then the word "coin" at column
`0x34`, whose tail runs off the 78-column matrix exactly as it does on the machine.
It slides down into place over 13 frames (`row = min(frame - 0xd, 0)`).

#### The message bank's roles

The 21 bitmaps are not anonymous, and the payout caption's own arithmetic pins
them: it prints digits with `FUN_801d3230(n / 1000 + 6)`, `(n % 1000) / 100 + 6`,
`/ 10 + 6`, `% 10 + 6`, so records `6..=15` are the glyphs `"0".."9"`. Record
**16** is a glyph of its own for **"10"** - because the tally indexes
`claimed - 0x10 + 6` over a claimed value of `0x10..=0x1A`, and a bonus reel can
land on ten. The bank decodes off the disc as exactly that:

| id | glyph |
|---|---|
| `0`..`5` | the attract legend + the per-feature-mode legends (scrolled by `FUN_801d069c`) |
| `6`..`16` | the eleven numerals `"0"` .. `"10"` |
| `0x11` | the multiplication sign |
| `0x12` / `0x13` | the bonus-round pips, filled / hollow |
| `0x14` | the word `"coin"` |

Ids + dot columns are exported by [`legaia_asset::minigame_slot_scene`]
(`MSG_NUMBER_BASE`, `MSG_TIMES`, `MSG_COINS`, `TALLY_NUMBER_COLS`, …).

The kick/punch symbol ids, their bonus-round counts, the bonus strip's value space
and the `1..=1000` payout bounds are exported from [`legaia_asset::slot_payout`]
(`KICK_SYMBOL_ID` / `PUNCH_SYMBOL_ID`, `KICK_BONUS_ROUNDS` / `PUNCH_BONUS_ROUNDS`,
`bonus_rounds_for`, `BONUS_VALUE_BASE`, `bonus_number_for_value`,
`bonus_round_payout`); the numeral art is [`legaia_asset::minigame_art::slot_bonus_number`].
Disc-checked by `slot_payout_real::kick_is_blue_punch_is_red_on_disc` and
`the_bonus_reels_carry_ten_distinct_numerals_on_their_own_art_page` (ten distinct-
coloured 64x64 cells; the grid goes blank past ten, so `1..=10` is bounded by the
art as well as by the code).

The engine port ([`legaia_engine_core::slot_machine`]) carries the whole cycle:
both source strips, the one-row-per-frame display refill, the free mode-6 stop,
the claimed latch (`SlotMachine::tally` / `tally_product`) and the product payout.
The site's minigames page draws the numerals off the disc and the tally on the
machine's own marquee.

## Coin economy

The credit balance the player accumulates while playing lives in the **overlay-local** word `DAT_801d4114` (capped at `9999999` in the tally path, displayed capped at `99999` by the HUD). It is *not* the casino coin bank. **Confirmed.**

The casino coin bank is the global `_DAT_800845A4` (u32). The slot machine touches it in exactly two places:

- **Read (exchange counter):** `FUN_801e6f70` (`overlay_slot_machine_801e6f70.txt`) renders the casino's **coin-exchange counter**, where coins are bought with gold. It reads `_DAT_800845A4` (current bank) for the "Your Coins" readout and `_DAT_8008459C` for "Your Gold" - that sibling word is the party's **gold**, not a record/high value. It does not modify either. **Confirmed.**

  The counter's "Coins to Buy" entry field is eight single-digit cells stored least-significant-first; their accumulated value times a flat **100 gold per coin** is the "Total Cost" line. The sale is gated twice - on gold against the total, and on the counter's remaining stock (`_DAT_8007BB90`) against the coin count - and the total is drawn in the alert ink when *either* gate fails. Port: `engine-core::slot_machine::coin_exchange_quote`. **Confirmed.** The port is not wired - the host has no casino exchange screen, so nothing calls the quote outside the crate's tests.
- **Write (cash-out commit):** state `100` of `FUN_801cf0d8` does `_DAT_800845A4 = DAT_801d4114` once the cash-out fade completes (`overlay_slot_machine_801cf0d8.txt`, the `_DAT_800845a4 = DAT_801d4114` store). So the bank is **assigned the final playing balance on exit**, not debited/credited per spin. **Confirmed.**

This is why the "Infinite Coins" cheat (`0x800845A4 = 0x05F5E0FF`, see [`cheats.md`](../reference/cheats.md)) works at the casino but does **not** make individual spins free: per-spin betting decrements the *overlay-local* `DAT_801d4114` (loaded from the bank when the machine opens). The cheat-database pointer noted "near `0x801d3cac`" lands in this overlay's state block - `DAT_801d3cac` is the **feature mode**, and the surrounding `0x801d3c80..0x801d4134` window holds the RNG seed, reel positions, state word, balance, submenu cursor and payout counters described in the table below. **Confirmed** address window from the disassembly; the specific cheat-pointer semantics are **Inferred**.

Per-spin betting (**Confirmed**, state `1` of `FUN_801cf0d8`): every spin is a **flat charge** - `DAT_801d4114 -= 3` in the normal modes 0..3 (the overlay's "insert 3 coins" instruction text), `-= 1` in the feature modes 4..6 (so a bonus "free spin" actually costs 1 coin). The same branch accrues the net-take counter: `DAT_801d3d40 += 6` per normal spin, `+= 1` per feature spin. The `< 3` not-enough gate runs before the mode check, so even a 1-coin feature spin needs 3 banked. All five paylines play on every spin - the per-reel line masks `DAT_801d3d10/14/18` are match bookkeeping the win eval and the reach scanner fill at stop time, not player bet selections, and `DAT_801d4110` is the cash-out submenu cursor (its only role).

Session entry (**Confirmed**, `FUN_801cec94` - see the init section above): `DAT_801d4114 = _DAT_800845A4`, with the 70-coin dev fallback when the battle-return flag is clear. So the bank round-trips by assignment on both ends: copied in at entry, assigned back at the state-`100` commit.

## RAM state

All overlay-local; the block clusters in `0x801d3c80..0x801d4140`. **Confirmed** addresses (roles from the disassembly):

| Global | Role |
|---|---|
| `DAT_801d3c80` | slot LCG state (`FUN_801d30cc`; seeded `0x6C0A2AF0` by the init entry) |
| `DAT_801d3c84` | **state word** (reel state machine dispatch) |
| `DAT_801d3c8c` | winning payline index (for the highlight) |
| `DAT_801d3c90` | spin timer / guaranteed-hit countdown (`0x18` on a bonus spin, and on the first spin after one) |
| `DAT_801d3c94` | payout-tally / prompt frame counter |
| `DAT_801d3c9c`/`DAT_801d3ca0` | animation tick counters (marquee, digit blink) |
| `DAT_801d3ca4`/`DAT_801d3ca8` | bonus-anticipation latch + one-shot guard (`FUN_801d1af4`) |
| `DAT_801d3cac` | **feature mode** (0 normal … 6 bonus round) |
| `DAT_801d3cb0` | bonus free-spin / multiplier counter |
| `DAT_801d3cb8` | normal-mode target symbol (`rand%6 + 2`) |
| `DAT_801d3cc0`/`+4`/`+8` | live reel positions (fixed-point, 3 reels) |
| `DAT_801d3cd0..` | reel velocities |
| `DAT_801d3ce0..`/`DAT_801d3cf0..` | per-reel landing offset + search depth |
| `DAT_801d3d00`/`+4`/`+8` | per-reel "stop accepted" flags |
| `DAT_801d3d10`/`14`/`18` | per-reel line-match masks (filled at stop time; read by the reach scanner - not bet selections) |
| `DAT_801d3d20`/`+4`/`+8` | per-reel **claimed value**: `payline value + 1`, latched at the snap (`FUN_801d0554`), cleared at the bet charge. What the marquee's bonus tally prints |
| `DAT_801d3d2c` | reels-stopped count (0..3) |
| `DAT_801d3d30` | feature/bonus-round active flag |
| `DAT_801d3d34` | winning symbol id (`-1` = no win) |
| `DAT_801d3d38`/`DAT_801d3d3c` | this-spin payout (live + latched for display) |
| `DAT_801d3d40` | net-take heat counter: `+6`/`+1` per spin, minus bonus payouts; the feature-odds bracket input |
| `DAT_801d3d50..` (3 × 0x14) | **display strip** - win eval + render read only this; refilled one row per reel per frame from the active source |
| `DAT_801d3e90..` | source strip: the ten reel **symbols** (`slot/2`, ids `0..=9`) |
| `DAT_801d3fd0..` | source strip: the ten bonus **numerals** (`slot/2 + 0x10`, values `0x10..=0x19`) |
| `DAT_801d37a0..` | the marquee's dot buffer (`col * 0x10 + row`), recomposed each frame by `FUN_801cfff0` |
| `DAT_801d3790` | richer-odds flag (widens feature denominators) |
| `DAT_801d3794` | flash/whiteout intensity |
| `DAT_801d3798` | "bonus just ended" flag - forces the next spin's long spin-up so the symbols rotate back onto the payline |
| `DAT_801d4110` | cash-out submenu cursor (`% 3`) |
| `DAT_801d4114` | **player credit balance** (seeded from `_DAT_800845A4` at entry, committed back on exit) |
| `DAT_801d4134` | per-spin landing jitter (`rand%5`) |
| `_DAT_800845A4` | global casino **coin bank** (written on cash-out; read by the HUD) |
| `_DAT_8008459C` | coin record / high value (HUD compare) |

HUD-art and payout tables (overlay rodata; **values** decode from the disc, not reproduced here):

| Table | Role |
|---|---|
| `DAT_801d347c` (0x14 stride, 3 records; PROT 0975 file offset `0x4C64`) | HUD widget sprite-quad descriptors (backdrop panel / marquee / cash-out cursor), indexed by `FUN_801d2cc0`. Layout **Confirmed** - see the symbol-art section above. NOT per-symbol reel art (that is computed in `FUN_801d0fa8`). |
| `0x801D34B8` (14 x u32) | attract-mode instruction-text line pointers into the string block at the overlay head. **Confirmed.** |
| `DAT_801d3598` | per-symbol line-payout byte, indexed by winning symbol id `0..=9` in `FUN_801d13e8` (and the HUD preview `FUN_801d2aa4`). **10 entries** (PROT 0975 file offset `0x4D80`), bounded by zero padding + an overlay string at `+0x10`. Parser [`legaia_asset::slot_payout`]. **Confirmed.** |

The payout table is exactly 10 bytes - one per symbol id, the index range `FUN_801d13e8` special-cases for the jackpot symbols 8/9. The normal-line credit is `balance += DAT_801d3598[symbol]`; a bonus round instead multiplies the three payline `(value − 0xf)` factors (the bonus reel strip carries values `0x10..=0x19`, so the factors are `1..=10`) and does not read this table.

## Key functions

| Address | Role |
|---|---|
| `FUN_801cec94` | overlay init entry: LCG seed `0x6C0A2AF0`, balance seeded from the coin bank (70-coin dev fallback) - `overlay_slot_machine_801cec94.txt` |
| `FUN_801cf0d8` | slot-machine per-frame state machine (the real reel dispatcher) - `overlay_slot_machine_801cf0d8.txt` |
| `FUN_801cfff0` | per-frame HUD/balance + **marquee composer**: the coin readout, and the bonus **tally** / round pips / payout caption into the dot buffer - `overlay_slot_machine_801cfff0.txt` |
| `FUN_801d0554` | per-frame **reel integrator**: advances each reel `+0x66`, snaps a stopping reel to its landing row, and **latches the claimed value** `DAT_801d3d20[reel] = payline value + 1` - `overlay_slot_machine_801d0554.txt` |
| `FUN_801d30cc` | slot LCG RNG (`x*5+1`, 16-bit fold) - `overlay_slot_machine_801d30cc.txt` |
| `FUN_801d258c` | per-spin feature roll (BIOS-rand, net-take-bracketed odds) - `overlay_slot_machine_801d258c.txt` |
| `FUN_801d2114` | per-reel stop: choose target symbol + depth by feature mode (mode 6 = depth 0 / target -1, the free stop) - `overlay_slot_machine_801d2114.txt` |
| `FUN_801d2440` | reel landing search (find target symbol within depth, else next row; a zero depth searches nothing) - `overlay_slot_machine_801d2440.txt` |
| `FUN_801d13e8` | win evaluation + payout-table lookup + bonus trigger - `overlay_slot_machine_801d13e8.txt` |
| `FUN_801d1af4` | bonus-symbol "reach" scanner (presentation only) - `overlay_slot_machine_801d1af4.txt` |
| `FUN_801d2cc0` | HUD widget sprite-quad rasteriser (3-record descriptor table `DAT_801d347c`) - `overlay_slot_machine_801d2cc0.txt` |
| `FUN_801d2aa4` | payout-preview HUD: for each of the 5 lines draws the symbol icon (preview-order table `DAT_801d3784`, page selected by the arg) + its `DAT_801d3598` payout split into tens/units digits via `FUN_801d32c8` - `overlay_slot_machine_801d2aa4.txt` |
| `FUN_801d32c8` | **single-digit glyph emitter** (render-track): builds one gouraud-textured `POLY_GT4` quad for the digit `(x, y, digit)` args - texpage `0x1D`, CLUT `0x7B42`, `U = digit * 0x10`, 16x16, semi-transparent (GP0 word `0x2c808080`, shade `0x70`/`0x80`) - into the prim buffer `_DAT_1f8003a0` (advanced `0x28`) and links it via `func_0x8003d2c4`. The digit-pair caller `FUN_801d2aa4` calls it per place. **Confirmed** from the field-by-field prim writes - `overlay_slot_machine_801d32c8.txt` |
| `FUN_801d0fa8` | **reel cylinder** renderer: trig-table `y`/`z`, `RotTransPers4` quads, arithmetic symbol UVs, per-symbol row-490/491 CLUTs, depth-cue shade - `overlay_slot_machine_801d0fa8.txt` |
| `FUN_801d08e4` | the **billboard pass**: medallions, lamps, reel-stop pedestals, marquee panel + mascots - `overlay_slot_machine_801d08e4.txt` |
| `FUN_801d3380` | the **5 paylines** as `RTPS`-projected 3D line segments - `overlay_slot_machine_801d3380.txt` |
| `FUN_801d0e1c` | the **dot-matrix marquee**: 78x13 individually projected 2x2 sprites - `overlay_slot_machine_801d0e1c.txt` |
| `FUN_801d069c` | marquee dot-buffer composer (scrolling blit; `msg < 0` clears) - `overlay_slot_machine_801d069c.txt` |
| `FUN_801d3230` | marquee dot-buffer blit at a `(col, row)` - `overlay_slot_machine_801d3230.txt` |
| `FUN_801d2914` | coin-readout digit renderer (screen space) - `overlay_slot_machine_801d2914.txt` |
| `FUN_801d079c` | **bonus-rounds-remaining lamp indicator** (render-track): emits three 16x16 textured HUD sprites (`x = 0x230` step `0x10`, tpage `0x84`, CLUT `0x7A8D`) into `_DAT_1f8003a0`; the first `DAT_801d3cb0` of them (the bonus free-spin counter) draw at the bright/dim shade selected by `DAT_801d3c9c & 1` (the blink tick), the rest neutral, then link a trailing quad via `func_0x80059010` - `overlay_slot_machine_801d079c.txt` |
| `FUN_801d30f8` | **attract-text column draw** (render-track): sets text attribute `DAT_80073f20 = 0xc`, then walks the 14-entry instruction-line pointer table `0x801d34b8` drawing each string via `func_0x80036888` at `(param1, param2)`, advancing y by `0xd` per line - `overlay_slot_machine_801d30f8.txt` |
| `FUN_801d317c` | **UI panel quad** (render-track): emits one 168x48 `POLY_FT4` textured quad (tag `0x9000000`, colour `0x2c808080`, tpage/CLUT word `0x7b43`) at caller `(param1, param2)` into `_DAT_1f8003a0`, linked via `func_0x8003d2c4` - `overlay_slot_machine_801d317c.txt` |
| `FUN_801d13c4` | **effectively empty**: a 3-iteration counter loop with no memory writes and no calls (loads the address of `DAT_801d3cc0` but never uses it); a dead stub - `overlay_slot_machine_801d13c4.txt` |
| `FUN_800172c0` | the per-frame **scene camera** the machine's 3D emits project through (SCUS) |
| `FUN_800195a8` | the **billboard projector** (SCUS): view-space centre + half-extent -> projected quad |
| `FUN_8005bac8` / `FUN_8003d368` | `RotTransPers4` / `RTPS` (SCUS GTE wrappers) |
| `FUN_801e6f70` | coin-exchange counter: cost at 100 gold/coin + gold/stock gates - `overlay_slot_machine_801e6f70.txt` |

The overlay is **extraction PROT 975** (dev module `other4`), loaded by the **mode-24 minigame
door-warp** as sub-id 3 (field-VM op `0x3E` with `op0 = 103`; `FUN_80025980` →
`FUN_8003EBE4(0x50)`, init entry `0x801CEC94`): the documented reel SM `FUN_801CF0D8` and payout
`FUN_801D13E8` land on function prologues in that entry at the canonical slot-A base
`0x801CE818`, and the `"insert 3 coins"` / `"game_coin %d"` strings sit inside its runtime
slice. Two sibling dev modules occupy sub-ids 1/2 (PROT 973 `OTHER2`, a 1-sector module; PROT
974 `OTHER3` - identities open). The earlier "PROT 973, loaded by the Mode-0 config-init
handler" note carried the loader-math off-by-2 *and* matched this overlay's image inside 973's
over-read tail - mode 0 actually loads the debug-menu overlay PROT 971. See [`script-vm.md §
0x3E WARP`](script-vm.md#0x3e-warp-mode-24-minigame-door-warp).

## Engine port

[`legaia_engine_core::slot_machine`](../../crates/engine-core/src/slot_machine.rs) is the clean-room rules engine over this page. The **Confirmed** kernels are ported directly:

- the slot LCG (`SlotRng`, `x*5+1` + 16-bit fold; `FUN_801d30cc`);
- **both** 20-slot strips per reel, built in retail's interleaved draw order (`build_reel` / `build_strip`: mod-`0x14` draw + `+0xd` / `+1` probe, values `slot/2` and `slot/2 + 0x10`; `FUN_801cf0d8` case 0);
- the **display strip** and its one-row-per-frame refill from the active source, `DISPLAY_REFRESH_LEAD` = 9 rows ahead of the payline (`SlotMachine::tick`; the `FUN_801cf0d8` render tail) - so the bonus round's numerals rotate in and out exactly as they do on the machine, with `BONUS_SPIN_UP_FRAMES` = `0x18` buying the travel on both edges;
- the net-take-bracketed feature roll (`feature_roll`; `FUN_801d258c`, exact draw order + bracket edges);
- the flat spin charge + net-take accrual (3/+6 normal, 1/+1 feature);
- the per-mode stop plan + landing search, including mode 6's **free stop** (depth `0`, no target) (`stop_plan` / `land_row`; `FUN_801d2114` / `FUN_801d2440`);
- the per-reel **claimed latch** and the marquee tally over it (`SlotMachine::claimed` / `tally` / `tally_product`; `FUN_801d0554` + `FUN_801cfff0`);
- the **marquee composition** itself (`SlotMachine::marquee` / `marquee_placements` over [`legaia_asset::minigame_slot_scene`]'s `compose_marquee_frame` / `place_message` / `clear_dots` / `render_marquee`; `FUN_801cfff0` + `FUN_801d3230` + `FUN_801d069c`) - see [the dot matrix's two blits](#the-dot-matrix-has-two-blits-not-one);
- the **five**-payline / payout / bonus-round evaluation - the bonus product over the payline `(value - 0xf)` factors, the centre winning line, the product subtracted from the net take (`SlotMachine::evaluate_spin`, per-reel row offsets from [`legaia_asset::minigame_slot_scene`], payout via [`legaia_asset::slot_payout`]; `FUN_801d13e8`);
- the entry constants (`ENTRY_DEFAULT_BALANCE` = 70, `ENTRY_LCG_SEED` = `0x6C0A2AF0`; `FUN_801cec94`);
- the coin economy (balance seeded from the bank, `9999999` tally cap, cash-out **assignment** back into the bank).

The engine-side reconstructions (each marked at its site): the spin-up pacing constants, the BIOS-`rand` stream substituted with a deterministic LCG, and feature modes 3/5 folded to the normal landing plan.

Runtime wiring: a suspending scene mode (`SceneMode::SlotMachine`; `World::enter_slot_machine` / `tick_slot_machine` / `exit_slot_machine`, which performs the state-100 bank commit into `World::casino_coins` = `_DAT_800845A4`). The `play-window` viewer starts it from the `O` key (loads PROT 0975, `slot_payout::parse`); Cross spins / stops / collects. Disc-gated `slot_minigame_real` drives real-table spins through the World pad path.

## Rendering - a 3D scene

The machine is **not** a sprite collage. Every element on its face is a quad in a
3D scene, projected through the GTE and depth-sorted into the ordering table. The
slot overlay contains **no `cop2` instruction of its own** - it reaches the GTE
entirely through the SCUS wrappers - so a sweep for GTE ops inside the overlay
reports the machine as 2D. It is not. The wrappers it goes through:

| SCUS | GTE op | Role |
|---|---|---|
| `FUN_8003d368` | `cop2 0x180001` = **RTPS** | project one vertex (`VXY0`/`VZ0` in, `SXY2` out) |
| `FUN_8005bac8` | `RTPT` + `RTPS` | project a 4-vertex quad (`RotTransPers4`) |
| `FUN_800195a8` | via `8003d344` (`MVMVA`) + `8005bac8` | the **billboard projector**: transform a 3D centre into view space, build four corners around it at a view-space half-extent, project |
| `FUN_800172c0` | matrix compose + `SetRotMatrix` / `SetTransMatrix` | the per-frame scene camera |

### The camera

`FUN_800172c0` runs every frame before the machine's 3D emits. The init clears
the camera rotation `_DAT_8007b790` to zero and writes the scale matrix
`_DAT_8007bf10` as `diag(0x6000, 0x3000, 0x3000)` = `diag(6, 3, 3)` in 4.12 fixed
point. Identity rotation is what makes the machine face the camera head-on; the
2:1 x:y scale is the **640-wide hi-res video mode's** pixel aspect (the init sets
mode `0x280` = 640, so horizontal pixels are half-width). The projection distance
`_DAT_8007b6f4` is set to `0x400`.

**`-z` is toward the viewer.** The *glass* (paylines, medallions, lamps,
pedestals, marquee) sits at `z = -768` / `-800`; the reel cylinders are centred on
`z = 0` with the symbol on the payline at `z = -512` - i.e. **behind** the glass.

### The reels are cylinders - `FUN_801d0fa8`

Each reel emits 8 `POLY_GT4` faces per frame. A face spanning reel angles `a` and
`a + 0x100` has corners

```
(x,            y(a),     z(a)   )   (x + 0x100,  y(a),     z(a)   )
(x,            y(a+100), z(a+100))   (x + 0x100,  y(a+100), z(a+100))
```

with `y(a) = (sin(a) * -0x249) >> 12` and `z(a) = cos(a) >> 3` - the SCUS sine /
cosine tables (4096-entry, amplitude `0x1000`, reached through the pointers
`_DAT_8007b81c` / `_DAT_8007b7f8`; the tables themselves live at SCUS
`0x80070A2C` / `0x8007122C`). So the reel is an ellipse of radius 585 in `y` and
512 in `z`: **a cylinder**, and the four corners go through `RotTransPers4`. The
symbols curl away from the payline because the cylinder does.

Reel `r` spans `x = -0x200 + r * 0x180` to `+ 0x100` (`FUN_801cf0d8`'s render
tail). The first face's angle is `0x380 + (pos & 0xFF)` - the low byte of the reel
position is the **sub-symbol fraction**, and it is what rotates the cylinder
between symbols.

The gouraud shade is depth-cued off `z`:

```
shade = clamp(0xB4 - ((z + 0x200) * 0x21C >> 9), 0, 0xB4)
```

and the `POLY_GT4` blend is `texel * shade / 128`, so the shade peaks (a 1.41x
**brighten**, not a clamp) exactly at `z = -0x200 = -512` - the payline face - and
falls to black within ~48 degrees either side. That fade is what caps each reel
window top and bottom, and what hides the near half of the cylinder: there is no
backface cull, the near half is simply shaded to black.

The **payline face is the one that carries the payline row** - the strip row the
win eval pays on (`strip[(pos >> 8) % 0x14]`). It is not the first face emitted:
with the first face at angle `0x380` and a `0x100` step, the face at the shade
peak is the *fifth* (`z(0x380 + 4*0x100 + 0x80) = -512`), and the faces above /
below it carry the rows either side as the strip walks downward with the angle. A
renderer that indexes the strip straight off the face index is off by that
constant, and draws a payline whose three symbols are not the three that paid.

### The paylines are 3D lines - `FUN_801d3380`

Five `LINE_F2` prims, each of whose two endpoints is `RTPS`-projected on its own.
The geometry is the 5 x 16-byte table `DAT_801d3680` (`[SVECTOR a, SVECTOR b]`),
all at `z = -768`, `x` from `-640` to `+640`: three horizontal at `y = -192 / 0 /
+192` and two diagonals crossing at `y = ±320`. The winning line (`DAT_801d3c8c`)
is drawn bright.

The packet is a `LINE_F2` with GP0 code `0x43` - flat, **semi-transparent**. The
idle colour is a neutral `0x808080`; the winning line is redrawn by overwriting
only the three colour bytes of the already-assembled command word with
`(0xFF, 0xFF, 0x80)`, so the `0x43` code byte survives and a lit line is still
semi-transparent. The highlight test is a plain equality against `DAT_801d3c8c`,
which means a frame where that word still holds `0` lights line `0` - only a
value outside `0..5` leaves the whole rack dark.

Retail projects each endpoint separately, then links the packet at the OT bucket
derived from the **second** endpoint's returned depth alone:
`(depth >> 2) >> ctx[+0x90]`, with a `+3` bias first when the depth is negative
so the shift truncates toward zero.

Ported as `legaia_engine_core::slot_machine::payline_prims` +
`payline_ot_depth`; the geometry comes from the parsed table
(`legaia_asset::minigame_slot_scene::SlotScene::paylines`), and projection plus
OT linkage stay caller-side as they do for the rest of the machine's furniture.

### The furniture is billboards - `FUN_801d08e4`

Four passes, each through `FUN_800195a8`. All the geometry is disc data, in four
tables that **tile contiguously** from file offset `0x4E68` to `0x4F38` (PROT 0975;
the overlay's load base is `0x801C_E818`, so `file = VA - 0x801C_E818`):

| Table | VA | File | Records | Draws |
|---|---|---|---|---|
| paylines | `DAT_801d3680` | `0x4E68` | 5 x 16B | the 5 line segments (above) |
| medallions | `DAT_801d36d0` | `0x4EB8` | 5 x 8B | the payline medallions down the **left** |
| lamps | `DAT_801d36f8` | `0x4EE0` | 5 x 8B | the payline lamps down the **right** |
| marquee | `DAT_801d3720` | `0x4F08` | 3 x 16B | the marquee panel + the two mascots |

- **Medallions** (`SVECTOR pos`, whose `pad` word is the CLUT column): page
  `0x0C`, cell `uv (0xA8, 0x80)` 32x32, CLUT `0x7A80 + art`, view-space half
  `0x1A0 x 0xD0`. One cell of artwork recoloured five ways; the column is
  symmetric (`2, 1, 0, 1, 2` top to bottom), and each medallion's `y` is its
  payline's `y`.
- **Lamps**: page `0x1C`, CLUT `0x7B09`, half `0xB4 x 0xA0`; unlit cell
  `uv (0x10, 0xE0)` 16x16, lit cell `uv (0, 0xE0)` - the winning line's lamp
  lights.
- **Reel-stop pedestals** (positions computed, not tabled: `x = -0x180 + r *
  0x180`, `y = 0x1E0`, `z = -800`): page `0x1C`, half `0x230 x 0x120`, 32x32 cell
  on row `v = 0x80 + r * 0x20`. While the reel spins it draws `u = 0x60` with CLUT
  `0x7B03 + r`; once the reel is stopped the palette swaps to `0x7B06 + r` **and
  the cell slides left to `u = 0`** - the stop branch overrides only the `U`s, so
  the pedestal stays on its own row. That swap is how retail shows a taken stop.
- **Marquee panel + mascots**: page `0x1C`, CLUT `0x7B00 + clut_off`; each record
  carries its own view-space half-extent and its own texture cell. The panel's
  interior is palette index 0 - **transparent**: the navy behind the legend is the
  cabinet's, not the panel's.

### The marquee is a dot-matrix display - `FUN_801d0e1c`

A **78 x 13 grid of individually projected 2x2 sprites** - 1014 of them, one
`RTPS` each, at `(-0x1AD + col * 0xB, -0x280 + row * 0xC, -800)`. Each dot samples
page 3 at `(u, 0)` where `u` is its byte in the dot buffer `DAT_801d37a0`
(`buf[col * 0x10 + row]`), and the buffer holds `nibble << 2` - so a nibble `n`
picks the lamp swatch at page-3 `u = n * 4`. Nibble `0` is an unlit dot.

The dots' CLUT is `0x7B4F` (row 493, **column 15**) - and that column is **empty
on the disc**. The reel SM `MoveImage`s a 16x1 palette from `((tick & 1) * 16,
493)` into it every frame: the marquee **blinks** between page 3's CLUT columns 0
and 1. Decoding column 15 straight off the disc yields a fully transparent,
invisible marquee.

The buffer's content is a **message bank**: 21 records at `DAT_801d34f0` (file
`0x4CD8`, 8-byte stride `[u8 u, u8 v, u8 w, u8 h, u32 runtime_ptr]`), every one 13
rows tall, laid out on page 3 at `v = 16..144`. `FUN_801CEC94` `StoreImage`s each
rect back out of VRAM and expands its nibbles into a byte-per-texel bitmap;
`FUN_801d069c` (scrolling blit, `msg < 0` clears) and `FUN_801d3230` (blit at a
`(col, row)`) compose them into the dot buffer.

Every record's role is pinned - six legends, **eleven** numerals `"0".."10"`, the
multiplication sign, the two round pips and the word "coin" - by the ids
`FUN_801cfff0` indexes them with; see
[the message bank's roles](#the-message-banks-roles). This is where the bonus
round's `0 x 0 x 0` tally and its `48 coin` payout caption are drawn: the marquee
is not decoration, it is the machine's readout.

### The dot matrix has two blits, not one

`FUN_801d069c` and `FUN_801d3230` are not variants of one copy loop, and the
difference decides what the marquee can express. They clip opposite ends of the
copy:

| | `FUN_801d069c` | `FUN_801d3230` |
|---|---|---|
| Offsets | the **source** `(x, y)` | the **destination** `(col, row)` |
| Clips | source coords, signed | dest coords, **unsigned** |
| Buys | scrolling one message through a fixed window | placing a message at a spot |

The unsigned clip is how one `sltiu` covers both bounds at once: a negative
offset fails the compare as a huge unsigned value, so there is no separate `< 0`
test. That is not a micro-optimisation to gloss over in a port - the payout
caption is composed at `row = min(frame - 0xD, 0)`, i.e. it *starts* 13 rows
above the matrix and counts up to 0, and the unsigned clip is the only thing
hiding the rows that have not arrived. Port the bound as a bare
`row < DOT_ROWS` and the caption appears fully formed on its first frame.

A negative `msg` id is `FUN_801d069c`'s **clear** command rather than a lookup:
the `bgez $a0` at its head skips the scroll body into a `78 x 13` zero-fill of
`DAT_801d37a0`. `FUN_801cfff0` opens every frame with that call, so the marquee
is rebuilt from scratch each frame and never diffed.

`FUN_801cfff0` then picks the frame's one occupant. The payout caption wins the
strip whenever it is up - retail gates it on **both** the figure `DAT_801d3d3c`
and the frame clock `DAT_801d3c94` being non-zero - and only when it is down do
the bonus tally / round pips draw, and then only in feature modes `4..=6` and
reel states `1..=4`. The caption's leading-zero suppression tests the **whole
figure** at each of its four places, not the running remainder: all four guards
re-read `DAT_801d3d3c`, while the digit values come off a remainder chain that
runs whether or not its own place drew. So `405` prints `4`, `0`, `5` - an
interior zero is kept - and `7` prints a bare `7` in the units column.

### The two screen-space draws - `FUN_801d2cc0`

The **only** things on the machine that do not go through the GTE. The 3-record
descriptor table `DAT_801d347c` (file `0x4C64`, 20-byte stride) is rasterised at a
caller-supplied pixel position:

| Rec | Drawn at | Cell | Role |
|---|---|---|---|
| 0 | screen `(560, 128)` | page `(640, 0)`, `uv (0, 16)`, 127x239 | the **paytable board** on the right ("x30 back" / "x9 back" / "Bonus games", with the coin box under it) |
| 1 | screen `(560, 160)` | page `(768, 0)`, `uv (0, 192)`, 64x16 | the **"COIN"** label |
| 2 | screen `(0xDC, cursor * 0x10 + 0x6C)` | page `(832, 256)`, `uv (96, 160)`, 16x16 | the cash-out cursor |

Record 0's page is sampled **8bpp** - its texpage attribute `0x8A` has the GPU's
8-bit colour bit set, so the 64-halfword-wide block is 128 texels across and its
CLUT is one 256-entry palette. The TIM header declares 4bpp; decoding it as the
header claims yields noise.

The coin digits are `FUN_801d2914` at screen `(546, 168)`: `U = 0x40 + digit *
0x10`, `V = 0xC0`, 16x16, CLUT `0x7A8D`, zero-padded to 5.

Parser [`legaia_asset::minigame_slot_scene`]; art [`legaia_asset::minigame_art`].

### The projection

The scene's screen mapping on the retail 640x240 framebuffer. Its **shape** is
derived - a perspective divide of a view-space point whose x:y scale ratio is
exactly 2, read out of the camera matrix. Its four **scalars** are *fitted* to a
retail framebuffer captured at the machine (the `minigame_slot_machine` capture),
because the GTE control words (`OFX` / `OFY` / `H`) live in COP2, not in main RAM,
and so are not in a save state.

The fit is over-determined and independently checked: it was solved on the five
payline lamps alone, and then **predicted** - to about a pixel each - the
on-screen rect of every other element, none of which entered the fit (the
medallion column, the marquee panel, the two mascots, the three reel windows, the
reel-stop pedestals, and the dot-matrix grid).

## Art pack (PROT 1200)

The overlay init `FUN_801CEC94` loads the machine's textures from **extraction
PROT entry 1200** (raw TOC `0x4B2`). The entry is a descriptor container whose
descriptor 0 (`TIM_LIST`, type `0x01`) LZS-decodes to a [`pack`](../formats/pack.md)
of **five standard PSX TIMs**. Their framebuffer destinations *are* the texture
pages and CLUT rows the reel renderer and the HUD rasteriser sample, which closes
the per-page fb-coordinate question:

| pack | image fb | CLUT row | texpage attr | role |
|---|---|---|---|---|
| 0 | `(768, 0)` | 490 | `0x0C` | reel symbols + digit font + the payline medallion |
| 1 | `(832, 0)` | 491 | `0x0D` | the bonus round's reel faces: the numerals `1..=10` |
| 2 | `(768, 256)` | 492 | `0x1C` | marquee panel, mascots, reel-stop pedestals, payline lamps |
| 3 | `(832, 256)` | 493 | `0x1D` | dot-matrix message bank + the marquee's lamp swatches + cursor |
| 4 | `(640, 0)` | 494 | `0x8A` | the paytable / coin info panel - sampled **8bpp** |

Page 4 is **not the cabinet** (see [Open](#open)): it is the paytable board the
HUD rasteriser draws on the right of the machine, and its texpage attribute has
the GPU's 8-bit colour bit set, so it is 128 texels wide with one 256-entry
palette - not the 4bpp its TIM header declares.

Every image block is **byte-identical to a retail VRAM dump** taken at the machine
(`minigame_slot_machine` capture), so `texpage 0x0C = (768,0)` / `0x0D = (832,0)`
is Confirmed, not inferred. The siblings PROT 1198 / 1199 (raw `0x4B0` / `0x4B1`)
are **not** backdrop art - they are the machine's *sound* bank (see below).

Sprite geometry, all Confirmed:

- **Reel symbols** - `FUN_801d0fa8`, no descriptor table: a 4x4 grid of 64x64
  cells on the `0x0C` page, `U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, and
  a **per-symbol CLUT** at `0x7A80 + sym` (row 490, column `sym`). The palette is
  load-bearing: symbol ids 0/1/2 are *one* cell of artwork recoloured three ways,
  as are 4/5 - a renderer that ignores the CLUT draws three identical reels.
- **Bonus numerals** - the same three lines, rebased. A strip value `>= 0x10`
  bumps the texpage to `0x0D` and the CLUT base to `0x7AC0`, so the ten numerals
  are their own 64x64 cells on page 1 - `U = (v & 3) * 0x40`, `V = (v & 0xC) * 0x10`,
  CLUT `0x7AC0 + (v & 0xF)` (row 491, one **column per numeral**). Every digit on
  the retail bonus reels is a different colour because every one has its own
  palette column; the 4x4 grid goes blank past the tenth, which bounds the numeral
  bank at `1..=10` from the art side. Decoder
  [`minigame_art::slot_bonus_number`](../../crates/asset/src/minigame_art.rs); the
  reels are **not** the coin font scaled up.
- **Digit font** - `FUN_801d2914`: `U = 0x40 + digit * 0x10`, `V = 0xC0`, 16x16
  per glyph, CLUT `0x7A8D`. The 64x16 cell at `(0, 0xC0)` is the **"COIN"** label -
  which is what HUD record 1 (below) actually points at. This font is the coin
  readout's only - the bonus reels and the marquee each have their own.
- **HUD widgets** - the 3 records of `DAT_801d347c` resolve to the **paytable
  board** (`(640,0)` page, CLUT row 494, `uv (0,16)`, `127x239`, 8bpp), the
  **"COIN" label** (`(768,0)` page, CLUT `0x7A8D`, `uv (0,192)`, `64x16`) and the
  cash-out cursor (`(832,256)` page, `uv (96,160)`, `16x16`). These three are the
  machine's only screen-space draws - see
  [The two screen-space draws](#the-two-screen-space-draws---fun_801d2cc0).

Parser [`legaia_asset::minigame_art`]; the machine's *geometry* is
[`legaia_asset::minigame_slot_scene`].

## Sound

The machine's cues are **runtime-bank** ids (`>= 0x200`), so they resolve through
the cue ring's second space (see [`sfx-table.md`](../formats/sfx-table.md)): the
descriptor block is the overlay's `efect.dat` at **extraction PROT 1199** (raw
`0x4B0`... loaded by the same init), and the samples come from the class-2 VAB at
**extraction PROT 1198**. Descriptors are 8 bytes - `[program, tone, note, voices,
class]` - starting at the `u16` at `bank + 2`. The block yields exactly **11**
class-2 records over 2 programs (4 tones + 7), and the PROT 1198 VAB declares
exactly 2 programs and 11 tones: the agreement is what pins the table offset.

| Event | Cue | Site |
|---|---|---|
| reel stop (once per reel) | `0x20A` | `FUN_801CF0D8` case 3 |
| payout tally tick | `0x209` | `FUN_801CF0D8` case 4 |
| reach / anticipation | `0x201` / `0x202` | `FUN_801CF0D8` |
| second jackpot symbol sighted | `0x200` | `FUN_801D1AF4` |
| confirm / cursor / cancel | `0x20` / `0x21` / `0x37` | static table, class-0 VAB (PROT 0868) |

The reel-spin *loop* is not a ring cue: it is a voice driven straight through
`FUN_80065034` - the reel SM calls `func_0x80065034(0x13, 2, 1, 0, 0x3C, 0x40,
0x28, 0x28)` (voice `0x13`, class-2 VAB, program 1, tone 0, note `0x3C`,
volume `0x28`) as the reels start, and releases the voice on all-reels-stop.
Decode it with `SfxCueBank::decode_tone` (constants
`minigame_sfx::SLOT_SPIN_*`).

The slot machine starts **no BGM** - it inherits the host scene's: the casino
floor `0543_koin1` starts BGM id `2018` via field-VM op `0x35` = `music_01`
sound-test slot 18 ("Sol casino", extraction 1006 - the bank map is piecewise,
so a slot in this range is extraction `988 + slot`, not `990 + slot`;
`legaia_asset::slot_payout::SLOT_HOST_BGM_PROT_INDEX`). Parser
[`legaia_asset::minigame_sfx`].

## Open

- The machine's own **in-game BGM** is unpinned: the overlay never starts a track,
  and every library capture reaches the minigame through a debug warp from
  `town01`, so the inherited track in those states is Rim Elm's, not the casino's.
  Pinning it needs a capture taken by walking into the Sol casino.
- The machine's **cabinet** - the grey body, the dark-red face the reels sit in,
  the navy marquee backing and the lit floor ramp - has **no pinned emitter**. It
  is in neither the art pack nor any prim a traced slot function emits: no slot
  function emits a large untextured quad or a body mesh, no slot art page holds
  cabinet art, and the live prim pool at the machine carries ~950 `POLY_FT4` +
  several hundred gouraud prims per frame that no traced slot function accounts
  for. The earlier "it is the casino room's own 3D geometry (`koin1`..`koin6`)"
  reading is **falsified**: the `minigame_slot_machine` capture reaches the
  machine by a debug warp from `town01`, a RAM TMD census over that capture finds
  town01's env meshes resident (56 of 114 body-slice matches) and effectively
  none of any `koin*` bundle's, and both framebuffers still carry the fully-drawn
  cabinet - so the casino room's geometry cannot be what draws it. The emitter is
  either an untraced slot/overlay-host function or a shared-renderer table loaded
  with mode 24; pinning it needs a prim-pool-to-code trace at the machine. The
  site's minigames page draws the cabinet as a composition **measured off the
  capture's framebuffer** (edges + colours), and says so.

## See also

**Reference** -
[Tile-board grid](tile-board.md) ·
[Cheats](../reference/cheats.md) ·
[Casino prize exchange (randomizer)](../tooling/randomizer.md)
