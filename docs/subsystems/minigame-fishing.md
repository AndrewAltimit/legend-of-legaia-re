# Fishing minigame

The fishing minigame is one mode of the shared minigame-hub overlay (the same binary that hosts the slot machine, Baka Fighter, and the dance game). The fishing-specific code occupies the lower address band of that overlay (roughly `0x801cf000`..`0x801d8000`); the higher band re-uses the shared field / actor / move VMs documented elsewhere ([`move-vm.md`](move-vm.md), [`actor-vm.md`](actor-vm.md), [`script-vm.md`](script-vm.md)) and is not redescribed here. Each frame the minigame ticks a small numeric-keyed state machine that walks the player through rod selection, casting, waiting, reeling against a per-fish AI, and the catch / score payout; the persistent fishing-point score lives in the save block and survives between sessions.

The per-frame driver is `FUN_801cf3bc` (`overlay_fishing_801cf3bc.txt`). It is dispatched indirectly as the active mode handler (no static caller inside the overlay dump), in the same "mode handler reached by an indirect dispatch table" pattern as the other minigame and field modes.

## State machine

`FUN_801cf3bc` switches on the mode-state word `DAT_801d926c` through a jump table, then runs a shared tail (`LAB_801d01a4`) that drives auxiliary animation timers, the HUD, and the global "press confirm to leave" check. The state values are sparse (the designers left gaps), and many states `+1` to advance to the next. Confirmed states:

| State | Role |
|---|---|
| `0` | Rod / type select: queues a small menu, reads a select edge, and on confirm grants the inventory rod + lure items (`func_0x800421d4` ids `0x9d`..`0xa2` - the SCUS item table names them Light/Normal/Heavy Lure + Old/Deluxe/Legendary Rod) and advances to `1`. |
| `1` | Scene / actor setup: spawns the fishing actors (`func_0x80020de0`), picks the location variant from `DAT_801d90d0`, initialises camera-tint bytes, then falls through to `0x32`. |
| `0x32` | Sets state to `10` (the run-loop entry). |
| `10` (`0xa`) | Run-loop init: zeroes the per-cast working set, including tension `DAT_801d9168`, depth/line `DAT_801d9298`, casting-power `DAT_801d9274` (seeded `0x40`) and its direction `DAT_801d9278`, then advances. |
| `0xb` | Fade-in: ramps the screen-fade level `DAT_801d905c` down to 0, then advances (or jumps to the "no rod" state `0x96` if `FUN_801d712c` reports no rod owned). |
| `0xc` | Idle / "press to cast": a button edge starts the cast (sets a sound cue and advances) or opens the shop branch (`0x64`). |
| `0xd` | Cast wind-up: advances a small counter, pans the camera, and after ~12 frames jumps to the casting-power state `0x14`. |
| `0x14` | Casting-power oscillator: bounces `DAT_801d9274` between `0x20` and `0x1000` (direction `DAT_801d9278`); on a button edge it locks the power, spawns the lure / line actors, and computes the line-projection vector from the locked power. |
| `0x19` | Transient hold state (sets the "allow leave" flag only). |
| `0x1e` | Lure-travel settle: waits for the line animation counter `DAT_801d91ac` to reach `0x14`, then jumps to `0x20`. |
| `0x20`..`0x22` | Lure-landing / line-sink sequence (camera + line-actor position setup), each advancing to the next. |
| `0x28` | Auxiliary animation wait keyed on `DAT_801d9164`; returns to `10` when the helper `FUN_801d7528` completes. |
| `0x2d` | Miss / retry bookkeeping (`DAT_801d9268` countdown via `FUN_801d6f10`), then back to `0x32`. |
| `0x64`..`0x66` | Shop / point-exchange branch: confirm prompts (`FUN_801d72a0`) gating the buy / sell helpers `FUN_801d06c8` / `FUN_801d092c` / `FUN_801d0c3c`. |
| `0x6e`,`0x78`..`0x7a` | Shop sub-flows that call the same buy / sell helpers and the picker `FUN_801d0474`. |
| `0x96` | "You have lost the lure" / no-rod end screen; a button edge advances to `200`. |
| `200` (`0xc8`) | Exit / fade-out: ramps a fade value to white and, once full, plays the leaving XA cue and tears the mode down. |

