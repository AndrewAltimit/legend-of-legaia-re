# Casino slot machine

The casino's slot-machine minigame: three reels of pictographic symbols, a 1-, 3- or 5-line bet selector, a per-spin win evaluation against a payout table, and a running coin balance the player can cash back out to the casino coin bank. It lives in the minigame-hub overlay (the binary shared with fishing / Baka Fighter / dance), so the locomotion, sprite, actor-VM and SDK helpers it leans on are documented elsewhere - this page covers only the slot-specific logic.

**This is the slot *gameplay*, not the prize exchange.** Cashing casino coins for items is a separate static table (`DAT_801e4518` / PROT 899, debiting `_DAT_800845A4`'s sibling coin counter) covered by the randomizer's `casino::CasinoExchange`. The slot machine pays out *into* the coin balance; the exchange spends it.

Provenance: the dumps are `ghidra/scripts/funcs/overlay_slot_machine_<addr>.txt`. Confidence is marked per-claim; the reel layout, RNG, payout lookup and coin commit are **Confirmed** from the disassembly, the symbol-art and payout-byte table *values* are **Inferred** (read from a data region this overlay indexes, not reproduced here).

## Reel state machine

The machine is a single per-frame handler, `FUN_801cf0d8` (`overlay_slot_machine_801cf0d8.txt`), dispatched on the state word `DAT_801d3c84` through a jump table (the overlay-resident table just below `0x801d2ac0`; the `< 0x65` bound on the index is in the dispatcher prologue). **Confirmed** states:

| `DAT_801d3c84` | Role |
|---|---|
| `0` | **init**: reseed (`func_0x80056798`), build the three reel strips, clone them into the display copy, fade in |
| `1` | **attract / idle**: wait for input; pressing a face button (`_DAT_8007b874 & 0xe0`) charges the bet and advances to spin if enough coins are banked; the menu/select edge (`& 0x110`) routes to state `0x32` (cash-out) |
| `2` | **spin-up**: ramp the three reel velocities (`DAT_801d3cd0..` added into the reel positions `DAT_801d3cc0..` each frame, wrapping mod `0x1400`) until the spin timer `DAT_801d3c90` expires |
| `3` | **stopping**: each Stop input (pad bits `0x80`/`0x40`/`0x20` → reels 0/1/2) calls `FUN_801d2114` to choose where that reel lands; once all three reels are stopped (`DAT_801d3d2c == 3`) it runs `FUN_801d13e8` (win eval) and advances to `4` |
| `4` | **payout tally**: animates the credited win (`DAT_801d3d38`) ticking from the win counter into the player balance `DAT_801d4114`; on completion returns to `1` |
| `0x32`..`0x39` | **cash-out / quit submenu**: a 3-option picker (`DAT_801d4110 % 3`) with fade-in/out states; option `1` commits the balance (route to state `100`), the others return to play or leave |
| `0x5a` | **not-enough-coins** prompt (reached from state `1` when `DAT_801d4114 < 3`) |
| `100` | **commit + exit**: fades out, writes `_DAT_800845A4 = DAT_801d4114`, returns to the casino field |

The tail of `FUN_801cf0d8` (after the switch) always advances the three reel positions, redraws the visible symbols via `FUN_801d0fa8`, refreshes the marquee/HUD, and - when the attract flag `_DAT_8007b9b0` is set - reads the bet-line buttons into `DAT_801d3cac`'s sibling line count.

### Reel strips

Each of the 3 reels is a 20-symbol (`0x14`) strip. Two parallel strip arrays exist - `DAT_801d3e90` and `DAT_801d3fd0`, each `3 × 0x14` ints at `0x50` stride - plus a display copy at `DAT_801d3d50` (the array win-eval and the renderer read). Init (`FUN_801cf0d8` case 0) fills a strip by, for each of the 20 slots, drawing a fresh RNG value, reducing it mod `0x14`, and probing forward (`+0xd` step for one array, `+1` for the other) until it finds an unused slot - a collision-resolving permutation that scatters each symbol id `slot/2` (so symbol ids run `0..9`, two strip positions each) around the reel. **Confirmed** from `overlay_slot_machine_801cf0d8.txt` (the `% 0x14` / `(uVar1 + 0xd) % 0x14` placement loops).

The live reel position `DAT_801d3cc0[reel]` is a fixed-point angle; the on-screen symbol index is `(pos >> 8)` reduced mod `0x14`, and adjacent rows are read at `±1`/`±0x10`/`±0x11` offsets (the three pay rows). The symbol *quad* is drawn by `FUN_801d2cc0` (the sprite-rasteriser the dispatcher reuses for HUD digits too), which indexes a 20-byte-stride descriptor table at `DAT_801d347c` by symbol id.

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

`FUN_801d258c` (`overlay_slot_machine_801d258c.txt`) runs once at spin start. It seeds `DAT_801d4134` (`rand%5` landing jitter) and `DAT_801d3cb8` (`rand%6 + 2` normal-mode target), then - only when no feature is already active (`DAT_801d3cac == 0`) - rolls several `rand % N == 0` probabilities to *enter* a feature mode. The probability denominators are **bucketed by the current balance** `DAT_801d3d40`: tighter odds for a fat balance, looser for a thin one (the `< 1000`, `1001..1999`, `> 2000` brackets with denominators like `700`/`500`, `0x15e`/`0xfa`, `0xaf`/`0x7d`, and a final `+600` roll for mode 3). When the optional richer-odds flag `DAT_801d3790` is set, every denominator is widened by `rand%100 + 200`. **Confirmed** arithmetic;
this is the house-edge / "make the player feel lucky when poor" tuning.

## Payout / win evaluation

After all three reels stop, `FUN_801d13e8` (`overlay_slot_machine_801d13e8.txt`) evaluates the win. **Confirmed** structure:

1. For each of the three diagonal/straight paylines it reads the three on-screen symbols (display strip `DAT_801d3d50` at the per-reel `±` row offsets) and checks all-three-equal. It keeps the **highest-value matching line** (`DAT_801d3d34` = winning symbol id, `DAT_801d3c8c` = winning line index for the highlight).
2. If a line wins, the credited amount is `DAT_801d3d38 = payout_table[winning_symbol]`, read as a byte from a table at `DAT_801d3598` indexed by symbol id (so payout scales with symbol rarity). **Inferred** values (not reproduced).
3. Symbol ids `8` and `9` are the **bonus/jackpot symbols**: matching them sets `DAT_801d3cac` to feature mode `6` and seeds a free-spin / multiplier counter `DAT_801d3cb0` (3 spins for id 9, 1 for id 8), kicking off the bonus round and a celebratory actor (`func_0x800653c8`).
4. During an active feature (`DAT_801d3d30 != 0`) the payout is instead the **product** of the three matched symbols' `(value - 0xf)` factors, the multiplier counter decrements, and the win is added to the round total `DAT_801d3d40`; when the counter hits zero the feature ends.
5. In feature mode 3 a `rand % 0x96 == 0` roll can spontaneously clear the feature.

`FUN_801d1af4` (`overlay_slot_machine_801d1af4.txt`) is the **bonus-symbol scanner** run while reels are still settling (state 3): it sweeps the same three paylines looking specifically for adjacent `8`/`9` symbols under the active-line masks (`DAT_801d3d10`/`14`/`18`) and, on the first sighting, fires the bonus-anticipation effect (`DAT_801d3ca4`/`DAT_801d3ca8` latches + `_DAT_8007b6dc = 0x200` SFX cue). It sets no payout - it only triggers the "reach" presentation. **Confirmed.**

## Coin economy

The credit balance the player accumulates while playing lives in the **overlay-local** word `DAT_801d4114` (capped at `9999999` in the tally path, displayed capped at `99999` by the HUD). It is *not* the casino coin bank. **Confirmed.**

The casino coin bank is the global `_DAT_800845A4` (u32). The slot machine touches it in exactly two places:

- **Read (entry / HUD):** `FUN_801e6f70` (`overlay_slot_machine_801e6f70.txt`) renders the coin HUD; it reads `_DAT_800845A4` (current bank) for the on-screen coin readout and the sibling word `_DAT_8008459C` for a record/high value, comparing them to flag a new record. It does not modify the bank. **Confirmed** (`func_0x80034b78(_DAT_800845A4, …)`).
- **Write (cash-out commit):** state `100` of `FUN_801cf0d8` does `_DAT_800845A4 = DAT_801d4114` once the cash-out fade completes (`overlay_slot_machine_801cf0d8.txt`, the `_DAT_800845a4 = DAT_801d4114` store). So the bank is **assigned the final playing balance on exit**, not debited/credited per spin. **Confirmed.**

This is why the "Infinite Coins" cheat (`0x800845A4 = 0x05F5E0FF`, see [`cheats.md`](../reference/cheats.md)) works at the casino but does **not** make individual spins free: per-spin betting decrements the *overlay-local* `DAT_801d4114` (loaded from the bank when the machine opens). The cheat-database pointer noted "near `0x801d3cac`" lands in this overlay's state block - `DAT_801d3cac` is the **feature mode**, and the surrounding `0x801d3c80..0x801d4134` window holds the RNG seed, reel positions, state word, balance, bet count and payout counters described in the table below. **Confirmed** address window from the disassembly; the specific cheat-pointer semantics are **Inferred**.

Per-spin betting: in state `1` a bet button subtracts the line cost from `DAT_801d4114` and credits the matching line mask; the bet-line count is `DAT_801d4110 % 3` (1/2/3 lines, edited in the cash-out/line submenu). **Confirmed** the subtraction and line selector; the exact coins-per-line constant is read alongside and is **Inferred**.

## RAM state

All overlay-local; the block clusters in `0x801d3c80..0x801d4140`. **Confirmed** addresses (roles from the disassembly):

| Global | Role |
|---|---|
| `DAT_801d3c80` | slot LCG state (`FUN_801d30cc`) |
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
| `DAT_801d3d10`/`14`/`18` | active payline masks |
| `DAT_801d3d2c` | reels-stopped count (0..3) |
| `DAT_801d3d30` | feature/bonus-round active flag |
| `DAT_801d3d34` | winning symbol id (`-1` = no win) |
| `DAT_801d3d38`/`DAT_801d3d3c` | this-spin payout (live + latched for display) |
| `DAT_801d3d40` | round/feature running total |
| `DAT_801d3d50..` (3 × 0x14) | display reel strips (win eval + render) |
| `DAT_801d3e90..` / `DAT_801d3fd0..` | the two source reel strips (3 × 0x14 each) |
| `DAT_801d3790` | richer-odds flag (widens feature denominators) |
| `DAT_801d3794` | flash/whiteout intensity |
| `DAT_801d3798` | "bonus just ended" flag |
| `DAT_801d4110` | bet-line selector (`% 3`) |
| `DAT_801d4114` | **player credit balance** (committed to `_DAT_800845A4` on exit) |
| `DAT_801d4134` | per-spin landing jitter (`rand%5`) |
| `_DAT_800845A4` | global casino **coin bank** (written on cash-out; read by the HUD) |
| `_DAT_8008459C` | coin record / high value (HUD compare) |

Symbol-art and payout tables (overlay rodata; **values** decode from the disc, not reproduced here):

| Table | Role |
|---|---|
| `DAT_801d347c` (0x14 stride) | per-symbol sprite-quad descriptor (UVs / colour), indexed by symbol id in `FUN_801d2cc0`. **Values still open.** |
| `DAT_801d3598` | per-symbol line-payout byte, indexed by winning symbol id `0..=9` in `FUN_801d13e8` (and the HUD preview `FUN_801d2aa4`). **10 entries** (PROT 0975 file offset `0x4D80`), bounded by zero padding + an overlay string at `+0x10`. Parser [`legaia_asset::slot_payout`]. **Confirmed.** |

The payout table is exactly 10 bytes - one per symbol id, the index range `FUN_801d13e8` special-cases for the jackpot symbols 8/9. The normal-line credit is `balance += DAT_801d3598[symbol]`; a bonus round instead multiplies the three matched `(value − 0xf)` factors (the bonus reel strip carries values `0x10..=0x19`, so the factors are `1..=10`) and does not read this table.

## Key functions

| Address | Role |
|---|---|
| `FUN_801cf0d8` | slot-machine per-frame state machine (the real reel dispatcher) - `overlay_slot_machine_801cf0d8.txt` |
| `FUN_801cfff0` | per-frame HUD/balance + bet-line + payout-digit renderer - `overlay_slot_machine_801cfff0.txt` |
| `FUN_801d30cc` | slot LCG RNG (`x*5+1`, 16-bit fold) - `overlay_slot_machine_801d30cc.txt` |
| `FUN_801d258c` | per-spin feature roll (BIOS-rand, balance-bracketed odds) - `overlay_slot_machine_801d258c.txt` |
| `FUN_801d2114` | per-reel stop: choose target symbol + depth by feature mode - `overlay_slot_machine_801d2114.txt` |
| `FUN_801d2440` | reel landing search (find target symbol within depth, else next row) - `overlay_slot_machine_801d2440.txt` |
| `FUN_801d13e8` | win evaluation + payout-table lookup + bonus trigger - `overlay_slot_machine_801d13e8.txt` |
| `FUN_801d1af4` | bonus-symbol "reach" scanner (presentation only) - `overlay_slot_machine_801d1af4.txt` |
| `FUN_801d2cc0` | symbol/HUD sprite-quad rasteriser (descriptor table `DAT_801d347c`) - `overlay_slot_machine_801d2cc0.txt` |
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

[`legaia_engine_core::slot_machine`](../../crates/engine-core/src/slot_machine.rs) is the clean-room rules engine over this page. The **Confirmed** kernels are ported directly: the slot LCG (`SlotRng`, `x*5+1` + 16-bit fold; `FUN_801d30cc`), the 20-slot strip permutation (`build_strip`, mod-`0x14` draw + `+0xd`/`+1` probe, symbol `slot/2`; `FUN_801cf0d8` case 0), the balance-bracketed feature roll (`feature_roll`; `FUN_801d258c`), the per-mode stop plan + landing search (`stop_plan` / `land_row`; `FUN_801d2114` / `FUN_801d2440`), the payline / payout / bonus-round evaluation (`SlotMachine::evaluate_spin`, payout via [`legaia_asset::slot_payout`]; `FUN_801d13e8`), and the coin economy (overlay-local balance, `9999999` tally cap, cash-out **assignment** into the bank).

The engine-side reconstructions (each marked at its site): the 1-coin-per-line bet constant, the bracket→denominator pairing inside the feature roll, the spin-up pacing constants, the BIOS-`rand` stream substituted with a deterministic LCG, and feature modes 3/5 folded to the normal landing plan.

Runtime wiring: a suspending scene mode (`SceneMode::SlotMachine`; `World::enter_slot_machine` / `tick_slot_machine` / `exit_slot_machine`, which performs the state-100 bank commit into `World::casino_coins` = `_DAT_800845A4`). The `play-window` viewer starts it from the `O` key (loads PROT 0975, `slot_payout::parse`); Cross spins / stops / collects, Left/Right cycle the bet lines. Disc-gated `slot_minigame_real` drives real-table spins through the World pad path.

## Open

- The **symbol-descriptor table** at `DAT_801d347c` (sprite UVs / colour per symbol) is read by `FUN_801d2cc0` but its layout/values are not lifted - the head records look `0x14`-stride but the region transitions into a pointer array a few entries in, so the exact extent needs more tracing. (The sibling **payout-byte table** at `DAT_801d3598` is now decoded - see [RAM state](#ram-state) / `legaia_asset::slot_payout`.)
- The exact **coins-per-line bet cost** is read in state `1` next to the balance subtraction; the constant is not pinned.
- Whether the casino field entry point seeds `DAT_801d4114` from `_DAT_800845A4` on open (the symmetric counterpart of the state-`100` commit) - implied by the cash-out being an assignment, but the seed store was not located in the dumps reviewed.

## See also

**Reference** -
[Tile-board grid](tile-board.md) ·
[Cheats](../reference/cheats.md) ·
[Casino prize exchange (randomizer)](../tooling/randomizer.md)
