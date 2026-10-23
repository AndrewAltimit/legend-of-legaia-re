# legaia-minigame-formats

The minigames' disc data. The rules engines that consume it live in
[`legaia-engine-minigames`](../engine-minigames/README.md) and
[`legaia-engine-fishing`](../engine-fishing/README.md).

| Minigame | Modules |
|---|---|
| Fishing | `fishing_species`, `fishing_exchange`, `fishing_sprites`, `fishing_captions`. See [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md). |
| Slot machine | `slot_payout`, `minigame_slot_scene` (the 3D scene). See [`minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md). |
| Dance | `dance_chart`, `dance_cast`, `dance_art`. See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md). |
| Baka Fighter | `baka_opponents`. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| Muscle Dome | `muscle_dome`, `ringside_still` ([`ringside-still.md`](../../docs/formats/ringside-still.md)). See [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md). |
| Shared | `minigame_art` (the art packs the overlays upload), `minigame_sfx` (the runtime cue bank). |

## Layering

Sits on [`legaia-container-formats`](../container-formats/README.md) and the
leaf codecs (`legaia-lzs`, `legaia-prot`, `legaia-tim`, `legaia-tmd`,
`legaia-vab`). It must not depend on the scene, UI or actor format
crates, nor on `legaia-disc-audit`.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `ringside_still` | The two headerless 16bpp stills (extraction 1221 / 1222): `0x28000` bytes of raw BGR555 uploaded as four 320x64 bands to VRAM `(384, 0)`. The rectangle is immediates in `FUN_801F6B24`, not a field, so the module carries the layout the file does not state. See [`ringside-still.md`](../../docs/formats/ringside-still.md). |
| `dance_art` / `dance_cast` | Dance-minigame presentation art (PROT 1230 TIM pack, HUD widget table, face-stamp rig) and cast / choreography tables. |
| `fishing_captions` / `fishing_sprites` | The fishing HUD's lure-row caption strings (`FUN_801D13F0`) and its 20-byte HUD sprite table drawn by the shared quad emitter `FUN_801D63B0` (PROT 0972). |
| `minigame_art` / `minigame_sfx` | Minigame texture packs and the runtime SFX bank the minigame overlays fire into. |

### `fishing_exchange`

Fishing-minigame **point-exchange (prize shop) tables**: two per-venue 6-row
`0xC`-stride tables (`[limit, price, item_id]`; Buma page VA `0x801D8088`,
Vidna `0x801D80D0`) the exchange sub-screens read through `PTR_DAT_801d90b8`
(list `FUN_801d0c3c`, quantity `FUN_801d092c`, confirm `FUN_801d06c8`,
availability `FUN_801d6f90`). Prices spend the persistent point pool
`_DAT_8008444C`; a `limit == 1` row latches bit `row + venue*8` of
`_DAT_8008446C`; row 0 hides until strictly affordable.

- `parse` → both venue pages of `ExchangeRow`s;
  `FishingExchange::purchase_bit(page, row)` maps the one-time bit index.
  No Sony bytes committed (disc-gated `fishing_exchange_real`). See
  [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md#point-exchange-prize-shop).

### `fishing_species`

Fishing-minigame **per-species parameter table** (overlay VA `0x801D81A4`), the
10-record `0x28`-stride table the fish-AI tick `FUN_801d4004` and catch-scoring
`FUN_801d5298` index by the hooked-fish id `DAT_801d91cc`. Static `.rodata` in
the fishing overlay (PROT 0972, base `0x801CE818`); the `+0x00` head is a fish-name
pointer into the same overlay.

- `parse` → 10 `FishingSpecies` records (`+0x04` score base, `+0x08` pull, `+0x0c`
  dart, `+0x10` sink, `+0x14` depth gate, `+0x18`/`+0x1c`/`+0x20` behaviour-roll
  cutoffs, `+0x24` strike gate).
- `FishingSpecies::score_for(strength)` reproduces the award formula
  (`value * (strength + 0x9c0) / 0x32000`); `name(overlay)` resolves the `+0x00`
  pointer. No Sony bytes committed (disc-gated `fishing_species_real`). See
  [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md#per-species-parameter-table).
- `parse_spawn_tables` → the two per-venue species-spawn tables directly after
  the species table (`0x801D8334` / `0x801D8434`): `8 × 8` u32 species ids the
  hooked-fish handler picks as `table[rod*8 + cast_band]` (`FUN_801d26cc`).

### `dance_chart`

Noa dance-minigame **step chart** (overlay VA `0x801D509C`), the `3 × 0x20`-byte
direction-symbol grid the hit-judge `FUN_801d1960` indexes by
`chart[lane*0x20 + beat]` (lane = groove gauge `/ 1000` ∈ {0,1,2}, beat =
beat-clock `/ 0x119`). Baked into the dance overlay (PROT 0980, base
`0x801CE818`, file offset `0x6884`) - not loaded per song.

- `parse` → a [`DanceChart`] of 3 rows × 32 beats (symbol `0` none, `1`/`2`/`3`
  judged directions). `symbol(lane, beat)` / `step_count(lane)` (row density
  rises with difficulty). No Sony bytes committed (disc-gated `dance_chart_real`).
  See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md).

### `slot_payout`

Casino slot-machine **per-symbol payout table** (overlay VA `0x801D3598`), the
10-byte table the win-evaluator `FUN_801d13e8` indexes by the winning symbol id
(`0..=9`) to credit a line (`balance += DAT_801d3598[symbol]`). Baked into the
slot overlay (PROT 0975, base `0x801CE818`, file offset `0x4D80`), bounded by
zero padding + an overlay string at `+0x10`.

- `parse` → a [`SlotPayoutTable`] of 10 bytes; `payout(symbol)` /
  `is_bonus_symbol(id)` (ids 8/9 trigger the bonus round). No Sony bytes
  committed (disc-gated `slot_payout_real`). See
  [`minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md).