The shared tail also services three auxiliary one-shot animation timers (`DAT_801d9160` / `DAT_801d915c` / `DAT_801d90f0`, each advanced through `FUN_801d78ec` / `FUN_801d75dc` / `FUN_801d71d4`), applies the screen fade, draws the persistent HUD (`FUN_801d13f0`) and - while a fish is hooked (`DAT_801d9058`) - the catch HUD (`FUN_801d1580`), and honours a global "confirm-to-leave" edge that returns to state `10`.

The reeling / fish-AI tick `FUN_801d4004` and the per-fish actor handler `FUN_801d26cc` run from the actor side of the run loop (`FUN_801d26cc` calls `FUN_801d4004` while the fish is engaged), not directly from the mode switch.

## Tension / reeling mechanic

The hooked-fight is a tug-of-war between the player's reel input and the fish's pull, mediated by the tension gauge `DAT_801d9168` (range `0`..`0x1000`). The whole update lives in `FUN_801d4004` (`overlay_fishing_801d4004.txt`); the gauge math at its tail is:

- **Reel held** (`_DAT_8007b850 & 0x40` or `& 0x80`, the held-pad reel buttons): tension *increases* by a per-frame step derived from a base pull, divided by a rod-dependent divisor (`_DAT_80084454 * 9 + 0x23` for one button, `* 6 + 0x19` for the other) and scaled by the frame-step `DAT_1f800393`. Holding reel also nudges the line / depth value `DAT_801d9298` down by a small per-state amount.
- **Reel released** (`(_DAT_8007b850 & 0xc0) == 0`): tension *decreases* by `(_DAT_80084454 * 0x40 + 0x4a) * DAT_1f800393`.
- The gauge is then clamped to `[0, 0x1000]`. (Confirmed: clamp at `0x1000` high, `0` low.)

`_DAT_80084454` is a persistent rod / upgrade stat read from the save block; a higher value softens the per-frame tension change. The fish's own behaviour is a sub-state machine on `DAT_801d910c` (run / dart-left / dart-right / dive, selected pseudo-randomly via the BIOS `rand` `func_0x80056798`), which moves the fish actor and modulates the pull; the timer `DAT_801d9110` counts down each behaviour and re-rolls the next. Per-fish parameters (pull magnitudes, dart push, behaviour-selection cutoffs, scoring value) come from the per-species table documented in [Per-species parameter table](#per-species-parameter-table) below, indexed by `DAT_801d91cc * 0x28` based at `&DAT_801d81a4`.

The catch HUD `FUN_801d1580` (`overlay_fishing_801d1580.txt`) renders the live state: the line length / record number `DAT_801d927c`, the casting-power bar `DAT_801d9274`, the depth `DAT_801d9298`, and - gated on `DAT_801d91b4` - the tension bar `DAT_801d9168` itself (drawn via `FUN_801d1870`). It uses the digit / glyph blitters `FUN_801d76e0` (number) and `FUN_801d63b0` (single sprite-quad).

A landed catch is resolved in `FUN_801d5298` (`overlay_fishing_801d5298.txt`). The awarded points are
`points = (fish_base_value * (DAT_801d91b8 + 0x9c0)) / 0x32000`,
where `fish_base_value` is the species record's `+0x04` field (`&DAT_801d81a8 + DAT_801d91cc*0x28`) and `DAT_801d91b8` is the accumulated pull / strength for the fight. The points are added to the persistent counter `_DAT_8008444c` (clamped to `999999`), guarded by a per-catch latch at actor `+0x2a` so a single fish is scored once. If the catch beats the current best (`_DAT_80084458`), the best value and its fish id (`_DAT_8008445c`) are updated.

## Per-species parameter table

