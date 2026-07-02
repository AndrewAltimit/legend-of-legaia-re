# Casino slot machine

The casino's slot-machine minigame: three reels of pictographic symbols, a flat 3-coin spin played across all three paylines, a per-spin win evaluation against a payout table, and a running coin balance the player can cash back out to the casino coin bank. It lives in the minigame-hub overlay (the binary shared with fishing / Baka Fighter / dance), so the locomotion, sprite, actor-VM and SDK helpers it leans on are documented elsewhere - this page covers only the slot-specific logic.

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

The tail of `FUN_801cf0d8` (after the switch) always advances the three reel positions, redraws the visible symbols via `FUN_801d0fa8`, and refreshes the marquee/HUD. There is **no bet-line selection anywhere in the machine** - every spin plays all three paylines for the flat cost (the earlier "bet-line selector" reading of `DAT_801d4110` conflated it with the cash-out submenu cursor, which is that word's only role).

### Entry init - `FUN_801cec94`

The overlay's init entry (mode-24 warp target `0x801CEC94`) seeds the session before state `0` runs. **Confirmed** from the disassembly (the function sits below the first dumped handler): the slot LCG `DAT_801d3c80` is written the literal seed `0x6C0A2AF0`; the playing balance is **assigned from the casino coin bank** (`DAT_801d4114 = _DAT_800845A4` - the symmetric counterpart of the state-`100` commit); when the battle-return flag `_DAT_8007B8B8` is zero (the overlay launched outside the casino door path) the balance instead defaults to `0x46` = 70 coins - a dev-launch fallback, printed by the adjacent `"battle_return_flag %d"` / `"game_coin %d"` debug strings; and the state word is cleared to `0`.

### Reel strips

Each of the 3 reels is a 20-symbol (`0x14`) strip. Two parallel strip arrays exist - `DAT_801d3e90` and `DAT_801d3fd0`, each `3 × 0x14` ints at `0x50` stride - plus a display copy at `DAT_801d3d50` (the array win-eval and the renderer read). Init (`FUN_801cf0d8` case 0) fills a strip by, for each of the 20 slots, drawing a fresh RNG value, reducing it mod `0x14`, and probing forward (`+0xd` step for one array, `+1` for the other) until it finds an unused slot - a collision-resolving permutation that scatters each symbol id `slot/2` (so symbol ids run `0..9`, two strip positions each) around the reel. **Confirmed** from `overlay_slot_machine_801cf0d8.txt` (the `% 0x14` / `(uVar1 + 0xd) % 0x14` placement loops).

The live reel position `DAT_801d3cc0[reel]` is a fixed-point angle; the on-screen symbol index is `(pos >> 8)` reduced mod `0x14`, and adjacent rows are read at `±1`/`±0x10`/`±0x11` offsets (the three pay rows).

### Symbol art - computed, not tabled

The reel-symbol quads are drawn by `FUN_801d0fa8` with **arithmetic UVs from the symbol value itself** - no descriptor table is involved (**Confirmed**, `overlay_slot_machine_801d0fa8.txt`): the art is a grid of 64x64 cells (`U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`), the texpage attribute is `0x0C` for the normal symbols and `0x0D` for the bonus-strip values `>= 0x10`, and each symbol has its own CLUT at id `0x7A80 + sym` (VRAM row 490, column = symbol; the bonus strip uses `0x7AC0 +` = row 491). The gouraud shade fades each row by distance from the payline (`0xB4` cap), curling the reel.

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

Record 0 is the 128x240 reel-window backdrop panel, record 1 the 64x16 marquee, record 2 the 16x16 cash-out-cursor arrow (`FUN_801cf0d8` state `0x32` positions it at `DAT_801d4110 * 0x10 + 0x6C`). Immediately after the third record (`0x801D34B8`) the region becomes a **14-entry pointer table** - the attract-mode instruction-text lines, pointing at the string block at the head of the overlay ("To spin the wheels, insert 3 coins by pressing the ... buttons", the earlier "transitions into a pointer array" observation).

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
| `5`,`6` | further bonus/hold variants |

`FUN_801d2440(reel, depth, target_symbol)` (`overlay_slot_machine_801d2440.txt`) is the actual landing search: starting from the current reel position it walks up to `depth` rows looking for `target_symbol` on the display strip; if found it returns a stop offset that lands the symbol on the payline, otherwise it returns the next natural row (no forced result). The `+ DAT_801d4134 * 0x10` term (a per-spin 0..4 jitter chosen in `FUN_801d258c`) nudges the exact landing so the stop does not look mechanical. **Confirmed.**

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

1. For each of the three diagonal/straight paylines it reads the three on-screen symbols (display strip `DAT_801d3d50` at the per-reel `±` row offsets) and checks all-three-equal. It keeps the **highest-value matching line** (`DAT_801d3d34` = winning symbol id, `DAT_801d3c8c` = winning line index for the highlight).
2. If a line wins, the credited amount is `DAT_801d3d38 = payout_table[winning_symbol]`, read as a byte from a table at `DAT_801d3598` indexed by symbol id (so payout scales with symbol rarity). **Inferred** values (not reproduced).
3. Symbol ids `8` and `9` are the **bonus/jackpot symbols**: matching them sets `DAT_801d3cac` to feature mode `6` and seeds a free-spin / multiplier counter `DAT_801d3cb0` (3 spins for id 9, 1 for id 8), kicking off the bonus round and a celebratory actor (`func_0x800653c8`).
4. During an active feature (`DAT_801d3d30 != 0`) the payout is instead the **product** of the three payline symbols' `(value - 0xf)` factors - computed **unconditionally**, with no all-equal check (the mode-6 stop plan drives the line together, and every bonus spin pays *something*). The multiplier counter decrements and the payout is **subtracted from the net-take counter** `DAT_801d3d40` (`DAT_801d3d40 -= DAT_801d3d38` - a big bonus win knocks the feature odds back down); when the counter hits zero the feature ends.
5. In feature mode 3 a `rand % 0x96 == 0` roll can spontaneously clear the feature.

`FUN_801d1af4` (`overlay_slot_machine_801d1af4.txt`) is the **bonus-symbol scanner** run while reels are still settling (state 3): it sweeps the same three paylines looking specifically for adjacent `8`/`9` symbols under the active-line masks (`DAT_801d3d10`/`14`/`18`) and, on the first sighting, fires the bonus-anticipation effect (`DAT_801d3ca4`/`DAT_801d3ca8` latches + `_DAT_8007b6dc = 0x200` SFX cue). It sets no payout - it only triggers the "reach" presentation. **Confirmed.**

## Coin economy

The credit balance the player accumulates while playing lives in the **overlay-local** word `DAT_801d4114` (capped at `9999999` in the tally path, displayed capped at `99999` by the HUD). It is *not* the casino coin bank. **Confirmed.**

The casino coin bank is the global `_DAT_800845A4` (u32). The slot machine touches it in exactly two places:

- **Read (entry / HUD):** `FUN_801e6f70` (`overlay_slot_machine_801e6f70.txt`) renders the coin HUD; it reads `_DAT_800845A4` (current bank) for the on-screen coin readout and the sibling word `_DAT_8008459C` for a record/high value, comparing them to flag a new record. It does not modify the bank. **Confirmed** (`func_0x80034b78(_DAT_800845A4, …)`).
- **Write (cash-out commit):** state `100` of `FUN_801cf0d8` does `_DAT_800845A4 = DAT_801d4114` once the cash-out fade completes (`overlay_slot_machine_801cf0d8.txt`, the `_DAT_800845a4 = DAT_801d4114` store). So the bank is **assigned the final playing balance on exit**, not debited/credited per spin. **Confirmed.**

This is why the "Infinite Coins" cheat (`0x800845A4 = 0x05F5E0FF`, see [`cheats.md`](../reference/cheats.md)) works at the casino but does **not** make individual spins free: per-spin betting decrements the *overlay-local* `DAT_801d4114` (loaded from the bank when the machine opens). The cheat-database pointer noted "near `0x801d3cac`" lands in this overlay's state block - `DAT_801d3cac` is the **feature mode**, and the surrounding `0x801d3c80..0x801d4134` window holds the RNG seed, reel positions, state word, balance, submenu cursor and payout counters described in the table below. **Confirmed** address window from the disassembly; the specific cheat-pointer semantics are **Inferred**.

Per-spin betting (**Confirmed**, state `1` of `FUN_801cf0d8`): every spin is a **flat charge** - `DAT_801d4114 -= 3` in the normal modes 0..3 (the overlay's "insert 3 coins" instruction text), `-= 1` in the feature modes 4..6 (so a bonus "free spin" actually costs 1 coin). The same branch accrues the net-take counter: `DAT_801d3d40 += 6` per normal spin, `+= 1` per feature spin. The `< 3` not-enough gate runs before the mode check, so even a 1-coin feature spin needs 3 banked. All three paylines play on every spin - the per-reel line masks `DAT_801d3d10/14/18` are match bookkeeping the win eval and the reach scanner fill at stop time, not player bet selections, and `DAT_801d4110` is the cash-out submenu cursor (its only role).

Session entry (**Confirmed**, `FUN_801cec94` - see the init section above): `DAT_801d4114 = _DAT_800845A4`, with the 70-coin dev fallback when the battle-return flag is clear. So the bank round-trips by assignment on both ends: copied in at entry, assigned back at the state-`100` commit.

## RAM state

All overlay-local; the block clusters in `0x801d3c80..0x801d4140`. **Confirmed** addresses (roles from the disassembly):

| Global | Role |
|---|---|
| `DAT_801d3c80` | slot LCG state (`FUN_801d30cc`; seeded `0x6C0A2AF0` by the init entry) |
| `DAT_801d3c84` | **state word** (reel state machine dispatch) |
| `DAT_801d3c8c` | winning payline index (for the highlight) |
| `DAT_801d3c90` | spin timer / guaranteed-hit countdown |
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
| `DAT_801d3d2c` | reels-stopped count (0..3) |
| `DAT_801d3d30` | feature/bonus-round active flag |
| `DAT_801d3d34` | winning symbol id (`-1` = no win) |
| `DAT_801d3d38`/`DAT_801d3d3c` | this-spin payout (live + latched for display) |
| `DAT_801d3d40` | net-take heat counter: `+6`/`+1` per spin, minus bonus payouts; the feature-odds bracket input |
| `DAT_801d3d50..` (3 × 0x14) | display reel strips (win eval + render) |
| `DAT_801d3e90..` / `DAT_801d3fd0..` | the two source reel strips (3 × 0x14 each) |
| `DAT_801d3790` | richer-odds flag (widens feature denominators) |
| `DAT_801d3794` | flash/whiteout intensity |
| `DAT_801d3798` | "bonus just ended" flag |
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
| `FUN_801cfff0` | per-frame HUD/balance + payout-digit renderer - `overlay_slot_machine_801cfff0.txt` |
| `FUN_801d30cc` | slot LCG RNG (`x*5+1`, 16-bit fold) - `overlay_slot_machine_801d30cc.txt` |
| `FUN_801d258c` | per-spin feature roll (BIOS-rand, net-take-bracketed odds) - `overlay_slot_machine_801d258c.txt` |
| `FUN_801d2114` | per-reel stop: choose target symbol + depth by feature mode - `overlay_slot_machine_801d2114.txt` |
| `FUN_801d2440` | reel landing search (find target symbol within depth, else next row) - `overlay_slot_machine_801d2440.txt` |
| `FUN_801d13e8` | win evaluation + payout-table lookup + bonus trigger - `overlay_slot_machine_801d13e8.txt` |
| `FUN_801d1af4` | bonus-symbol "reach" scanner (presentation only) - `overlay_slot_machine_801d1af4.txt` |
| `FUN_801d2cc0` | HUD widget sprite-quad rasteriser (3-record descriptor table `DAT_801d347c`) - `overlay_slot_machine_801d2cc0.txt` |
| `FUN_801d0fa8` | reel-column renderer: arithmetic symbol UVs, per-symbol row-490/491 CLUTs, distance shading - `overlay_slot_machine_801d0fa8.txt` |
| `FUN_801e6f70` | coin HUD render: reads `_DAT_800845A4` + record - `overlay_slot_machine_801e6f70.txt` |

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
- the 20-slot strip permutation (`build_strip`, mod-`0x14` draw + `+0xd`/`+1` probe, symbol `slot/2`; `FUN_801cf0d8` case 0);
- the net-take-bracketed feature roll (`feature_roll`; `FUN_801d258c`, exact draw order + bracket edges);
- the flat spin charge + net-take accrual (3/+6 normal, 1/+1 feature);
- the per-mode stop plan + landing search (`stop_plan` / `land_row`; `FUN_801d2114` / `FUN_801d2440`);
- the payline / payout / bonus-round evaluation with the bonus product subtracted from the net take (`SlotMachine::evaluate_spin`, payout via [`legaia_asset::slot_payout`]; `FUN_801d13e8`);
- the entry constants (`ENTRY_DEFAULT_BALANCE` = 70, `ENTRY_LCG_SEED` = `0x6C0A2AF0`; `FUN_801cec94`);
- the coin economy (balance seeded from the bank, `9999999` tally cap, cash-out **assignment** back into the bank).

The engine-side reconstructions (each marked at its site): the spin-up pacing constants, the BIOS-`rand` stream substituted with a deterministic LCG, feature modes 3/5 folded to the normal landing plan, and the bonus product computed as `symbol + 1` over the normal strip in place of the bonus strip's `value - 0xf`.

Runtime wiring: a suspending scene mode (`SceneMode::SlotMachine`; `World::enter_slot_machine` / `tick_slot_machine` / `exit_slot_machine`, which performs the state-100 bank commit into `World::casino_coins` = `_DAT_800845A4`). The `play-window` viewer starts it from the `O` key (loads PROT 0975, `slot_payout::parse`); Cross spins / stops / collects. Disc-gated `slot_minigame_real` drives real-table spins through the World pad path.

## Open

- The **reel-art texture source**: which scene-pack TIM fills texpages `0x0C`/`0x0D` and the per-symbol CLUT rows 490/491 that `FUN_801d0fa8` samples is not traced (needed only if the engine ever draws the real reel art instead of symbol-id placeholders).

## See also

**Reference** -
[Tile-board grid](tile-board.md) ·
[Cheats](../reference/cheats.md) ·
[Casino prize exchange (randomizer)](../tooling/randomizer.md)