### `minigame_slot_scene`

The slot machine's **3D scene graph**. The machine is not a sprite collage: every
element on its face is a quad projected through the GTE and depth-sorted into the
ordering table, and the overlay reaches the GTE entirely through SCUS wrappers, so
it carries no `cop2` op of its own.

- The four geometry tables tile contiguously at PROT 0975 file `0x4E68..0x4F38`:
  the 5 paylines (`FUN_801d3380` - three straight, two diagonal), the 5 payline
  medallions, the 5 payline lamps, and the marquee panel + two mascots
  (`FUN_801d08e4`). `parse_scene` decodes all four plus the **21-bitmap
  dot-matrix message bank** (file `0x4CD8`), cut out of art page 3.
- The reel cylinder (`FUN_801d0fa8`): `reel_y` / `reel_z` (SCUS trig tables,
  radius 585 x 512), `reel_shade` (the depth cue that peaks on the payline face
  and blacks out the near half - there is no backface cull).
- `project` / `billboard_half`: the screen mapping onto the retail 640x240
  framebuffer. Its shape is derived from the camera matrix `diag(6, 3, 3)`; its
  four scalars are *fitted* to a retail framebuffer (the GTE control words are in
  COP2, not main RAM), solved on the five payline lamps and then verified by
  predicting every other element's rect. No Sony bytes committed (disc-gated
  `the_slot_machines_3d_scene_decodes_and_projects_onto_the_retail_frame` in
  `crates/web-minigames/tests/minigames_wasm_api.rs`).

### `baka_opponents`

Baka Fighter **per-opponent table** (overlay VA `0x801D76BC`), the 17-record
`0x6c`-stride table the match code indexes by opponent id. Each record carries
the `+0x00` gold reward (`FUN_801d0fe4` loads it into the gold bank on a win) and
the `+0x2c` NUL-terminated AI move-pattern (symbols `1`/`2`/`3` = the three
attack types). Baked into the Baka overlay (PROT 0976, base `0x801CE818`, file
offset `0x8EA4`).

- `parse` → 17 `BakaOpponent` records; `attack_at(cursor)` plays the CPU pattern
  (`pattern[cursor % len] - 1`). `ROUND_WIN_TARGET = 2` (best of 3). No Sony
  bytes committed (disc-gated `baka_opponents_real`). See
  [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md).

### `muscle_dome`

Resolves the Muscle Dome minigame's overlay identity: its match SM
`FUN_801d0748` + data tables (deck `0x801f4b8c`, sub-draw script `0x801f4d34`,
victory messages `0x801f4dfc`) are **resident in the battle-action overlay
(PROT 0898)** - it is a mode of the battle engine, not a separate overlay.

- `verify_resident(overlay)` confirms a PROT 0898 image hosts the arena (the
  match-controller prologue signature at file offset `0x1F30` reads the ctx
  `_DAT_8007bd24`, and the sub-draw / victory tables hold in-overlay pointers).
  Disc-gated `muscle_dome_real`. See
  [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md).