The species table is **static `.rodata`** in the fishing overlay (PROT entry 0972, `data\OTHER1`; base `0x801CE818`, table head `0x801D81A4` = file offset `0x998C`). Record `N` lives at `0x801D81A4 + N*0x28`; the decompiler resolves the head as `(&PTR_s_Spikefish_801d81a4)[DAT_801d91cc * 10]`, so `+0x00` is a pointer to the fish-name string (also in this overlay). The structure runs for **10 records** (`Spikefish` = id 0 .. the rarest catch = id 9); record 10's `+0x00` is no longer an in-overlay pointer, which bounds the table.

Each record is 10 words (stride `0x28`). Every field has a *confirmed reader* in `FUN_801d4004` (fish-AI tick) or `FUN_801d5298` (scoring); the designer-level meaning is the consuming formula:

| Off | Field | Consuming site / formula |
|---|---|---|
| `+0x00` | name pointer | `FUN_801d4004` - the hooked-fish name banner |
| `+0x04` | score base value | `FUN_801d5298` - `points = value * (strength + 0x9c0) / 0x32000` |
| `+0x08` | pull factor | `FUN_801d4004` - per-frame pull `((rand & 0xff) + bias) * f / 150` (also a `/0xc8000` term) |
| `+0x0c` | dart push factor | `FUN_801d4004` - dart-state lateral push `((step >> 2) + 0x20) * f / 100` |
| `+0x10` | depth-sink factor | `FUN_801d4004` - run-state line-sink `(pull * f) / 150` |
| `+0x14` | depth gate | `FUN_801d4004` - behaviour pick when `f < line-depth` |
| `+0x18` | behaviour-roll cutoff A | `FUN_801d4004` - `f <= rand & 0xfff` |
| `+0x1c` | behaviour-roll cutoff B | `FUN_801d4004` - `rand & 0xfff < f` |
| `+0x20` | behaviour-roll cutoff C | `FUN_801d4004` - `rand & 0xfff < f` |
| `+0x24` | strike / record gate | `FUN_801d4004` - hook check `record < f + 300` |

The `+0x04` score value and `+0x08` pull factor both climb monotonically with rarity (the rarest catch carries the largest of each), so a higher-value fish is also the harder fight. Parser: [`legaia_asset::fishing_species`] (`parse` decodes the 10 records from the overlay image; `FishingSpecies::score_for` reproduces the award formula; `name` resolves the `+0x00` pointer). No Sony bytes are committed - the values + names decode from the user's disc (disc-gated `fishing_species_real`).

## RAM state

Fishing-specific globals (overlay-resident unless noted; `_DAT_8008xxxx` live in the persistent save block):

| Address | Type | Meaning |
|---|---|---|
| `0x801d926c` | `u32` | Mode-state word for `FUN_801cf3bc` (the values in the table above). |
| `0x801d9168` | `s32` | **Tension gauge**, `0`..`0x1000`. Raised by held reel input, lowered when released. |
| `0x801d9274` | `s32` | Casting-power meter; oscillates `0x20`..`0x1000` in state `0x14` and is locked on cast. |
| `0x801d9278` | `s32` | Casting-power oscillation direction (`+1` / `-1`). |
| `0x801d9298` | `s32` | Line depth / sink value during the fight (clamped against the cast power). |
| `0x801d91cc` | `u32` | Hooked-fish species id; indexes the per-species table at `&DAT_801d81a8` (stride `0x28`). |
| `0x801d910c` | `u32` | Fish behaviour sub-state (run / dart / dive). |
| `0x801d9110` | `s32` | Frame countdown until the next fish-behaviour re-roll. |
| `0x801d91b8` | `s32` | Accumulated pull / strength for the current fight; feeds the score formula. |
| `0x801d927c` | `s32` | Line length / catch record value shown on the HUD. |
| `0x801d9058` | `u32` | "Fish hooked" flag; gates the catch HUD (`FUN_801d1580`). |
| `0x801d905c` | `s32` | Screen-fade level (down-ramped on fade-in, up-ramped on exit). |
| `0x801d90d0` | `u32` | Fishing-location variant selected at setup. |
| `0x8008444c` | `s32` | **Persistent fishing-point score** (save block), capped at `999999`. |
| `0x80084450` | `u32` | Persistent selected-rod index (HUD label + SFX base). |
| `0x80084454` | `s32` | Persistent rod / upgrade stat; scales the per-frame tension change. |
| `0x80084458` | `s32` | Persistent best-catch point value. |
| `0x8008445c` | `u32` | Persistent best-catch fish id. |

