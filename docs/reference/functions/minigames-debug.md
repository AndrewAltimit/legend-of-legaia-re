# Key functions: minigames + debug overlays

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Minigames

Each minigame's per-frame controller, with the full per-overlay function tables in its subsystem page. These overlays **VA-alias** - the four minigame-hub overlays (fishing / slot machine / Baka Fighter / dance) are distinct files that share a common library core, so the *same* address hosts a *different* function in each; always read the overlay-qualified dump (`overlay_<minigame>_<addr>.txt`), not the bare address. The "main entry" addresses some captures label per minigame (`801d63b0` / `801d2cc0` / `801d5ed0` / `801d2f38`) are the shared **textured-quad sprite/HUD emitter** the minigame reuses for every draw (hence their high caller counts), not the controller - the real per-frame drivers are below.

| Address | Role |
|---|---|
| `801CF3BC` | **Fishing** per-frame mode driver; `DAT_801d926c` state machine (rod-select / cast / reel / catch / exit). See [`minigame-fishing.md`](../../subsystems/minigame-fishing.md). `overlay_fishing_801cf3bc.txt`. |
| `801CF0D8` | **Slot machine** per-frame reel state machine (states 0..100; commits the overlay-local balance to coin bank `0x800845A4` on cash-out). See [`minigame-slot-machine.md`](../../subsystems/minigame-slot-machine.md). `overlay_slot_machine_801cf0d8.txt`. |
| `801D3380` | **Slot machine** payline draw. Emits the five paylines as `LINE_F2` prims (GP0 `0x43` - flat, semi-transparent) from the 5 x 16-byte geometry table `DAT_801d3680`, each endpoint projected on its own. Idle colour `0x808080`; the line whose index equals `DAT_801d3c8c` has only its three colour bytes overwritten with `(0xFF, 0xFF, 0x80)`, so a lit line keeps the `0x43` code. Ported as `legaia_engine_core::slot_machine::payline_prims`. See [`minigame-slot-machine.md`](../../subsystems/minigame-slot-machine.md#the-paylines-are-3d-lines---fun_801d3380). `overlay_slot_machine_801d3380.txt`. |
| `801CF388` | **Baka Fighter** cabinet mode state machine - the overlay's top-level per-frame driver, a 37-way switch on `DAT_801DBF44`. See [`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md#cabinet-state-machine-fun_801cf388). `overlay_baka_fighter_801cf388.txt`. |
| `801D3468` | **Baka Fighter** round/match resolution state machine (rock-paper-scissors exchange of attack types). See [`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md). `overlay_baka_fighter_801d3468.txt`. |
| `801CF00C` | **Baka Fighter** scene-setup leaf (baka_fighter overlay occupant; VA-aliases per the note above). Runs the one-time graphics/scene init - display-env setup (`FUN_8001DAF8`), primitive-packet + OT allocation (`FUN_8001E3B8`), the streaming asset / SEQ loads (`FUN_8001FC00` / `FUN_8001E54C`), and the duel-actor spawns (`FUN_80020DE0`) - before handing to the round SM `FUN_801D3468`. See [`minigame-baka-fighter.md`](../../subsystems/minigame-baka-fighter.md). `overlay_baka_fighter_801cf00c.txt`. |
| `801CF470` | **Dance** per-frame controller / beat-clock state machine (switch on `DAT_801d5334`). See [`minigame-dance.md`](../../subsystems/minigame-dance.md). `overlay_dance_801cf470.txt`. |
| `801D3A2C` | **Dance** floor render cluster: per-frame draw pass `801D3A2C` (actor list + tile grid) + tile-grid blit `801D2A10` + two-layer step-marker lookup `801D3EC0`→`801D3F54`. Reuses the field scene buffer `_DAT_1f8003ec` (grid `+0x8000`, step layers `+0x10000`/`+0x12000`) + actor list `_DAT_8007c36c`. Live-pinned to the dance overlay via the resident mode-24 slot-A help text. See [`minigame-dance.md` § Dance-floor rendering](../../subsystems/minigame-dance.md#dance-floor-rendering). `overlay_dance_801d3a2c.txt`. |
| `801D6028` | **Scene ground-height solver** - shared slot-A overlay-band code (byte-identical in the fishing / slot-machine / debug-menu images). Returns the world height under an actor off the scene floor buffer `_DAT_1F8003EC` and maintains the actor's `0x800000` off-floor flag. Bilinear corner blend, or a step-layer patch path via `801D79E0`. See [`minigame-fishing.md` § The scene floor buffer](../../subsystems/minigame-fishing.md#the-scene-floor-buffer). `overlay_fishing_801d6028.txt`. |
| `801D6BBC` | **Scene floor pass** - the shared-band sibling of `801D3A2C`: same cell walk, same tile-actor spawn, different overlay-local globals, and it opens with a bounds debug print. Not the field-VM tile board. `overlay_fishing_801d6bbc.txt`. |
| `801D0750` | **Dance** setumei (how-to) tutorial script: the Disco King actor's per-frame state machine over `actor+0x9C`, a 19-slot jump table. See [`minigame-dance.md`](../../subsystems/minigame-dance.md#the-setumei-how-to-tutorial-script-fun_801d0750). `overlay_dance_801d0750.txt`. |
| `801D0748` | The **battle-action overlay's round driver**, which is also the Muscle Dome's per-frame match controller - pad read, phase dispatch on the sub-state byte `_DAT_8007BD24[6]`, direction pick / commit / resolve. **Not a distinct overlay**: the `overlay_muscle_dome` / `overlay_magic_capture` / `overlay_magic_level_up` / `overlay_battle_action` / `overlay_0898` dumps are byte-identical across all 2781 instructions, and it owns no score loop (that is the arena hub's). See [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md) and [`battle.md`](../../subsystems/battle.md#battle-main-dispatcher-fun_801d0748). |
| `801D56E4` | **2-D segment clipper** (fishing overlay 0972). Eight arms: all four scratchpad draw-context bounds (`0x1F800314 + 0x74 / +0x76 / +0x78 / +0x7A`) applied to both `(i16 x, i16 y)` endpoints in place. Port `engine-core::fishing_actors::clip_segment_2d`. `overlay_fishing_801d56e4.txt`. |
| `801D5C2C` | **3-D segment transform + depth clip** (fishing overlay 0972). Pushes both endpoints through the GTE wrapper `FUN_8003D344` (`MVMVA`), rejects the segment when **both** transformed Z fall inside the near cutoff `_DAT_1F80037E` (zeroing both output pairs), otherwise writes the view-space coordinates back and clips against `0x1F800314 + 0x6A`. `overlay_fishing_801d5c2c.txt`. |
| `801D7030` | **Walkability-grid wall probe, high nibble** (fishing overlay 0972). `(x, z) -> bool` against `*(_DAT_1F8003EC) + 0x4000` - the same per-scene grid `FUN_801CFE4C` reads (see [`field-locomotion.md`](../../subsystems/field-locomotion.md)), but taking the byte's **high** nibble rather than its low one, and with asymmetric coordinate ladders (`z` truncating then `+2`, `x` rounding up then `-1`). Leaf, no frame. Port `engine-core::fishing_actors::walk_grid_overhead`. `overlay_fishing_801d7030.txt`. |
| `801D765C` | **Sub-cell separation of two tracked points** (fishing overlay 0972). No arguments; reads two `i16` pairs at `0x801D9184` (`+0` = x, `+4` = y) and `0x801D918C`, normalises the squared distance through `FUN_8005AF0C`, `>> 6` into 64-unit sub-cells, clamps negatives to zero. Port `engine-core::fishing_actors::tracked_point_separation`. `overlay_fishing_801d765c.txt`. |
| `801D7BB8` | **Polar offset helper** - shared slot-A overlay-band code (byte-identical in the fishing / slot-machine / debug-menu images; the PROT 0897 dump at this VA is the empty-dump artifact, not a fourth copy). See [`minigame-fishing.md`](../../subsystems/minigame-fishing.md#the-shared-polar-offset-helper-fun_801d7bb8). `overlay_fishing_801d7bb8.txt`. |

## Debug-menu overlay (PROT 0971, mode-0 CONFIG)

The dev debug menu is the overlay mode 0 (CONFIG) loads (`FUN_80025C68`), resident at slot-A base `0x801CE818`. The generic-slot `overlay_0971_*` dumps are these same functions re-imported at `0x801C0000` (mis-based by `0xE818`) - read the `overlay_debug_menu_*` copy for correct VAs. The whole menu is retail-gated: each per-frame body early-returns unless the debug-enable flag is set.

| Address | Role |
|---|---|
| `801CE97C` | **Debug-menu per-frame controller.** Runs a ~22-row cursor list (index `_DAT_8007B862`): pad edges move the cursor, adjust the selected row's value, or fire the row action - scene-load (`FUN_8001FC00` / `FUN_8001E54C`), SPU voice key-on/off (`FUN_80026478` / `FUN_800266E0`), sound stop-all (`FUN_80017910` + the `FUN_800653C8` voice loop). Draws labeled readouts through the debug string drawer `FUN_8001A068` and the digit drawer `FUN_8001ABC8`. `see ghidra/scripts/funcs/overlay_debug_menu_801ce97c.txt`. |
| `801CF338` | **Debug TMD-viewer per-actor tick.** D-pad edits the viewed object's translate/rotate fields (`actor+0x14/+0x16/+0x18/+0x24/+0x26`), sums POLY/VERT counts over the object table at `actor+0x44`, prints the object readouts via `FUN_8001A068`, and cycles the TMD index `_DAT_8007B6E4`, reloading through `FUN_80024E08`. `see ghidra/scripts/funcs/overlay_debug_menu_801cf338.txt`. |
| `801D0100` | **Sub-screen per-frame body**, and the owner of the `801D0230` bytes - see [below](#801d0100-sub-screen-body). |
| `801D03B0` | **Sub-screen sway helper.** Leaf, no frame. Samples the shared sine table `*_DAT_8007B81C` at `angle`, `angle + 0x400` and `angle + 0x800`, scales each `>> 8` (round toward zero) and biases it by `-0xA`, writing the triple to the render scratch block `0x1F80035E/60/62` after clearing `0x1F80035C`; then advances the angle at `0x801D9118` by `DAT_1F800393 << 4`. Port `engine-core::fishing_chrome::sway_vector`. `see ghidra/scripts/funcs/overlay_fishing_801d03b0.txt`. |
| `801CFE20` / `801CFE5C` | Two 15-instruction mode wrappers: `a0 == 0` runs the per-mode setup (`FUN_801D0100` / `FUN_801D0198`), otherwise they call into the `FUN_801D0100` body at `0x801D0230` and return bit 29 / bit 24 of its result. `see ghidra/scripts/funcs/overlay_debug_menu_801cfe20.txt`. |

<a id="801d0100-sub-screen-body"></a>

`0x801D0230` is **interior to `FUN_801D0100`**, not an entry. Disassembling PROT
0972 at the slot-A base gives a 172-instruction body at `0x801D0100` spanning
688 bytes - `0x801D0100..0x801D03B0` - which contains it; the dump that starts
at `0x801D0230` opens `lw a0,-0x6F10(s0)` on a live `s0` with no prologue and
closes by restoring `s0..s7`/`ra` from a `0x48`-byte frame it never builds.
The body advances a fade timer by `DAT_1F800393`, emits a full-screen dim quad
via `FUN_80024EE4`, draws sub-panels (`FUN_801D13F0` / `FUN_801D1580`), raises
an SFX cue on a pad edge, and tails into `FUN_801D03B0`.

The bytes also do not belong to the debug-menu overlay its dumps are labelled
with: PROT 0971's own content stops at file `+0x1800`, i.e. VA `0x801D0018`,
so everything the `overlay_debug_menu_*` dumps print above that address is
PROT 0972 read through 0971's extraction footprint
([`dump-corpus-integrity.md`](../../tooling/dump-corpus-integrity.md)).

## Other-game minigame overlay (PROT 0977)

Slot-A occupant of the mode-24 sub-id-5 door warp, true base `0x801CE818`. `other_game` is the **CDNAME block name**, not an identity: the module is the Muscle Dome **arena door/init** slot (dev module `other6`), pinned by its own monster-name roster and by the contest settlement `FUN_801D0F60`. The rows below are its HUD primitive layer. The `0x801Dxxxx`-named `overlay_0977_other_game_*` dumps are correctly based; the `0x801Cxxxx`-named ones are mis-imported at `0x801C0000` (add `0xE818` for the true VA, per the same anchor that fixes `801C2748` -> `801D0F60`).

Ports: `legaia_engine_ui::other_game_hud` (the two quad emitters + the decimal readout) and `legaia_engine_core::other_game_overlay` (the step scaler + the SFX cue). See [PROT 0977 HUD primitives](#prot-0977-hud-primitives).

| Address | Role |
|---|---|
| `801D050C` | **Centred sprite-quad emitter** - a `POLY_GT4` packet from the scratchpad pool `0x1F800314+0x8C`, textured and Gouraud-shaded from the descriptor table at `0x801D170C` (stride `0x14`), centred on the argument point. [details](#prot-0977-hud-primitives). `see ghidra/scripts/funcs/overlay_0977_other_game_801d050c.txt`. |
| `801D08EC` | **Corner-anchored sibling** of `801D050C`: same packet, but the argument point is the quad's top-left and the brightness argument is clamped to `0..=0xFF` first. [details](#prot-0977-hud-primitives). `see ghidra/scripts/funcs/overlay_0977_other_game_801d08ec.txt`. |
| `801D1308` | **Decimal readout** - up to eight digits through `FUN_801D050C`, stepping the digit record's texture column per glyph. Negative values draw nothing. [details](#prot-0977-hud-primitives). `see ghidra/scripts/funcs/overlay_0977_other_game_801d1308.txt`. |
| `801D1288` | **Round-robin SFX cue** - one `FUN_80065034` voice-attr call per frame across voices `0x10..=0x13` (counter `DAT_801D1AE4 & 3`), at program `0` / tone `1` / note `0x3C`. Its last two arguments are `vol_l` / `vol_r`, halved out of the voice-volume config `_DAT_80084580`; the earlier "positioned from the party-block word" reading is falsified, see [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md#the-arenas-per-frame-voice-cue-fun_801d1288). Not a sprite emitter - `FUN_80065034` is the libsnd `SpuSetVoiceAttr` analogue. `see ghidra/scripts/funcs/overlay_0977_other_game_801d1288.txt`. |
| `801D14B0` | **Step-size scaler** - a leaf: returns the argument unchanged while flag `DAT_801D1AB4` is set, else `arg/5` (`arg > 5`), `1` (`arg < 3`), or `arg/2`. `see ghidra/scripts/funcs/overlay_0977_other_game_801d14b0.txt`. |
| `801CF074` (true VA; the `801c085c` dump is mis-based, `+0xE818`) | **Contest score-tally screen** - the six-row label + number readout, and the count-up that fills it. [details](#the-contest-score-tally-screen-fun_801cf074). `see ghidra/scripts/funcs/overlay_0977_other_game_801c085c.txt`. |
| `801CEA6C` | **Contest entry** - re-entered after every leg. Advances the mode-24 sub-id word `_DAT_8007BAC0` by one when it is already non-zero (`0x801CEC00`), decodes `(course, round)` out of its low byte into `DAT_801D1A90` / `DAT_801D1A94`, picks the opening course from story flags `0x536`/`0x537`/`0x538`, clamps the Master course's length against `0x378`/`0x382`/`0x471`, and routes the hub on `DAT_8007BD60 & 0x80` (the party-standing bit). [details](../../subsystems/minigame-muscle-dome.md#two-state-machines-not-one). |
| `801D1184` | **Leg score rows** - the four count-up lanes a cleared leg is worth: `round*2`, `min(turns_taken, 8)` and `DAT_801D1A5C[min(outcome,3)]` (`= [8,12,4,2]`), each `× max_hp / 100` through the `0x51EB851F` reciprocal, plus the raw `(course, round)` score cell. The first three drain into the HP-restore accumulator `DAT_801D1AC8`, not into the tally. [details](../../subsystems/minigame-muscle-dome.md#what-a-cleared-leg-is-worth). |
| `801D0ED8` | **Contest-start restore** - refills the fighter's HP/MP/SP to their maxima on the `0x80084140` window, and, only when `DAT_801D1A90 != 0` (`bnez` at `0x801D0EE8`), first zeroes the four equipment bytes `+0x75E`/`+0x75F`/`+0x760`/`+0x762`. So "no equipment" is an Expert/Master rule; the Beginner course keeps its gear. |
| `801CF870` | **Contest hub** (and the intro / title / interval screens it draws) - dispatches the hub state `DAT_801D1A78` through the 51-entry jump table at `0x801CE990`; states `0`, `1`..`6`, `0x0A`..`0x0C`, `0x14`..`0x16` and `0x32` are real, the other 37 route to its default arm. State `0x0C` is the between-leg HP restore (`0x801CFE7C`), `0x32` settles. Every state but `0x32` tails through the `(course, round)` re-pack at `0x801D00B8`. Draws the "Welcome" strip (record 3) centred on `(160, 120)`, the course-title art (record 4) at `(160, 64)` under a shrinking scale ramp with a variant-2 shadow at `(168, 72)`, and the INTERVAL heading (record 16) at `(160, 32)`; tails into `FUN_801CF074` with its own fade counter. |
| `801D02F0` | **ROUND banner** - record 0 centred on `(120, 120)` in both variants, then `DAT_801D1A94 + 1` as one digit at `x=240` or two at `x=240`/`x=264`, each in both variants. |
| `801D15C8` | **ROUND digit glyph** - sets record **1**'s `u0` to `digit * 24` and delegates to `FUN_801D050C` with the index forced to 1. Not the decimal readout's record 9. |
| `801D1510` | **Opponent installer** - indexes the course descriptor table `0x801D1A08` by `DAT_801D1A90`, its `+4` round pointer by `DAT_801D1A94`, `lbu`s the round record's `+4` monster id and `sb`s it into formation slot 0 at `0x8007BD0C` (clearing slots 1..3, and writing `0x14` to the stage word `0x8007B83C`). The arena's opponent, per (course, round). [details](../../subsystems/minigame-muscle-dome.md#course-ladder-the-opponent-per-course-round). |
| `801D0CD4` | **Course menu** - walks all three descriptors, using each `+0x00` count as its loop bound and each round record's `+0x00` label pointer through the text drawer `0x80036888`; also clamps `DAT_801D1A94` against the count. |

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### The contest score-tally screen (`FUN_801CF074`)

`FUN_801CF074(alpha)` is one whole screen: four count-up lanes and the
twelve quads that show them. It returns non-zero while the count-up is still
running and `0` once it has finished, which is how its caller knows to hold
the screen.

Each lane accumulates the scratchpad frame delta (`0x1F800393`) into its own
fade counter, clamps that counter at `0x10`, and then - if its pending value
is non-zero - takes one step through `FUN_801D14B0`, moves it from the
pending counter into the sink, and fires `FUN_801D1288`. A lane whose
pending value has reached zero instead starts the next lane's fade, so the
rows fill in sequence.

| Lane | Fade counter | Pending | Sink |
|---|---|---|---|
| 0 | `0x801D1ABC` | `0x801D1ACC` | `0x801D1AC8` |
| 1 | `0x801D1AC0` | `0x801D1AD0` | `0x801D1AC8` |
| 2 | `0x801D1AC4` | `0x801D1AD4` | `0x801D1AC8` |
| 3 | `0x801D1AB8` | `0x801D1AAC` | `_DAT_80084440` |

Lane 3's sink is the running contest score tally the settlement routine
`FUN_801D0F60` settles, which is what identifies the screen.

The draw is six label strips (records `10..=15`, corner-anchored at `x=0x40`,
`y=0x50` stepping `0x10`, scale `0x1000`) followed by six decimal values
(`x=0xC0`, `y=0x55` stepping `0x10`), each set emitted twice - variant 1 then
variant 2, and for the digits palette `0`/`2` then `1`/`3`, with record 9's
`+0x0F` / `+0x13` bytes stamped between the passes. Every brightness is
`fade * (alpha << 4) >> 7`. Port: `legaia_engine_ui::other_game_hud`
(`HUB_SCORE_TALLY_LABELS`, `score_tally_quads`).

### PROT 0977 HUD primitives

Three routines share the sprite descriptor table at `0x801D170C`. Each record
is `0x14` bytes:

| Offset | Field |
|---|---|
| `+0x00` | `i32` texel-to-world size scalar, applied before the caller's scale |
| `+0x04` | `u16` base tpage word; the emitter adds `page * 0x20` |
| `+0x06` | `u16` CLUT word |
| `+0x08` / `+0x09` | `u8` texture U / V of the top-left texel |
| `+0x0A` / `+0x0B` | `u8` texel width / height |
| `+0x0C..0x0E` | `u8` RGB of the two **top** vertices |
| `+0x0F` | non-zero selects the semi-transparent command (`0x3E` over `0x3C`) |
| `+0x10..0x12` | `u8` RGB of the two **bottom** vertices |
| `+0x13` | `u8` tpage page offset |

The two colour triples make every quad a vertical two-stop gradient - the
packet is a `POLY_GT4`, thirteen words including the tag, not a flat sprite.

The `sel` argument packs a table index in its low ten bits and a **variant**
above them (`sel / 0x400`, truncating). A non-zero variant is a *write* into
the shared record: it sets `+0x0F` to `1` and `+0x13` to the variant, and
those stay set for every later call on that record. Variant `2` additionally
draws with `CLUT + 1`, which the record itself never sees.

Geometry differs only between the two emitters. `FUN_801D050C` halves the
extent (`(texels * size) >> 13`, then `* scale >> 12`) and brackets the
argument point, `x - half ..= x + half - 1`. `FUN_801D08EC` shifts once less
and spans `x ..= x + extent` from the argument point, and clamps brightness
into `0..=0xFF` before scaling - the centred emitter does not, so a brightness
above `0x100` wraps its colour bytes there.

Both post through `FUN_8003D2C4` at the ordering-table depth held in
`DAT_801D1AA8`, then reset that depth to `3`.

`FUN_801D1308` renders a decimal readout through the centred emitter using
record index `9`. Its eight digit slots start at `-1`, the units slot is
pre-seeded with `0`, and slot `i` is overwritten with `value / 10^(7-i)` only
when that quotient is non-zero; a slot holding a negative quotient is skipped
at draw time. Two consequences: `0` renders as a single `0`, and a **negative
value renders nothing at all**. The pen advances 8 px per slot including the
skipped ones, and the digit record's CLUT is offset by the palette argument
for the call and restored to `0x7D86` on return.

### `801CFE20` / `801CFE5C`

**MDEC in / out DMA sync.** Both live in the slot-A STR/FMV overlay and are
byte-identical in PROT 0970 (`cutscene_str`) and PROT 0971 (`debug_menu`) -
the same co-residency the MDECin DMA-callback hook `FUN_801CFE98` shows (see
[`subsystems/cutscene.md`](../../subsystems/cutscene.md)). They are **not**
debug-menu logic; the `overlay_debug_menu_*` dumps at these VAs are that
capture's copy of the same overlay bytes.

Each takes one argument selecting between a blocking wait and a poll:

| Entry | argument `0` | argument non-zero |
|---|---|---|
| `801CFE20` | `FUN_801D0100` - spin until MDEC-**in** idle | bit `0x1D` of the status word |
| `801CFE5C` | `FUN_801D0198` - spin until MDEC-**out** idle | bit `0x18` of the status word |

The blocking halves count down from `0x100000`, re-reading their status word
each pass. They return `0` the moment the busy bit (`0x2000_0000` in-side,
`0x0100_0000` out-side) clears, and `-1` after logging `"MDEC in sync"` /
`"MDEC out sync"` if the budget runs out. Both polling halves read the *same*
word - `FUN_801D0230`, a six-instruction leaf that dereferences the in-side
pointer global - so the out-side poll queries the in-side register while its
own blocking arm queries the out-side one. Port:
`legaia_engine_core::mdec_dma_sync`.