(Pad-input globals `_DAT_8007b850` held-mask and `_DAT_8007b874` edge-mask, and the frame-step `DAT_1f800393`, are the shared field-VM globals; see [`field-locomotion.md`](field-locomotion.md) / [`script-vm.md`](script-vm.md).)

## Key functions

- `FUN_801cf3bc` (`overlay_fishing_801cf3bc.txt`) - per-frame fishing mode driver; the `DAT_801d926c` state machine plus the HUD / fade / leave tail.
- `FUN_801d4004` (`overlay_fishing_801d4004.txt`) - fish-AI + tension-gauge tick: reel-input integration into `DAT_801d9168`, the `DAT_801d910c` behaviour sub-state, and the next-behaviour roll.
- `FUN_801d26cc` (`overlay_fishing_801d26cc.txt`) - hooked-fish actor handler; positions the fish / lure / line actors and calls `FUN_801d4004` while engaged.
- `FUN_801d5298` (`overlay_fishing_801d5298.txt`) - catch resolution + scoring: computes the point award, credits `_DAT_8008444c`, and updates the best-catch record.
- `FUN_801d1580` (`overlay_fishing_801d1580.txt`) - catch HUD: draws tension, casting power, depth, and record values.
- `FUN_801d13f0` (`overlay_fishing_801d13f0.txt`) - persistent HUD: draws the fishing-point total (`_DAT_8008444c`, capped) and the rod-type label.
- `FUN_801d712c` (`overlay_fishing_801d712c.txt`) - rod-ownership gate; queries inventory item ids `0x9d`..`0x9f` (`func_0x80042f4c`).

Parser: [`legaia_asset::fishing_species`](../../crates/asset/src/fishing_species.rs) decodes the [per-species table](#per-species-parameter-table) from the disc.

Engine port: [`legaia_engine_core::fishing`](../../crates/engine-core/src/fishing.rs) is the clean-room rules engine over that table. The **Confirmed** numeric kernels are ported directly: the casting-power oscillator (`CastPower`, bounds `0x20..=0x1000`, seed `0x40`; `FUN_801cf3bc` state `0x14`), the tension-gauge tug-of-war (`TensionGauge`, reel divisors `rod*9+0x23` / `rod*6+0x19`, release `(rod*0x40+0x4a)*frame_step`, clamp `[0, 0x1000]`; `FUN_801d4004`), and the catch award + persistent-record credit (`FishingRecord`, `value*(strength+0x9c0)/0x32000`, `999999` cap, best-catch; `FUN_801d5298`).

The `FishingSession` composes those kernels into a cast → fight → score loop. The win/lose glue (line-snaps-at-max-tension, reel-progress land, the locked-cast species pick, and the steady per-frame fish pull) is an **engine-side reconstruction** of the [Open](#open) items below and is marked as such at each call site - no Sony bytes are baked in.

Runtime wiring: installed as a suspending scene mode (`SceneMode::Fishing`; `World::enter_fishing` / `tick_fishing` / `exit_fishing`). The `play-window` viewer starts it from the `L` key (loads the fishing overlay PROT 0972, `fishing_species::parse`); Cross locks the cast and reels (reel A), Circle is reel B, and the HUD shows the cast-power / tension / catch-result line plus the running point total. `P` opens the [point exchange](#point-exchange-prize-shop) (Up/Down move, Left/Right switch venue, Enter trades).

## Point exchange (prize shop)

The shop branch of the mode SM (states `0x64`..`0x7a`) is a **point exchange**: it spends the persistent fishing-point pool `_DAT_8008444C` on in-game items. The screens are:

- `FUN_801d0c3c` (state `0x64` family) - the 6-row prize list. Each row prints its item name through the MES `0xC2` item-name token fed with the record's item id, plus the per-unit price; the running point total renders capped at `999999`. **Row 0 is hidden until strictly affordable** (`price0 < points` - the cursor floor is `(price0 < points) ^ 1`), which is why each venue's top prize only "appears" once the pool is big enough. Row availability (white vs grey, `FUN_801d6f90`): affordable, inventory count `!= 99`, and - for a one-time row - its purchased bit not yet latched.
- `FUN_801d092c` (state `0x7a`) - the "Trade how many?" quantity picker. Max quantity = `min(points / price, limit − owned)` where `owned` is the live inventory count (`func_0x80042f4c`) and a not-yet-purchased one-time row treats `owned` as 0.
- `FUN_801d06c8` (state `0x79`) - the "Are you sure?" confirm. Yes grants `func_0x800421d4(item_id, qty)`, deducts `price * qty` from `_DAT_8008444C`, and for a `limit == 1` row latches bit `row + venue*8` of the persistent purchased bitmask `_DAT_8008446C`.

**Record layout** (12-byte stride, 6 rows per venue, read through `PTR_DAT_801d90b8`):

| Off | Field | Meaning |
|---|---|---|
| `+0x00` | `limit` | Max obtainable count: `1` = one-time prize (latched in `_DAT_8008446C`), `99` = repeatable |
| `+0x04` | `price` | Fishing points per unit |
| `+0x08` | `item_id` | Granted item id (SCUS item-name-table space) |

**Venue pages.** Two consecutive 6-row tables live in the overlay rodata at VA `0x801D8088` / `0x801D80D0`; `FUN_801cf3bc` state `1` selects the page from the venue global `_DAT_8007BAC4` (`0x187` → page 0, the Buma pond; `0xF4` → page 1, the Vidna pond - the selector values equal the Karisto / Sebucus kingdom-bundle extraction indices). Both venues spend and latch against the same globals; venue 1's one-time bits occupy `8..`. Cross-validated row-for-row against the curated walkthrough prize lists ([`gamedata.md`](../reference/gamedata.md)) - including one entry the walkthroughs miss: **Vidna's row 0 is a 50,000-point one-time War God Icon**, invisible until the pool exceeds its price.

The same state-1 page select also pages the venue's **species-spawn table** into `PTR_DAT_801d9114` (rodata `0x801D8334` / `0x801D8434`, directly after the species table): `8 × 8` u32 species ids read by the hooked-fish handler as `species = table[rod*8 + band]` (`FUN_801d26cc`), where `rod` is the equipped-rod index `_DAT_80084450` (rows 3..8 are zero padding - three rods exist) and `band` is the cast band `DAT_801d90e8` (0..4; band 4 is the venue's rare band, entered by a venue-specific roll - 1/16 at Buma with rod 1 + lure 2 after `0x32` even-count catches, 1/4 at Vidna with rod 2 + lure 2 - or directly on a deep cast).

Parsers: [`legaia_asset::fishing_exchange`](../../crates/asset/src/fishing_exchange.rs) (exchange pages) and `fishing_species::parse_spawn_tables` (spawn pages); disc-gated `fishing_exchange_real` pins the structural invariants. Engine port: `legaia_engine_core::fishing::PrizeExchange` (list-floor / availability / quantity-cap / confirm kernels) with the grant committed by `World::fishing_exchange_buy` against the persistent `World::fishing_points` pool + `World::fishing_prizes_purchased` mask (the retail `_DAT_8008444C` / `_DAT_8008446C` pair); disc-free runtime oracle `fishing_exchange_runtime`.

## Open

- The exact bit assignment of the reel buttons within `_DAT_8007b850` (which physical face/shoulder buttons `0x40` / `0x80` map to) is not pinned from these dumps.
