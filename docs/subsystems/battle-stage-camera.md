# Battle stage and camera

## Battle background

A battle is fought **on the environment where the encounter triggered, kept
resident and rendered as a full 3D backdrop** - the battle does not load a
separate flat arena. The battle-action SM only swaps the **camera** (from the
field/world walk camera to a slow orbit around the party↔enemy midpoint) and
overlays the actors + HUD; the surrounding terrain keeps drawing through its
normal renderer.

For an **overworld (world-map) encounter** the backdrop is **two layers** -
a flat tiled **ground grid** + the map's `scene_tmd_stream` **dome** (sky +
distant mountains) - pinned from a 4-angle capture set
(`overworld_battle_bg_angle_a..d`, the same Vahn-vs-Gobu-Gobu battle paused on
the Begin/Run menu while the camera idly orbits).

### Backdrop ground - a procedural flat grid (`func_0x801d02c0`)

The grass underfoot is **not** geometry from a file; it is a procedural flat
tiled grid emitted by `func_0x801d02c0` (battle-overlay variant), the **sole
draw call** the mode-`0x15` render `FUN_80026f50` makes
(`ghidra/scripts/dump_battle_backdrop_draw.py`). It is a GTE rasteriser, not a
TMD walk:

- A `_DAT_1f8003f8 × _DAT_1f8003fa` cell grid (cell pitch `0x200`, sub-step
  `0x100`), centred at the world origin on a **`Y ≈ 0` flat plane**.
- **Pass 1** - RTPS each grid point and write a per-cell visibility byte
  (`-1`/`0`/`1`) into the `0x1000`-byte buffer `_DAT_8007b814` (so the grid can
  be up to ~64×64). **Pass 2** - for each visible cell, RTPT its corners and
  emit one `POLY_GT4` (GP0 `0x0C000000`) into the ordering table.
- These tiles are the **619 `POLY_GT4`** in the live pool. Because the grid is a
  *full* flat plane centred on the actors, it fills the foreground/ground at
  **every** orbit angle - there is no half-dome gap for the ground.
- **Texture address (constant in the overlay, content per scene).** The grid
  quads sample a **4bpp texture page at framebuffer `(832, 0)`** (tpage attr
  `0x000D`) with **CLUT `(0, 479)`** (CBA `0x77C0`), UV window
  **`(192..255)²`** - scratch literals in `func_0x801d02c0`, confirmed
  against the GT4 packets in the live prim pool of the Tetsu battle states.
  The 64² window is stretched across one whole `0x200` cell as **four quads**:
  the emit loop runs 2×2 times per visible cell and advances the sub-tile row
  pointer by `0x10` each time, so the sub-tile is `sub_row * 2 + sub_col` and
  there is **no RNG anywhere in the routine**. An earlier reading here - "each
  cell samples one sub-tile with a per-cell random corner mirror", over "two
  distinct variants duplicated across the row" - was wrong on both counts: the
  tiling is deterministic, and the variant count is a claim about the texture's
  content rather than about the renderer. The random corner mirror is real but
  belongs to the effect-VM walker `FUN_801E0080` (`rand() % 4` → two mirror
  bits on each child billboard). See [`functions/battle.md`](../reference/functions/battle.md#801d02c0).
  The *address* is scene-independent - the
  scene's battle VRAM build is what places that scene's own ground tile
  there (`town01` = warm sandy pebbles; an earlier engine heuristic that
  borrowed the dome's nearest "grass vertex" sampled a blue texel region in
  `town01` and painted the floor sky-blue). Engine mirror:
  `build_battle_ground_grid` in `play-window` (an alias of
  `legaia_asset::battle_backdrop::build_ground_grid_rgbc`).
  The historical overlay capture filed under the `0896` label (a mislabeled
  slot-A window image; PROT 0896 itself is neither the battle background nor
  an overlay that loads here) shows the same grid renderer + `_DAT_8007b814`
  buffer - it is battle-overlay code seen through that capture.

#### The grid's own constants, read off the emitter

The sub-tile UVs are not derived - they are sixteen literal words the prologue
builds into scratchpad `0x1f800034` (`0x801d0304..0x801d03a0`) and the emit
loop reads back one group per quad, advancing `0x10` each time
(`0x801d0660` / `0x801d06c8`). Decoding them as `POLY_GT4` UV words gives four
fixed 32×32 blocks of the `(192..=255)²` window:

| Quad | Words | `u` | `v` |
|---:|---|---|---|
| 0 | `77c0c0c0 000dc0df 0000dfc0 0000dfdf` | `0xC0..=0xDF` | `0xC0..=0xDF` |
| 1 | `77c0c0e0 000dc0ff 0000dfe0 0000dfff` | `0xE0..=0xFF` | `0xC0..=0xDF` |
| 2 | `77c0e0c0 000de0df 0000ffc0 0000ffdf` | `0xC0..=0xDF` | `0xE0..=0xFF` |
| 3 | `77c0e0e0 000de0ff 0000ffe0 0000ffff` | `0xE0..=0xFF` | `0xE0..=0xFF` |

The `clut` half of word 0 and the `tpage` half of word 1 are where the `0x77C0`
/ `0x000D` address above comes from. No corner is ever mirrored: the four UVs
are copied into the packet verbatim.

**Grid origin.** `0x801d03b4..0x801d03d8` computes `x0 = -((w >> 1) << 9)` and
`z0 = -((h >> 1) << 9) - 0x200`. The `z` axis carries an extra cell of bias, so
the grid is not symmetric about the origin - at the live 28×28 it spans
`x ∈ [-7168, +7168]` but `z ∈ [-7680, +6656]`.

#### The grid's near colour and cue depth

Each lattice vertex is cued by `DPCS` from the GTE `RGBC` register, which the
emitter loads from scratch `0x1F800398` (`lwc2 a2, 0x84(t9)` at `0x801d05f4`,
`t9 = 0x1F800314`) and never writes. `FUN_80026CE4` rewrites that word every
frame from the ambient word `0x8007B7B0`, and the backdrop pass `FUN_80050120`
stores the ambient beside the far colour, on every stage class, as the stage
base plus `0x404040` (`0x800507E0..0x800507F0`). Once the intro fade has
settled the base is `0x808080`, so the floor's **near** colour is `0xC0` per
channel - the texel is lifted by half before the cue blends it toward the far
colour. Every catalogued battle capture outside a cast, indoor and outdoor,
holds `0x8007B7B0 = 0xC0C0C0`.

**A cast dims it.** The base is not a constant: it is `ctx+0x890`, a packed
`10:10:10` colour (channel `c` at bits `2 + 10c`) that `FUN_80050120` ramps
every frame on `ctx+0x243`. While the byte is set (`0x80050608`) the ramp
subtracts `step * 0x20` per lane (`8` per 8-bit channel per vsync, `step`
being the frame step `0x1F800393`, `0x80050670..0x800506A4`) down to the
floor `0x08020080` - base `0x20`; while it is clear it adds `step * 8` per
lane (`2` a vsync) back up to `0x20080200` - base `0x80`
(`0x80050724..0x8005075C`). The ambient stored is the base plus `0x404040`,
so a cast pulls the grid's near colour from `0xC0` toward `0x60`, and the far
colour `0x8007BB48` is derived from the same word (`0x800507FC..0x80050834`).
Battle init seeds the floor (`0x80051C84`), so every fight's floor fades in
over its first 48 vsyncs.

Who drives the latch: the summon close-up (`FUN_801DC0A0` case `0x12`,
`sb v0,0x243(v1)` at `0x801DCCFC`) sets `ctx+0x243 = 1` on every `0x33` /
`0x34` pass. The summon band's `0x37` exit (`0x801E4E8C..0x801E4EA4`) and the
capture band's `0x71` exit (`0x801E5214..0x801E5248`) clear it together with
`ctx+0x278` and re-seed the base at `0x08421084` (`0x21` a channel), so the
floor climbs back from dark after the creature leaves.

**The store can freeze.** Once the base sits on the floor, the pass skips
both colour stores when `ctx+0x278` bit 0 is set or `ctx+0x243 == 2`
(`0x80050790..0x800507D4`), and clears bit 3 of `0x1F800394` - the gate on
`FUN_8001D058`'s call to `FUN_80026CE4`, the routine that copies the ambient
into `RGBC`. The summon band sets `ctx+0x278 = 1` at `0x32 -> 0x33`
(`0x801E49F8`) and clears it at the `0x34` exit (`0x801E4B14`), so through the
close-up the grid holds its **last pre-floor** ambient. That is what the
catalogued summon states read: the `0x34` captures hold
`0x686868..0x787878` with the live base already at `0x20` (a `0x33` capture
still mid-ramp holds its live base plus `0x404040`), and every `0x35` capture -
`0x278` cleared, stores resumed - holds `0x606060`. The frozen value
varies with the frame step, since the last pre-floor base depends on how many
vsyncs each ramp step spans.

The stage meshes do not ride the ambient. The backdrop pair
(`ctx+0x106C` / `+0x1070`) has a parallel ramp of its own in the same pass:
their `+0x78` depth-cue weight rises by `step << 6` while `ctx+0x243` is set,
toward `0x800` (indoor), `0xC00` (outdoor) or `0x1000` (`ctx+0x278 > 1` or
`ctx+0x243 > 1`), and falls back to `0` while it is clear
(`0x800505B0..0x80050714`); `FUN_8001ADA4` case 3 hands that weight and the
record's `+0x74` colour word (`0` from battle init) to `FUN_80043390`. A
weight of `0x1000` switches `+0x56` to `0`, which drops the pair from that
dispatcher entirely (`0x80050848..0x80050880`). The battle bodies are not lit
from the ambient either; `ctx+0x243` reaches them only through the tint
pass's plain arm ([the distance fade](battle-actor-rendering.md#the-distance-fade)).

Engine side: `legaia_engine_vm::battle_ground_grid::ambient_base_step` is the
ramp, `BattleActionCtx::ambient_base` the word, and
`World::tick_battle_ambient` runs it once a vsync with the store-skip test
over the band's and the slot-B module's copies of `ctx+0x278`. Both hosts
colour the grid from `World::battle_ambient_base`: the native window
re-uploads the grid mesh when the ambient moves and re-derives the cue's far
colour every frame; the play page re-reads the packet colours and the cue on
the `play_battle_ground_ambient_key` change key. The backdrop pair's `+0x78`
ramp is `battle_ground_grid::backdrop_cue_step`, stepped beside the ambient in
`World::tick_battle_ambient` over the same two bytes, with the stage's
outdoor-table membership (`BattleState::stage_outdoor`, set by the host that
resolved the stage) picking the ceiling. Both hosts read it through
`World::battle_backdrop_cue`: a flat per-draw cue toward black on the stage
draw, and no stage draw at all at full weight. A summon module drives it
there by storing `2` or `3` into `ctx+0x278` - PROT 0903's arms 4 and 6, so
Gimard's attack plays inside its fire tunnel with nothing of the stage behind
it (`gimard_burning_attack` reads both records at `+0x78 = 0x1000`).

`IR0` is `SZ >> 2` on the vertex's own screen depth, with no scale of the
battle world folded in. The `map01` Gobu Gobu capture's grid packets
(`mednafen-state display-list`, the `77C0/000D` family) climb from `0xCC` per
channel at the bottom edge through `0xE9` at mid-ground to the `0xFF` clamp at
the horizon; the bottom edge sits roughly `0xC00` deep under the far framing,
and `0xC0 + (0xFE - 0xC0) * SZ / 0x4000` lands there only with `SZ` unscaled.
Engine side: both battle
hosts build the grid with `build_ground_grid_rgbc` over the live ambient
(`legaia_asset::battle_backdrop`, `legaia_engine_vm::battle_ground_grid`) and
cue it over the unscaled `grid_cue_far_z`. With the neutral `0x80` and a ramp
four times too long, the port's floor read at about two thirds of retail's
brightness on outdoor stages and too bright on indoor ones.

**The two culls.** Pass 1 transforms each cell *centre* by the view matrix
(`cop2 0x0480012` = `MVMVA` rotation/`V0`/`+TR`/`sf=1`), reads `IR3` back, and
writes `-1` / `0` / `1` per cell: `-1` when `z + 0x200 <= 0`, `0` when
`z > 0x6500`, else `1`. Only `1` emits - pass 2 skips on both the `bltz` and
the `beq zero` (`0x801d04b0` / `0x801d04b8`). There is **no screen-space test
in pass 1**; the screen-rect reject is separate, in pass 2
(`0x801d052c..0x801d05e8`), and drops a cell only when all four outer corners
fall past the same edge of the `0x140 × 0xF0` display.

Both are ported and tested (`battle_backdrop::classify_cell` /
`cell_offscreen`) and neither is applied by the port's builder: they remove
only geometry that is off-screen or behind the camera, which a depth-buffered
projection discards anyway, and the port uploads the grid once while the
camera orbits over it.

**Where the tile comes from.** The two addresses are constant for the whole
game, but the pixels behind them are not: each `scene_tmd_stream` entry carries
its own TIM at framebuffer `(832, 0)` with a palette at `(0, 479)`, so the
floor changes per stage while the emitter never does. 178 of the 182 backdrop
entries carry that pair, and **no** entry fills that page under a different
palette - which is what pins the constants against the corpus rather than
against one stage. The four that carry neither must draw no floor at all: an
untextured grid is a flat slab across the whole stage, which is a worse artifact
than an absent one. `battle_backdrop::ground_grid_drawable` is that decision,
shared so the two viewers cannot answer it differently, and the sweep is
`the_ground_tile_is_addressed_by_the_emitters_own_constants`.

The asset-viewer PROT browser and the browser entry viewer both draw the grid
under a backdrop. Two traps sit on that path. The grid is appended **after** the
shell's second copy, because it is world-fixed rather than part of the shell and
handing it to the transform would draw it twice, once flipped in `Z`. And the
browser viewer's VRAM upload is *targeted* - it uploads only the blocks the
TMD's own primitives sample - so the grid's page has to be added to that request
by name (`ground_page_rect` / `ground_clut_rect`); left out, the mesh builds
fine and the floor draws untextured, a failure visible only on screen.

> **Correction.** An earlier reading called the backdrop the *world-map continent
> heightfield* per a `prim-trace` "3715 hits in `0x80190000`". That was a **false
> positive** (3 degenerate `clut=0` `POLY_FT4` prims stride-1 flooding that
> window). The ground is this **flat procedural grid**, not a per-tile continent
> descriptor table read from RAM, and not a 3D heightfield (cell `Y ≈ 0`).

### Which stage stream a scene fights in

A scene bundle is a fixed slot array - `.MAP`, v12 table, event scripts, asset
table, texture pack, then **one `scene_tmd_stream` per sub-area**. The battle
backdrop is whichever of those streams the type-`0x01` chunk walker
`FUN_8001FE70` last recorded in `_DAT_8007B864` (its sole writer, at
`0x8001FEC0`), so the choice is scene data, not a code table - and it is **not
uniformly the block's first stream**:

| Scene | Bundle slot | Extraction entry | Dome shape | Pinned from |
|---|---|---|---|---|
| `map01` (overworld) | 5 | 88 | 4 objects, 340 verts | the four camera-orbit angle saves |
| `town01` (Rim Elm) | 6 | 7 | 2 objects, 341 verts | the three Tetsu tutorial anchors |

Rim Elm's bundle carries four sub-area backdrops (entries 6..9); the Tetsu
sparring match is fought in the **second**. Each row is pinned by reading
`_DAT_8007B864` in a battle save state, taking object 0's live vertex pool, and
byte-matching it back to a PROT entry.

> **Over-read trap.** PROT extraction over-reads into the following entries, so
> the Rim Elm dome's bytes also appear inside entry **6**'s file - at offset
> `0x16038`, past entry 6's own `(next_lba - lba) * 0x800 = 0x14000`. Any "scan
> the block for the resident dome" sweep must reject hits beyond an entry's
> unique length or it will attribute the backdrop one entry too low. Entries 7
> and 8 additionally share a vertex *count*, so shape alone cannot separate them
> either - only the bytes can.

Engine mirror: `ProtIndex::battle_stage_entry_for_scene`, consumed by
`play-window`'s `build_battle_stage`. Tests
`crates/engine-core/tests/battle_stage_entries_real.rs` (disc) and
`crates/engine-shell/tests/battle_stage_live.rs` (save library).

### Backdrop shell - two copies of one mesh

The sky hemisphere, distant mountain ring and far ground ring come from the
scene's `scene_tmd_stream` entry (PROT `88` for `map01`) - `POLY_GT3` prims,
116 of them on screen in the angle-a capture. The entry is loaded by the
type-`0x01` chunk walker `FUN_8001FE70` into `_DAT_8007b864` and lands
contiguously in battle RAM (base `0x800A8B34` for PROT 88, byte-matched across
the four angle saves; leading TMD magic `0x80000002` at file `+4`,
uncompressed). PROT 88/89/90 share identical geometry and differ only in
texture payload.

#### One primitive list, two texture classes

A shell is not all texture. About a fifth of it by primitive count is
`F*`/`G*` flat / gouraud panels that carry a baked colour word and no UVs -
the sky band, the painted wall faces, the flat water. `town01`'s Tetsu arena
is 325 textured triangles and 79 untextured; `map01`'s dome is 336 and 78.
Retail draws them together: `FUN_8001ADA4` case 3 walks the whole group chain
and the GPU takes `POLY_F*` packets as readily as `POLY_*T*` ones.

The port has to reassemble that from two builders, because
`tmd_to_vram_mesh` drops any prim with no UVs - such a prim samples nothing.
The native window pairs it with `tmd_to_color_mesh` on the untextured
pipeline; the browser page uses the single `tmd_to_vram_mesh_field_hybrid`
mesh with a per-vertex textured flag. Both halves take the same second-copy
transform (`ColorMesh::append_scaled` mirrors the textured builder's, winding
reversal included), and both hosts must end up with the same triangle set -
pinned by `the_backdrop_shells_untextured_half_is_a_double_digit_share` in
`crates/engine-core/tests/battle_stage_entries_real.rs`. Rendering only the
textured half punches holes in the arena wherever a sky panel belongs.

#### The stage streams of one bundle share their VRAM

A scene bundle carries one `scene_tmd_stream` per sub-area, and those streams
are **not** allocated disjoint VRAM. Rim Elm's four (extraction entries
6..=9) each declare the same two 4bpp pages, `(768, 0)` and `(832, 0)`, under
the same two CLUT rows, `473` and `479`; the field texture pack puts a page
at `(768, 0)` as well. Retail never has to arbitrate, because the chunk
walker records one stream in `_DAT_8007B864` and only that one is resident.

A port that DMAs every TIM in the bundle - which the battle resource build
does, `BuildOptions::upload_all_tims` - leaves whichever sibling was written
last holding the address, and the shell then draws through a neighbouring
sub-area's texels and palette. `town01`'s semi-transparent cloud band
(`(768, 0)` at `v` 191..254, palette `1` of row 473, a greyscale + STP ramp)
came out as flat green rectangles standing on the arena wall, because the
palette that won the row was one of the rainbow CLUT-cycling ramps a sibling
parks at that index.

`engine-core::scene::upload_battle_stage_tims_into_vram` re-uploads the
selected entry's own TIMs last, restoring retail residency without touching
the rest of the build. Both hosts call it from their `build_battle_stage`.
Sweeps: `rim_elms_four_stage_streams_all_claim_the_same_vram` and
`the_selected_stage_entry_owns_its_vram_after_the_reupload`.

The shell is authored as **half** a bowl. That is the real shape, not a
truncated parse: across all 182 entries object 0 puts at most 8 % of its X or
Z extent past `X = 0` / `Z = 0`, and every object satisfies
`vert_top + n_vert * 8 == normal_top` exactly. What closes the circle is a
second draw of the same mesh.

**What the second copy is worth, measured.** Project `map01`'s drawn objects
through the exact camera each of the four angle captures was taken at (yaw
`_DAT_8007B792`, pitch `32`, `TR = (0, 1280, 7680)`, `H = 256`, all read from
the save state) and count the 320 screen columns the mountain ring covers:

| Capture | Camera yaw | One copy | Two copies | Retail pixels |
|---|---|---|---|---|
| a | 19.7° | 100.0 % | 100.0 % | 98.1 % |
| b | 334.7° | **71.9 %** | 100.0 % | **100.0 %** |
| c | 275.6° | 100.0 % | 100.0 % | 100.0 % |
| d | 231.3° | 99.7 % | 100.0 % | 100.0 % |

Three of the four yaws cannot tell the models apart - one copy already fills
the frame. Capture **b** can: a single copy leaves columns `0..89` with no
mountain geometry at all, and the retail framebuffer has a mountain band in
**90 of those 90 columns** (mean thickness 15.3 px). The second copy is not an
embellishment the captures merely tolerate; without it those pixels have no
source.

#### Two actors, one registered mesh

`FUN_800513F0` registers the TMD **once** - `80051a60 jal 0x80026b4c`, slot
stashed at the descriptor `0x8007680c + 4` = `DAT_80076810` - and then calls
`actor_alloc` (`FUN_80020DE0`) **twice** from that same descriptor
(`80051a7c`, `80051aa8`), parking the two actor pointers at
`battle_ctx + 0x106C` (copy A) and `+0x1070` (copy B). Both are ordinary
battle actors on the normal draw path, which is why `DAT_80076810` has no
resolved reader: the actor list is walked pointer-indirect.

They are two genuine draw entries, not one entry visited twice: each actor
gets its **own** `0x9C`-byte part table at `+0x44`, zeroed in `actor_alloc`
(`80020f04`) and allocated in the link pass (`80021184`). Live battle states
read two distinct table pointers, and the object-count edit below is applied
to each separately.

`FUN_80050120` drives the pair in lockstep - the depth-cue ramp at `+0x78` and
the draw-mode selector at `+0x56` are written to both on the same path
(`80050848..80050880`). `+0x56 = 3` selects case 3 of `FUN_8001ADA4`'s jump
table (`8001ae60 lhu v0,0x56(s0)`, table at `0x8001042C`) - **not**
`FUN_80048A08`.

Copy A draws at raw coordinates. Copy B gets one of two transforms:

| Selector | Written by | Effect | Determinant |
|---|---|---|---|
| `+0x26 = 0x800` (default) | `80051bc0`/`80051bc4` | half turn about world Y | `+1` |
| `+0x5A = 2` (exception) | `80051cc4`..`80051ce4` | X scale `-1` - reflection in the YZ plane | `-1` |

`+0x26` is the second of the three half-words `FUN_80026988` reads at
`actor + 0x24`; that kernel writes `sin` of it bare into matrix element
`[0][2]` and `cos` into `[2][2]`, which only a Y rotation does. `0x800` of the
`0x1000` full turn is exactly 180 degrees. The exception path routes through
`FUN_8001ADA4` case 3, which turns `+0x5A & 2` into `_DAT_1F800348 = -0x1000`
(`8001af28`..`8001af34`) and calls `FUN_8005B4E8` (`ScaleMatrix`, column
scaling - so the reflection is in model space, under the rotation). The same
predicate `+0x5A & 0xE` (`8001afd8`) negates the per-object rotation argument
and swaps the draw-call mode word from `0x40000000` to `0x48000000` - the
winding compensation a negative-determinant transform needs.

The compensation is to stop culling, not to flip a winding. The mode word is
ORed into the node's `+0x74` and handed to the prim dispatcher `FUN_80043390`
as its colour argument (`0x8001B014..0x8001B024`), and the dispatcher reads
bit `0x08000000` as "both sides": the NCLIP mask it stores at `-0x2D8(t2)` is
`0xFFFFFFFF` without it and `0x7FFFFFFF` with it (`0x80043520..0x80043540`),
and every prim leaf ANDs the signed area with that mask before its sign test
(`and s2,s2,s3` / `bltz` at `0x80043E78` in the GT3 leaf `FUN_80043DD4`). So
copy A, and copy B under the half turn, are back-face culled; the mirrored
copy B draws both sides. The two backdrop nodes of
`nivora_duel_pre_megaton_press` read `+0x74 = 0`, one with `+0x5A = 2`. The
port draws every battle mesh both-sided (`camera_view::nclip_cull_mode` is `0`
in battle); on that capture the difference is invisible - the shell's
`0x7640` additive group, which no retail packet of the frame carries, covers
no pixel of the port's frame either.

#### The per-stage table

Which transform a stage gets comes from the zero-terminated `u16` table at
`DAT_80078B50` (`SCUS_942.54` file `0x69350`, 99 slots naming 98 distinct
stages), walked at `80051bc8`..`80051c18` against the backdrop id
`word[0x80084540] + byte[0x8007BD60] & 0x7F`. A hit takes the mirror; a miss
takes the half turn. Stage id + 3 is the PROT extraction index, and every one
of the 98 distinct ids resolves to a `scene_tmd_stream` entry under that
offset.

**The table respects one geometric constraint.** A shell whose open side faces
`-Z` is symmetric about `X = 0`, so reflecting it in the YZ plane reproduces it
in place and fills nothing - only a half turn closes it. Of the 49 `-Z`-open
shells in the corpus, **zero** are on the mirror list; of the 133 X-open
shells, 98 are. Parser `legaia_asset::battle_backdrop`; the disjointness sweep
is `no_z_open_shell_takes_the_mirror_transform` in
`crates/asset/tests/battle_backdrop_real.rs`.

`0007_town01` (stage id 4) is on the list - the Tetsu arena is completed by a
reflection, not a half turn. Applying the half turn there instead plants a
second village wall across the open sea side, which is the artifact that once
read as "no completion exists" (see
[`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md#the-backdrop-shell-is-drawn-once-so-no-completion-exists)).

#### The choice is authorial, not derivable

Beyond that one constraint the table is hand-maintained per-stage data, and a
viewer that tries to infer it from the mesh will be wrong. 39 backdrop meshes
are carried by more than one PROT entry, byte for byte, and retail's table
splits **12** of those groups across the two transforms.

The clearest case is the Conkram family. `0730_concend` and `0736_conc3` are
identical files - `concend` carries `conc3`'s three stage meshes in reverse
slot order - and the table names `conc3`'s variants while naming none of
`concend`'s. So the same mesh is half-turned in one scene and mirrored in the
other, and the two renders differ visibly in where the colonnade and the
stairs sit around the ring. Neither is a port defect. `conc` and `conc2` take
the mirror alongside `conc3`; `urudre2` takes the half turn alongside
`concend`.

One group is split **inside a single scene**: `0321_balden2` is mirrored and
`0322_balden2` is half-turned on identical bytes. That rules out any per-scene
rule as well as any per-mesh one. It also shows what the choice costs where it
does not matter - that shell's cut section is exactly symmetric in `z`, and a
`z`-symmetric half is carried to the same point set by both transforms, so the
two draws are indistinguishable. Retail can differ freely wherever that holds.

Sweep: `the_second_copy_transform_is_not_a_function_of_the_mesh`.

#### The sibling table at `DAT_80078C1C` - a depth-cue selector, not geometry

`80051c1c`..`80051c6c` scans a **second** zero-terminated `u16` table the same
way and against the same backdrop id, setting a byte flag at `0x8007BDA8`
(`gp + 0xA90`, `gp = 0x8007B318`) instead of touching either actor.

Its 13 ids are the outdoor stages: the three variants of each kingdom
overworld (`map01` / `map02` / `map03`) plus `retona`, `deene`, `kor5` and
`rikuroa`. Note the 7 four-object shells are all inside the overworld nine.

The flag is read twice, both times in `FUN_80050120` and both times on the
**depth-cue** value the two backdrop actors share:

- `800505b8`..`800505c8` picks the ramp ceiling clamped into both actors'
  `+0x78` - `0x800` when clear, `0xC00` when set (either way forced to
  `0x1000` when `ctx+0x278 > 1` or `ctx+0x243 > 1`).
- `800507fc`..`80050834` picks how the far colour at `0x8007BB48` is derived
  from `ctx+0x890` - `>> 1` when clear, `(c - 0x010101) * 2` when set.

So it brightens the far-fog ramp on wide-open stages. It adds no third
geometric behaviour, and the completion is unaffected. Live-confirmed across
15 battle save states: the flag is `1` in exactly the captures whose stage id
is in the table and `0` in every other.

#### Object 1 is dropped

Immediately after allocating the pair, `80051ad4`..`80051bac` decrements each
actor's object count at `**(actor + 0x44)` and left-shifts the pointer array
by one **from index 1** (`A[i] = B[i+1]`, `B[i] = B[i+1]`, `i >= 1`). The
surviving draw list is objects `0, 2, 3, ...`; object 1 stays resident in the
relocated object table, unreferenced. So the 175 two-object stages draw object
0 alone, and the 7 four-object overworld shells draw 0, 2 and 3. For `map01`
that is obj0 = sky (`Y` to `-10522`), obj2 = mountains (`Y` to `-2257`),
obj3 = flat far ground (`Y = 0`, inner radius `2889`); obj1 is a near-detail
prop that never appears on screen.

The whole block is gated on `DAT_8007B64B == 0` (`80051abc` / `80051acc`).
That byte is bit 5 of byte `+8` of the field scene's encounter-region record
(`801DA09C`..`801DA0AC` in the field battle-intro overlay) - the same byte
whose low 5 bits pick which of a scene's stage variants to use. Set, it keeps
object 1.

**A kept object 1 is drawn over the shell, not into it.** Retail has no
depth buffer, so what a kept object covers is its place in the ordering
table. `nivora_duel_pre_megaton_press` (stage `638`, extraction 641, the
region's keep bit set) draws object 1 - twelve quads, the horizon mist
ribbon, a ring of radius about `2330..2580` and height `614` - as five
additive `POLY_FT4` (tpage `0x2D`, CLUT `0x77C1`) in the chain right after
every shell packet of the frame and before the first combatant: floor grid,
shell, ribbon, actors. The ribbon's quads stand partly behind the cave
wall they belong to, and the whole band shows, `60` rows tall on the left
copy and `100` on the mirrored one. Projecting object 1's vertices through
the capture's camera lands on those packets, so its place is the mesh's own
- only the order is special. The port draws the shell and its kept object
as one depth-tested mesh, so the wall hides the part of the ribbon behind
it: on that capture the band is about half as tall and missing from the
mirrored copy's side.

#### Port

`legaia_asset::battle_backdrop` is the shared kernel: `MirrorXTable::from_scus`
parses the table, `drawn_objects_tmd` applies the object-1 drop, and
`SecondCopy::scale` / `flips_winding` give the second copy's transform (both
are exact integer diagonals, so no trigonometry is involved). Mesh side,
`Mesh::append_scaled` / `VramMesh::append_scaled` in `legaia_tmd::mesh` append
the transformed copy and reverse triangle winding when the determinant is
negative - the mesh-level equivalent of retail's mode-word swap. The
asset-viewer PROT browser and the browser entry viewer both place backdrops
this way and label them from the resolved transform.

The rest of the stage scene in `legaia-engine play-window`: the phase-scripted
camera (below), the flat tiled ground grid under the actors (the
`func_0x801d02c0` grid + constant texture address above), a black clear -
the draw environments' background colour is `(0, 0, 0)` in every battle
capture, so a stage shell with no sky panel (a cave, a castle hall) shows black
above it, never a sky (`engine-ui::battle_stage_clear`) - the real
**assembled** battle party (see below),
and animated monsters. Every actor turns by its live facing `+0x46`
(`f / 4096 * TAU` about Y, the direction root motion moves it along) over a
mesh that rests facing `+Z`: a seated monster's `0x800` is the half-turn that
faces the party (the retail Tetsu dialogue close-up shows the monster's face),
and the same rule turns an approaching attacker toward its target and a
fleeing party away from the fight. The actors draw
through the exact `tr.z = 7680` camera with the retail **4× actor world
scale** composed under the rotation (see below) - the battle meshes are small
(party 134–284 units, monsters 77–368), and the 4× base is what makes them
read at retail size against the deep translation.

### Battle camera (exact)

The orbit camera (game mode `_DAT_8007b83c == 0x15`) is pinned exactly from the
four saves + Ghidra. Per-frame `FUN_80026ce4` → `FUN_80026f50` builds the view
matrix via the Euler kernel `FUN_80026988` (cos table `DAT_8007b7f8`, sin table
`_DAT_8007b81c`), composed with the identity base matrix `DAT_80010b84` and
stored at `DAT_8007bf10`; the backdrop + actors then draw through
`func_0x801d02c0`. For a PSX (Y-down) world vertex `v`:

```
screen = H * (R*v + TR) / Ze          R = Rx(pitch) * Ry(yaw)
```

with `pitch = _DAT_8007b790 = 32` (12-bit angle, `4096` = 360°, ≈2.8° down-tilt),
`yaw = _DAT_8007b792` (the orbit azimuth; the battle tick `FUN_801D0748`
decrements it by `DAT_1f800393 * 2` ≈ 4 units per camera step while idle -
one step per 2 vsyncs, i.e. -120 units/s), `roll = 0`,
`TR = (_DAT_800840b8, _DAT_800840bc, _DAT_800840c0) = (0, 1280, 7680)` (eye-space
depth 7680 / height 1280), `H = _DAT_8007b6f4 = 256` (written to the GTE
projection register by `FUN_8003d254`), and the look-at target at the world
origin. The engine mirrors this in `legaia-engine`'s `retail_battle_mvp` as
`Proj_H * T(TR) * R * F` (`F` = the renderer's Y-flip), verified to 0.0002 px
against the hand-rolled projection and against the savestate framebuffer.

These values are **live-confirmed byte-exact** by
[`scripts/pcsx-redux/autorun_battle_render_capture.lua`](../../scripts/pcsx-redux/autorun_battle_render_capture.lua):
run on a real `map01` battle save (reading at the `func_0x801d02c0` grid-render
breakpoint, since at frame 0 the globals hold stale field state) it reports
`mode=0x15 pitch=32 roll=0 TR=(0,1280,7680) H=256`, the grid as **28×28** cells,
the battle actors at scale `+0x72 = 0x1000` (1.0, *not* scaled up - the
on-screen size comes from the mesh, not a scale), and the dome registered at
`DAT_8007C018[2]`.

**Phase-scripted framings + glides.** The projection above is the fixed part;
the *pose* (pitch / yaw / TR) is **phase-scripted with glides**, not a single
orbit. Pinned per-frame from a PCSX-Redux camera trace on the
`s5_tetsu_battle` anchor (logging the rotation trio `0x8007B790` + the
translation trio `0x800840B8` every vsync), cross-checked against the
catalogued mednafen Tetsu battle states; one camera step spans **2 vsyncs**:

| Phase | pitch | yaw | TR | motion |
|---|---|---|---|---|
| tutorial dialogue up | 0 | 0 | `(0, 1280, 1638)`, focus the speaking monster's seat `(0, 800)` | held static |
| dialogue dismiss | 0→32, `+6`/step | orbit resumes | z 1638→7680, `+864`/step | rate-clamped glide |
| Begin/Run menu | 32 | free | `(0, 1280, z)` | idle orbit `-4` yaw/step |
| command submenu | 32 | **2288** | `(-512, 1152, 2457)` | 6-step glide in, then held |
| submenu exit | swings 32→256→32 | eases to 0 | via `(0, 1536, 3276)`, back to menu TR | 6-step swing + 7-step return |
| target cursor (`Auto` / `Command` prompt, cursor `0x5A`) | 256 (one enemy) / 32 (one ally) | `0x800 − bearing(target → focus)` / `0x900 − ally[+0x46]` | `(0, 1536, 3276)` on the member / `(0, height, 2457)` on the ally | 6-step glide, re-armed per cursor move |
| action executing | 0 (or floor-tilted) | `0x800 − facing`, or the drifting `ctx[+0x6DA] − facing` | `(0, height, 0x500)` party / `(0, 0x500, ctx[+0x6D0])` monster | 6-step glide in, 7-step out, then held |

**The target cursor is cases 1 and 3, not the far framing.** The menu
driver `FUN_801D388C` re-arms `FUN_801D5854` on every cursor step against the
cursor's scope: the Attack command's steps `0x0C` / `0x2D` / `0x30` (the
`Auto` / `Command` prompt and the cursor it opens) jump to `0x801D43C0`, case
`1` on the commanding member; the item / magic cursor steps switch on their
scope argument - one enemy case `1`, one party member case `3` on that member
(`lbu a0,0x1dd` at `0x801D43F0`), a whole side cases `4` / `5`, whose
jump-table slots (`0x801CEA00`) land on the case exit and leave the framing
as it stands. Case 1 (`0x801D5A6C..0x801D5B04`) orbits the member at pitch
`0x100`, `TR (0, 0x600, prescale(0x800))`, and turns the yaw to
`0x800 - bearing(target -> live focus)`, the bearing taken from the cursor's
target to the camera's current focus word `-_DAT_80089118/20`; with the
target dead ahead on the seat axis that is yaw `0`, the "submenu exit" swing
the solo-Tetsu trace above measures. Case 3 (`0x801D5BD4..0x801D5C54`) is
case 0 turned onto the ally: `TR.x = 0`, yaw base `0x900`. Engine:
`BattleCamPhase::TargetEnemy` / `TargetAlly` over
`battle_cam_script::CursorFraming`, filled by `engine-core::battle_cam_inputs`
from the live picker for both hosts. No library save state is parked on the
cursor, so the framing rests on the disassembly and the trace's swing pose.

The **step counts are retail's own** `FUN_801D829C` durations, not just trace
readings: the framing cases pass `a3` in *display frames* and a camera step is
two frames, so cases `0`/`1`/`2`/`3`/`6` (`a3 = 0xC`) glide over 6 steps and
case `9` (`a3 = 0xE`, `0x801D712C`) over 7. The `-4`/step idle orbit is likewise
in the disassembly: the action SM subtracts `DAT_1F800393 * 2` from
`_DAT_8007B792` per tick and gates that on `ctx[7]` being `0x00` or `0x0B`
(`0x801E2A3C..0x801E2A6C`), which is what makes "no action executing" the
phase-script condition for the orbit rather than an inference.

**The action framing (`FUN_801D5854` case 6)** is the one the action SM arms at
almost every state, and it forks on `DAT_8007BD71 == 0xFE && slot < 3`
(`0x801D5CEC..0x801D5CFC`). `DAT_8007BD71` is the **battle-end signal**
([battle-action.md](battle-action.md#state-table)):
the `0x5A` wipe scans and the `0x66` escape teardown raise `0xFE`, SCUS
`0x80056014` zeroes it at battle init, and it reads `0xFF` for the whole of a
running fight - twelve battle save states (five Begin/Run prompts, two
mid-strike frames, three mid-approach parks, the arts-input close-up and the
tutorial open) all carry `0xFF`. So **while a fight runs, every action, party
or monster, takes the `0x801D64C4` arm**; the `0x801D5CFC` arm is the
end-of-battle framing. That arm frames from behind the actor (`yaw = 0x800 −
actor[+0x46]`) at a height of `−5 × actor[+0x3E]`, floored at `0x280` with a
quarter of the shortfall added to the pitch so the camera tilts down instead
of sinking (`0x801D6494`), and between the base pose and that floor runs a
per-character script dispatched at `0x801D5D50` (`0x801D5DAC` / `0x801D5FC0`
/ `0x801D61E8` / `0x801D6440`, rejoining at `0x801D645C`) which reads
`actor[+0x1DB]` over the win-pose band `0x11..=0x18` (bias `-0x11`, bound
`8`). The port carries it behind `ActionFraming::battle_over`, raised only by
the battle-end sequence ([the victory camera](battle-round-loop.md#the-victory-camera)). An earlier reading of `0xFE` as "the in-battle state" sent every
party action through this arm - eye `prescale(0x500)` behind the actor, i.e.
inside whichever combatant stood there - and is recorded in
[re-do-not-re-walk.md](../reference/re-do-not-re-walk.md#the-case-6-party-arm-is-the-battle-over-framing).
**Which states hand it the camera is a band, not a byte list.** `FUN_801E295C`
arms per band: the setup band (`0x00`, `0x0B`) arms nothing and runs the
prologue orbit, the seed (`0x0C`) and action (`0x14..=0x48`, less the strike band
`0x1E..=0x20`) bands arm case
`6`, the Run band (`0x64..=0x67`) arms case `9` plus the orbit itself, and the
Done band (`0x50..=0x52`) arms case `6`/`8` **per category** under a
**bounded** tail - retail seeds `ctx[+0x6D8] = 0x3C` in the `0x50` arm and
leaves for `0x5A` when the frame step drives it negative, so the per-action
framing survives ~60 display frames past the strike. `0x3C` is the default,
not the ceiling: a non-zero `ctx[+0x15]` raises the seed to `0x96` (150
frames) at `0x801E5F2C..0x801E5F3C`.

**The Done band's fork is on the action category**, read off the `0x50` arm
(`0x801E5E90..0x801E5EF4`; the `0x51` arm at `0x801E5FC0..0x801E6018` is the
same ladder): `actor[+0x1DE] == 5` (Run) skips the framing call and runs the
yaw orbit instead, `== 3` (Attack) takes `li a1,0x8`, a target that is itself
a party seat whose live HP `+0x14C` reads zero takes `0x8` too, and everything
else `li a1,0x6`. The seat test reads the spill `sp+0x20`, which the prologue
fills from `lbu t2,0x1dd(s3)` (`0x801E29B0`) - the **target** index, beside
`s8`, the target actor - not the acting seat `ctx[+0x13]`. A party caster whose
spell killed a monster therefore keeps case 6 on itself through the tail
(`shiny_refactor_gimard_levelup`: target slot `3` at `0` HP, step-table
endpoints case 6's). Two captures pin the two sides:
`zora_glare_petrify_post` (`ctx[7] == 0x51` after a monster's spell) reads
pitch `0`, `TR (0, 1275, 4820)` - one tween step short of case 6's
`(0, 0x500, prescale(0xC00) = 4915)` - with the focus on the caster's own seat
and the yaw `ctx[+0x6DA] − actor[+0x46]`; `evil_medallion_rage_battle`
(`ctx[7] == 0x0A`, flow byte `0xFF`, between actions) reads case 9's far
framing over its `±825` seats. So the tail is filmed by the per-action
framing and the end-of-action gate `0x5A` is where the far framing takes
over. Engine: `battle_cam_script::done_band_phase` over `DoneBandInputs`,
which both hosts fill from the acting actor; the far framing at a collapsed
formation's `0x800` floor (`z = 3276`) is a closer shot than the in-fight
arm's `4915`, which is what the port's earlier "Done band is idle" reading
showed as a torso close-up after every strike. Guards:
`the_done_band_owns_the_action_framing_until_end_of_action`,
`a_monster_spell_done_tail_reads_the_zora_capture` and
`a_real_turn_films_its_done_tail_and_hands_back_at_end_of_action`.

**The idle orbit has two writers, and they never add up.** Besides the
action SM's prologue store (gated on `ctx[7]` being `0x00` / `0x0B`), the
battle tick `FUN_801D0748` carries the same `yaw -= DAT_1F800393 * 2` store
in its own prologue (`0x801D07AC..0x801D07CC`), gated on the command-flow
byte `ctx[+6]` being `0x1E` / `0x32` / `0x6E` / `0xFE` - the Begin/Run prompt
among them. A 240-vsync PCSX-Redux trace parked at the `battle_gaza2_prompt`
state (`scripts/pcsx-redux/autorun_battle_cam_orbit.lua`, Exec breakpoints on
both stores) counts the dispatcher's store once per battle tick and the SM's
never - the action SM is not run while the flow byte owns the frame - with
the yaw stepping `−2 × frame_step` each time. The prompt therefore orbits at
`−2` per display frame, the `−4` per camera step the port runs, from either
writer alone.

**Case 6 is re-armed every pass, so the framing chases the actor.** Each of
the action states calls `FUN_801D5854(actor, 6)` before it does anything else
- `0x0C`, `0x14`..`0x19`, `0x32`, `0x37`, `0x3C`..`0x40`, `0x46`,
`0x47` (the strike band `0x1E`..`0x20` arms cases 7 / 8 instead,
[below](#the-post-strike-two-shot-fun_801d5854-cases-7-and-8)) - so the three tween-target vectors are rebuilt out of the live actor
record each display frame and `FUN_801D829C` re-emits the step table. The
target is not frozen at the state change, and the difference is not cosmetic:
`0x14` stages the approach walk and `0x19` runs it, so a party member crosses
most of the gap to its target before the swing. A focus pinned to the vacated
seat frames bare ground - at the close-up depth `prescale(0x500)` = 2048
against 4x-scaled stage coordinates the whole formation leaves the frustum,
several combatants behind the eye. One visible consequence: the in-fight
arm's yaw follows the live `ctx[+0x6DA]` drift instead of freezing on its
value at the phase change.

**The re-arm makes it an ease-out, not a glide.** Each rebuild takes the gap
as it stands and divides it by `a3 = 0xC` again, and the walker task
`FUN_8002149C` adds `increment * frame_step` before the next pass rebuilds it -
so a pass covers about a sixth of what remains (at the 30 Hz tick), and the
camera is still closing in long after the twelfth frame rather than landing
there. Retail's step table at `ctx[+0x118C]` pins it in two captures:
`nivora_duel_mid_blazing_slash` reads yaw / TR z increments `59` / `55` with
`589` / `547` to go, `battle_noa_miracle_art_combo` `66` / `86` with `587` /
`772` - each exactly `ceil((rem + frame_step * step) / 0xC)`, a table one pass
old with one walk applied. A drifting yaw therefore trails its counter by a
few units for as long as the gap stays under `0xC`, where the increment
equals the counter's own two units a pass - the eight units the `0x19` parks
read. Cases 7 and 8 are re-armed the same way on the same `a3`.
`BattleCamera::retarget_action_glide` / `retarget_post_action_glide` are the
port's re-arms, each a `Glide::chase` over `0xC` frames; the port had carried
the armed segment's remaining step count over instead, which landed every
framing linearly at step 6.

The in-fight arm (`0x801D64C4`) frames on the live position `actor[+0x34/+0x38]`
with the focus height left at the stage floor, pitch `0`, `TR = (0, 0x500,
ctx[+0x6D0])` - the depth `FUN_801F0348` derives from the framed monster's
size class - and `yaw = ctx[+0x6DA] − actor[+0x46]`, with a style byte
`ctx[+0xD]` selecting three tweaks and character id `4` overriding the whole
translation. Three PCSX-Redux captures parked in `ctx[7] == 0x19` with Gaza
acting pin it byte-exact: `TR (0, 1280, 5324)` from `ctx[+0x6D0] = 0xD00`, the
focus trio the negated `+0x34/+0x38` pair, the `ctx[+0xD] == 2` capture at
pitch `0x80` over `TR.y = 0x400`, and the live yaw eight units behind the
counter - the per-pass re-arm chases it. `ctx[+0x6DA]` is a **per-action
ladder**, not a free drift from battle entry: the `0x00` round-begin arm
zeroes it (`0x801E2B40`), the `0x0C` seed arm stores `0x800` (`0x801E2CF8`),
the seed's Attack branch stores `0x200` as it enters `0x14` (`0x801E2F20`),
and a **party** attacker's first swing-clip commit re-seeds `(rand() % 2) ×
0x800 + 0x280` with `ctx[+0xD] = 0` (`FUN_8004E13C` `0x8004E288..0x8004E2B4`,
from the anim commit `FUN_8004AD80` at `0x8004BE28`, gated on the clip header
byte `+0x87 == 2`, the previous commit's not, and `ctx[+0x13] < 3`); on top of
that the SM's prologue adds `max(1, 4 × frame_step / 3)` per pass
(`0x801E29E4..0x801E2A24`), about one unit per display frame. A monster's
melee is therefore filmed from the `0x200` base and a party member's from
`0x280` or `0xA80` - a three-quarter view that keeps both combatants in frame
- while a spell or item keeps the seed's `0x800`. The
`battle_melee_hit_spark` capture reads `0x298`, `0x280` plus 24 frames. Engine:
`BattleCamera::observe_action_state` applies the ladder on the action-state
edges, standing the swing-clip commit in with the edge into `0x1E`.

The style byte itself reaches all three framings live. Both hosts read
`World::battle_ctx.camera_variant` into `ActionFraming::style` - the native
window's `battle_action_framing` and the browser page's
`play_battle_render`'s `BattleCamInputs` builder - where each of them used to
pass a hard-coded `0`, which pinned every action to variant `0` of four. The
action SM writes the byte at its seed and narrows it per category arm, per
[`ctx[+0xD]`](battle-action.md#ctx0xd---the-per-action-camera-angle-variant).
The commit's own `ctx[+0xD] = 0` is the one part still standing in as a latch
on `BattleCamera`, because the host re-supplies the framing inputs every frame
and a local write would not survive the next one.

**The framing-case table.** `FUN_801D5854`'s mode argument indexes a
ten-entry jump table at `0x801CEA00` (PROT 0898 file `0x1E8`), and modes `4`
and `5` are the same no-op tail slot:

| Mode | Entry | Framing | Focus |
|---|---|---|---|
| `0` | `0x801D59E0` | arts / spell / item **input** close-up | acting actor |
| `1` | `0x801D5A6C` | submenu-exit swing | acting actor |
| `2` / `3` | `0x801D5BB0` / `0x801D5BD4` | menu-driver transitions | acting actor |
| `4` / `5` | `0x801D7138` | nothing - straight to the shared tail | - |
| `6` | `0x801D5CE8` | per-action framing (in-fight arm; the battle-over arm only under `DAT_8007BD71 == 0xFE`) | acting actor |
| `7` | `0x801D65DC` | post-strike **two-shot** | attacker-target **midpoint** |
| `8` | `0x801D67D0` | end-of-action | the target |
| `9` | `0x801D6EF4` | far Begin/Run framing | formation centre |

### The battle frame step is the frame's own cost

Every per-frame battle path - the camera walker `FUN_8002149C`, the effect
waits, the root-motion term - scales by the frame step `DAT_1F800393`, and
the frame driver `FUN_80016B6C` rebuilds that byte every frame from what the
frame cost. The newest frame time (`FUN_800173BC`'s `VSync(1)` hblank
count) goes into a sixteen-entry ring at `0x80084098`, index `gp+0x440`,
clamped to `0x2BC` at `0x2D0` or more (`0x80017098..0x800170CC`); the step
is the ring's maximum against `0xF1` / `0x1FF` / `0x2D1` - `1` / `2` / `3`,
else `4` (`0x80017108..0x8001715C`) - raised to the floor `0x8007B9D8`
(`0x80017170..0x80017198`). Only mode word `gp+0x4CE == 0x10` measures; a
non-zero `gp+0x5D8` forces the step instead.

The battle's floor is `1` in every catalogued battle state, so its step is
the load alone and is not a property of the game state. Across the
battle captures the ring reads `2` on about three in five; summon close-ups,
module casts, the arts input and the Spirit-heavy Delilas fights read `3`
(`theeder_summon_mid_cast` peaks at `625`, `gimard_burning_attack` sits at
the `700` clamp), and two light casts and the battle-loading frame read
`1`. A capture recovers only its newest sixteen frames this way:
`gp+0x480` counts frames and the libetc vsync count `0x8007A894` counts
vsyncs, but the frame counter restarts somewhere no `SCUS_942.54` store
shows (only its increment at `0x80016BA8` is there) and nothing records the
vsync count beside it, so the steps of a whole fight are lost. Two captures of
one session show how mixed they are: `player_steal_skeleton_pre` to
`_banner` is `304` vsyncs over `115` frames.

The engine ticks once a vsync, which keeps every per-vsync rate whatever
the step; the step only places the frame boundaries. `BattleFrameClock`
groups the ticks into frames - two vsyncs by default - for the root-motion
carry and the camera (`BattleCamera::set_frame_step`). The camera keeps
every tween's length in display frames whatever the step: a step fires
every `step` vsyncs, the walker scales each increment by it, a tween armed
over `a3` frames lands in `a3 / step` steps (`frames_to_steps`, the module
and spell-cast shots; `steps_of` for the glides authored in default-step
steps), and the idle orbit turns `2 * step` a step. Play keeps the
default; a replay that knows retail's step installs it while the action SM
sits in one state (`World::seed_battle_frame_step`). The retail-compare
drive does so for a replayed cast caught in the summon close-up `0x33` /
`0x34`, where each pass re-arms case `0x12`'s three-frame tween: at step
`3` the walker lands it every pass, at `2` it trails
([retail-compare](../tooling/retail-compare.md#an-ease-out-camera-carries-its-history)).

The cast modules keep their per-vsync passes. Run once a battle frame
with every drain, drift and ramp scaled by the step - retail's own shape -
they measured worse on the step-`3` captures than the per-vsync passes,
which run the same rates: `nova_summon_mid_cast` `image` `.963` to `.717`
(its flash fade spawned up to two vsyncs later), `gimard_burning_attack`
`camera` `.965` to `.932`. The captures these modules reach are placed by
the module's own arm and countdown
([retail-compare](../tooling/retail-compare.md#driving-to-the-phase)), so
quantising the arms to frame boundaries moves only what spawns on them.

### The post-strike two-shot (`FUN_801D5854` cases 7 and 8)

Case `7` is the only framing in the set that orbits **both** combatants. Its
base (`0x801D65DC..0x801D6694`) is pitch `0`, yaw `ctx[+0x6DA] - actor[+0x46]`,
`TR = (0, 0x500, ctx[+0x6D0])` and a focus at the midpoint of the acting actor
and its target (`actor[+0x1DD]` through the actor table `0x801C9370`), each
component `(a + b) >> 1` and negated. Then the shared `ctx[+0xD]` style fork
(`1`/`3` add half a turn, `2`/`3` drop `TR.y` to `0x400` and tilt the pitch by
`0x80`), a **one-way** yaw unwrap at `0x801D6700` - `yaw = (yaw - 0x700) &
0xFFF`, plus a full turn when that lands below the live `_DAT_8007B792`, so the
swing never takes the short arc back - and a "pull in" tweak at `0x801D6780`
(pitch levelled, `TR.y += 0x40`, `TR.z = 3z/5`) gated on `_DAT_800846C0 == 0`
and the acting actor's anim state.

Case `8` is the same shape aimed at the target alone: an extra `-0x100` on the
yaw base, `focus.y` forced to the stage floor, a `-0x600` unwrap, and a focus
fork that falls back to the acting actor when `actor[+0x1DD] >= 8` or the
target's node is dead (`0x801D6870`).

#### The death re-frame and its `ctx[+0x270]` ramp

Case 8's tail from `0x801D69A8` forks on the framed target's live-HP halfword
`+0x14C`. The **dead** arm (`0x801D6A20`) is the death re-frame, and it is
ported: `battle_cam_script::apply_death_reframe`, applied by
`BattleCamera::action_end_pose` whenever the post-action target reads dead.

Three literals land unconditionally at `0x801D6AF8` - `TR.y = 0x300`,
`pitch = 0x140`, `TR.z = ctx[+0x6D0]` - and the per-action yaw ladder is zeroed
beside them (`sh zero,0x4(t0)`, `t0 = ctx + 0x6D6`, so the store is
`ctx[+0x6DA]`), which is why a death shot does not inherit the swing's orbit.
Then the fork on the target's own anchor height `+0x36` (`lh v0,0x36(v0)` at
`0x801D6B38`), the Y of the same world triple case 7 takes its focus midpoint
from:

| target `+0x36` | pose | `ctx[+0x270]` |
|---|---|---|
| `0` (body on the stage floor) | the three literals above, unchanged | re-zeroed (`sb zero,0x270(a0)`, `0x801D6B4C`) |
| non-zero (still falling) | `TR.z = ctx[+0x6D0] - 4r`, `TR.y = 0x300 - r`, `pitch = 0x180 - (3r >> 1)` | left to ramp |

`r` is `ctx[+0x270]`, the second byte ramp `FUN_801D5854`'s own prologue
advances beside `ctx[+0x26E]` on every call - same `8 x frame_step` increment,
same `0xC8` ceiling (`0x801D5960..0x801D59B8`), and no per-action reset, so a
fight's second death reads a ramp already at the cap. At the cap the re-frame
is `TR.z - 0x320`, `TR.y = 0x238`, `pitch = 0x54`: the camera drops, levels off
and pushes in on the falling body, then snaps to the flat pose the frame the
body lands. Engine side the ramp lives on
`battle_attack_camera::AttackCamCtx::death_ramp`.

The lone-monster defeat fork above it (`ctx[+0x287]` / `ctx[+0x288]` /
`_DAT_8007BD0D` at `0x801D6AC8..0x801D6AF0`; `+0x288` is the defeat-fade latch
[`battle-action.md`](battle-action-helpers.md#ctx0x287-is-the-scripted-fight-flag-and-0x288-is-the-lone-monster-defeat-latch)
documents) sends a scripted fight's lone monster, dying in place, to the same
stand-off arm as a gone node (`PostActionTarget::lone_defeat`). Case 8's focus fork tests the target's
node word `+0x4` (`0x801D682C`), not its HP, so a target killed but still drawn
stays framed; both cases read the body pair `+0x3C` / `+0x40` for X / Z (the
live `+0x36` for case 7's Y), not the live pair case 6 reads.

#### The live-target arm (`0x801D6BFC`)

A target still standing takes its own re-aim, sized by how it stands:

| target | `TR.z` (raw) | `TR.y` | floor |
|---|---|---|---|
| party seat, animating (`+0x1D9 != 0`) | `0x600` | `-4y` level, `-7y/2` tilted | `0x280` |
| party seat, idle | `0x600` | `height[char] - 0x140` level, `- 0xC0` tilted | `0x280` |
| monster, animating (or `_DAT_8007BD84` set) | by formation id: `0xB4` `9z/10`, `0xA2` / `0xA7` `z`, `0x1F..=0x21` `8z/10`, else `7z/10` | `-7y/2` level, `-3y` tilted | `0x300` |
| monster, idle | as above | the live camera's `TR.y` and pitch, held | - |

`y` is the target's display height `+0x3E`, `z` is `ctx[+0x6D0]`, tilted
is a non-zero staged pitch (the style-2/3 tweak), and a floor raises `TR.y` to
itself while adding a quarter of the shortfall to the pitch. The
`battle_melee_hit_spark` capture (a swing on a monster held on its knockdown)
reads it: tween target `TR.z` `prescale(0x866)` = `3440` exactly, `TR.y`
within a few units of `-7y/2`. Engine: `battle_cam_script::apply_live_target_reframe`.

Which states arm them is `FUN_801E295C`'s own fork, not an inference. The
strike loop `0x1E` (jump-table entry `0x801E35F0`) arms mode `7` on every
pass with no fork (`li a1,0x7` at `0x801E36EC`). The recovery wait `0x1F`
(`0x801E3A88`) and the return `0x20` (`0x801E54EC`, fork at
`0x801E5660..0x801E56C0`) default to `li a1,0x7` and take mode `8` when the
target's current anim `+0x1D9` is its knockdown `+0x1F1` or its non-zero
get-up `+0x1F2`; `0x20` alone also takes `8` when a party slot faces a
target in a death clip (anim `7` / `8`). `player_steal_skeleton_pre`
(`0x1E`) reads case 7's tween targets, `player_steal_skeleton_banner` (`0x20`,
the skeleton on its knockdown at HP `0`) case 8's death re-frame at the ramp
cap.

The per-art attack camera `FUN_801D71B8` hangs off `FUN_801D5854`'s shared
tail, so it overrides cases 7 and 8 as it does case 6 - which matters now
that the strike loop, where most art swings are filmed, is case 7. Its first
test is the target's live HP (`0x801D71E8..0x801D7208`), so a death re-frame
is never overridden. The Done cleanup (`0x50`) forks on the action category at
`0x801E5FC0..0x801E6018` - `actor[+0x1DE] == 3` (Attack) and "a party-seat
target whose live-HP halfword reached zero" branch to `li a1,0x8`, everything else
to `li a1,0x6` - and `0x52` / `0xFD` arm `8` unconditionally (`0x801E5F74`).

Engine side: `battle_cam_script::recover_framing` / `action_end_framing`, armed
by the `Recover` / `ActionEnd` phases (`post_strike_phase` runs the fork, on
`World::battle_on_knockdown` / `battle_current_anim`). `0x51` is deliberately left idle - see
the Done-band note above; the port's residency there is unbounded where
retail's is `ctx[+0x6D8] = 0x3C` frames.

### The Battle Camera option calms the action shots

The options screen's Battle Camera row (Close / Normal / Far, config word
`_DAT_800846C0`, [field-menu](field-menu.md)) is not a distance. Retail reads
it in four places, each making the action shots less dynamic as it rises:

| Site | Close (`0`) | Normal (`1`) | Far (`2`) |
|---|---|---|---|
| action SM prologue `0x801E29D4`: `ctx[+0x6DA]` drift and the setup-band orbit | run | run | skipped |
| case 7 pull-in `0x801D6724` | allowed | - | - |
| case 8 `0x801D6958..0x801D69EC`: yaw | built | built | the live `_DAT_8007B792` |
| case 8's dead- / live-target arms | run | skipped | skipped |
| `FUN_801D71B8` call `0x801D7138` | run | run | skipped |

Case 8 keeps its arms on Normal / Far in a scripted fight (`ctx[+0x287]`) once
the results phase word `_DAT_8007BD2C` is non-zero; the port does not carry
that exception, nor the skipped setup-band orbit store (the port runs one
orbit for both of retail's writers). Engine: `World::toggles.battle_camera`, pushed by
`OptionsState::apply_to_world` on both hosts, read through
`BattleCamera::set_camera_option`.

### The round prompt is the far framing, a member's surfaces are the close-up

"A battle menu is open" does not select the close-up. The battle menu driver
`FUN_801D388C` arms **both** cases: `0x801D475C` / `0x801D53B8` pass `a1 = 0`
and `0x801D4908` / `0x801D5688` pass `a1 = 9`, and the battle tick
`FUN_801D0748` arms case `9` itself at `0x801D0E98`. Which surface takes which
is read off the library's battle captures, keyed on the command-flow byte
`ctx[+0x06]`, framebuffer and RAM together:

| `ctx[+0x06]` | Surface | Framing every capture reads |
|---|---|---|
| `0x1E` | the round's **Begin / Run** prompt | case 9: `pitch 32`, `TR (0, 1280, max(span * 3, 0x800))` prescaled, focus at the formation centre - both rows in frame |
| `0x28` | the member's command **ring** | case 0: `pitch 32`, `TR (-512, height[char], 2457)`, `yaw = 0x8F0 - actor[+0x46]`, focus on the member |
| `0x50` | the member's **arts input** | case 0, the same pose |
| `0x6E` | the commit confirm | case 9 again, after the submenu-exit swing |

The case-0 captures span Vahn, Noa and Gala and two Delilas fighters on
seats 0, 1 and 2, each on its own per-character height (`1152`, `960`,
`1408`) and its own seat as focus, so the pose is the member's, not a seat
constant. So the close-up belongs to the member - from the ring through the
pickers it opens (the item and magic windows `0x3C` / `0x46` are the same
member's) - and only the party-wide prompt films the formation. A host that
keeps the far framing on the ring films the whole command phase from the
wrong place; one that folds the round prompt into the close-up puts the
opponent behind the eye. Engine side: `battle_cam_inputs`'s
`member_surface_open`, which keys the phase on the ring session as well as
on the submenu sessions.

**The close-up follows the ring from member to member.** A commit lands on
the next member's ring (`0x28`) or on the commit confirm (`0x6E`), never on
the round prompt, so between two members the camera never passes through
the far framing - the phase is case 0 on both sides of the hand-off. What
moves it is the menu driver re-arming case 0 with `a0 = ctx[+0x13]`, the
member now commanding (`lbu a0,0x2(s4)` with `s4 = ctx + 0x11`, at
`0x801D4758` and `0x801D53B4`), over the case's own 6-step glide. A port that
re-arms only on a phase change keeps the first member framed while the rest
of the party chooses. Engine side: `BattleCamera::set_actor` re-arms the
close-up when the actor changes under it; the item window, which carries no
member of its own, frames the member whose ring opened it.

### Case 9 is re-derived every pass, so the depth follows the formation

The far framing is not armed once and left. `FUN_801D0748` re-arms it per tick
and the menu driver re-arms it on its own transitions, so `max(span * 3,
0x800)` and the bbox centre are rebuilt out of the live actor table - exactly
like case 6. This matters because the formation *moves*: an attacker walks most
of the way to its target during the approach, collapsing the span onto the
`0x800` floor. A depth frozen at the moment the far framing was armed survives
the actor walking back to its seat, leaving the eye at `prescale(0x800)`
against a full-width formation with one combatant filling the frame and the
other behind it. Engine side: `BattleCamera::retarget_menu_glide`, which skips
only the two segments that are not "walk to the far framing" (the rate-clamped
dialogue dismiss and the scripted submenu-exit swing).

### The resting yaw is the orbit, and battle init zeroes it

`_DAT_8007B790/92/94` is **one** rotation trio, shared by the field and battle
cameras, and battle init `FUN_80055B6C` overwrites it: pitch `0x3C`, yaw and
roll `0` (`sh zero,-0x486e(at)` at `0x80055E84` is the yaw), TR
`(0, 0x500, 0x1C00)` (`0x80055E50..0x80055E90`). From there the yaw is a
clock - the entry sweep leaves it alone ([below](#the-battle-entry-sweep)),
case 9 passes it straight through and the battle tick only decrements it - so
the five battle save states caught at the identical far framing
(`ctx[7] == 0x00`, pitch `32`, `TR (0, 1280, 7680)`, focus at the origin,
`+-800` seats) read five different yaws (`224`, `2632`, `3136`, `3808`,
`3882`) because they were taken at five different times, and no captured value
is *the* resting yaw. At yaw `0` the eye looks straight down the seat axis and
the two rows project to the same screen X, each occluding the other; retail
opens every fight there and orbits out of it. The port does not: it opens on
the azimuth the field camera left, moved off the seat axis by
`battle_entry_yaw` - a port judgement, carried by
`BattleCamInputs::entry_yaw`, which both hosts feed
`World::locomotion.camera_azimuth`.

### The battle-entry sweep

The SCUS frame driver `FUN_80046A20` owns the camera before the battle tick
does. Its entry counter `gp+0x330` (`0x8007B648`) counts the load up to `0x80`
and then, advancing by the frame step `0x1F800393` a pass, runs the sweep
(`0x80046EEC..0x8004700C`):

| Counter | Camera |
|---|---|
| `0x80..0xA1` | TR y `+= 0x30 * fs`, TR z `-= 0x40 * fs` from battle init's pose: the camera rises and pulls in |
| `0xA2..=0xC0` | `FUN_801D5854(0, 2)` every pass - case 2's pitch `0`, yaw `0`, TR `(0, 0x600, 0x700)` on the origin, `a3 = 0xC`, re-armed each pass |
| past `0xC0` | parked at `0xFF`; the battle tick `FUN_801D0748` runs from then on |

Two captures of the sparring fight's entry pin it: `v0_1_battle_loading_tetsu`
(counter `0x84`) reads pitch `60`, `TR (0, 1472, 6912)` - four frames of
drift from `(0, 1280, 7168)` - with an empty step table, and
`s5_tetsu_battle` (`0xAF`) reads pitch `16`, `TR (0, 2010, 3552)` under a
case-2 step table whose endpoints are `(0, 1536, 2867)`. The battle tick's
first framing takes over from wherever the sweep leaves the camera: the
tutorial cuts to its dialogue close-up, any other fight re-arms case 9's far
framing. Engine: `BattleCamera::start_entry_sweep`, armed on a fight's first
camera frame (`BattleCamInputs::entry_sweep`, which the live world sets); the
port's battle tick does not wait for it, so the round prompt opens under the
sweep, and a command surface or an action that opens under it (a fight that
auto-acts on load) ends it early: retail cannot open one there at all. The
enemy-name intro keeps retail's order: its labels and their `ctx[+0x6D6]`
hold belong to the battle tick's flow `0x0A` / `0x0B`, which the frame
driver reaches only past the sweep (`0x80046EF8` / `0x80047014`), so the
port neither shows nor drains them until the sweep is over
(`World::battle_entry_sweeping`).

**The per-art attack camera is an override, not a fold.** `FUN_801D71B8` is
*not* part of case 6. Its only call site is `FUN_801D5854`'s shared tail
(`0x801D7180`), which runs after whichever framing case has already handed its
pose to the tween builder, and is gated on `_DAT_800846C0 != 2` and the acting
actor's `+0x1DD < 8` (`0x801D7138..0x801D7178`). The routine then seeds a
**fresh** pose from the actor - pitch `0`, yaw `−actor[+0x46]`,
`TR = (0, 0x400, 0x400)` (`0x600` height for character `3`), look-at the negated
actor position - runs a per-character / per-art arm over the second band
`0x1A..=0x2D`, and calls the *same* tween builder again with its own much
shorter duration (`1`, `3` or `6` display frames against case 6's `0xC`).
Whichever call ran last owns the step table that frame, and this one runs last;
an art id with no arm returns without arming anything and case 6's framing
stands. Both the seed depth (`0x400`) and the arms' folds make the swing
close-up **tighter** than case 6's `0x500`, ramping as `ctx[+0x26E]` climbs.

The arm's offsets come from the disc table
[`battle-attack-camera-table.md`](../formats/battle-attack-camera-table.md),
whose two columns are a per-action coin flip rather than two swing phases; that
page carries the row map, the ramp counters and the `actor[+0x1DB]` id space.
Engine side: `legaia_engine_vm::battle_attack_camera` runs the thirteen arm
bodies and owns the ramp quartet; `battle_cam_script`'s Action phase steps the
live pose toward whatever the arms produce, each frame, on the arm's own
duration - which is what retail's per-frame rebuild of the step table amounts
to. Both hosts feed it the same three per-actor channels (`actor[+0x1DB]` as
`BattleActor::latched_anim`, `actor[+0x21B]` as `hit_count_bound`, and
`actor[+0x22C][+0x68]` from the battle animation player's cursor, `<< 4` into
retail's sixteenths).

`H = 256` and the identity·16384 base hold through every phase. The traced
numbers above are one fight's *instance* of two formulas, not constants: the
submenu yaw `2288` is `0x8F0 - actor_facing` and the menu depth `z` is the
formation-sized `max(span * 3, 0x800)`, which lands on `7680` for the solo
Tetsu seats. Per-seat variation lives in the **focus trio**, which a solo
trace cannot distinguish from a constant. Both framing laws, the per-character
height table `0x801F4D2C`, and the focus trio are covered under
[`battle-action.md`](battle-action-helpers.md#case-0---the-submenu-close-up-framing).
Engine mirror: the phase script lives ONCE, in
`legaia_engine_vm::battle_cam_script` (phases, poses, glides, plus
`battle_vp` - the retail GTE view-projection as one matrix), and so do its
inputs and its state: `engine-core::battle_cam_inputs` derives phase / acting
actor / formation / framing context from the live world, and
`World::tick_battle_camera` steps the camera from `World::tick` on the retail
display-frame clock, holding it in `BattleState::camera`. The native
`play-window` and the browser play page (`play_battle_camera_vp`) only read
the pose (`World::battle_cam_pose`), so neither host's render build can gate
the step. The glide-table kernel port stays at
`legaia_engine_vm::battle_camera` (`FUN_801D829C`); the recipe tests in each
host pin the shared derivation to the same literal pose.

**Screen shake.** `FUN_801D9D30` jitters the same translation pair
(`0x800840B8/BC`) by two LCG samples masked to `0xFFFFFF >> (0x15 − amplitude)`,
where the amplitude is `_DAT_8007B630`. That global has exactly one retail
non-zero writer (the scene reset `FUN_8003A024` zeroes it) - the field-VM opcode `0x4C` outer-nibble `8` sub-`4`
(`[4C, 84, amplitude]`, arm `0x801E2134`, jump-table slot `0x801CEF58`) - and
`FUN_801D9D30`'s only callers are the field-family overlay's per-frame camera
updaters (`0x801D1344` and siblings), so in retail the shake is a *field*
effect and no caller is resident during a fight. The port models the opcode
(`FieldHost::op4c_n8_sub4_set_b630` → `World::camera.shake_amplitude`) and
steps the kernel from the shared battle camera, which owns the same
translation pair. The offset is held beside the framing pose rather than
inside it, so a live shake cannot stall a rate-clamped glide.

**Actor pass: the 4× world-scale base matrix.** The battle base matrix
`DAT_8007BF10` holds `16384 * I` (GTE `4096` = 1.0 → a **4.0× uniform
scale**), in RAM across every catalogued battle savestate and at every orbit
angle (a pure diagonal at all four yaws, so it is a *base*, not the composed
rotation - the composed view matrix lives in GTE scratch `0x1F8003C8`). The
actor render `FUN_80048A08` multiplies that camera matrix per actor
(`FUN_8005B3A8(&DAT_1f8003c8, ...)` with the actor's `+0x24` rotation trio,
GTE TR from the actor's `+0x2C` view-translation trio), so the actors - and
their stage translations - draw at 4× under the same `Rx(32)·Ry(yaw)` /
`TR=(0,1280,7680)` / `H=256` camera the backdrop uses at 1×. The 4× is what
makes the small battle meshes read at retail size against the deep
translation (`256 * 4*370 / 7680` ≈ 49 px for a 370-unit monster).

**Every battle draw class rides that scale in the port**, not just the
combatants - the backdrop is registered as an ordinary background actor
(`FUN_800513F0` → `FUN_80020de0` alloc → the normal actor path), so it goes
through the same `FUN_80048A08` composition. The port therefore lifts the
arena and the ground grid with the same `BATTLE_WORLD_SCALE = 4.0`
(`PlayWindowApp::battle_stage_model` natively, `BattleMesh::stage_positions`
in the browser upload). The grid's DPCS ramp window is **not** scaled with
them: it is keyed on the vertex's view depth `SZ`, and the scale is a model
transform under a camera whose translation trio is already in view units, so
the port's fragment depth is retail's `SZ` as-is
([the grid's near colour and cue depth](#the-grids-near-colour-and-cue-depth)).

The camera's translation trio is authored in this scaled space: the traced
far framing's `TR.z = 7680` is the eye distance to a formation whose seats
are `±800` **before** the scale. Leaving a draw class at raw 1× under that
trio has two consequences, and the port shipped both. The eye orbits that
class at four times the intended radius - clear of the arena on one side and
straight through its shell on the other, so the frame fills with a single
magnified wall - and every actor draws `3 × seat` away from the ground cell
it stands on. Neither is visible at the far framing on a centred formation,
because a focus at the origin makes the two classes coincide: that is the one
configuration the pose tests and `retail_battle_mvp` sample. Guard:
`the_ground_under_an_actor_projects_under_the_actor` projects each retail
seat through both classes at every framing and requires the same pixel.

The function that camera comes from is **`battle_dome_camera_mvp`**, not
`retail_battle_mvp`. The two are not interchangeable and only one is live:

| | `retail_battle_mvp` | `battle_dome_camera_mvp` |
|---|---|---|
| Pose | the fixed `TR = (0, 1280, 7680)` | the live phase-scripted pose |
| Role | camera-RE reference + regression target | every battle draw |
| Reached by | nothing (`#[allow(dead_code)]`) | the play-window battle path |

`retail_battle_mvp` pins the *static* composition to 0.0002 px against the
savestate framebuffer, which is what makes it the regression target; it holds
the backdrop's own translation fixed, so it cannot express the phase glides the
[camera section](#battle-camera-exact) traces. `battle_dome_camera_mvp` takes pitch /
yaw / TR / focus from the live `battle_cam` pose each frame and falls back to
the far framing at its minimum depth on the first frame. Both build on the
shared `battle_mvp_with_tr`, which is why the pinned projection stays a valid
oracle for the live one.

Note also that the 4× is sourced from `DAT_8007BF10 = 16384 * I` - the actor
pass's base matrix - and not from the actor field `+0x78`, which
`FUN_8001ADA4` passes as `FUN_80043390`'s IR0 depth-cue argument
([`renderer.md`](renderer.md)). Two different quantities; reading `+0x78` as
the world scale would be a different claim with different evidence.

## Field-to-battle intro presentation

The transition between leaving the field and the battle scene coming up is its
own overlay, PROT 0979 `field_battle_intro`. It does two jobs at once:
sequence the battle handoff, and drive one of five visual styles.

The **handoff** half is live. `FUN_801CF5BC` is ported as
`engine-vm::battle_intro_transition::tick_transition` and driven once per frame
by `World::tick_battle_intro` for as long as the encounter session sits in its
`Transition` phase. Phase 7 is terminal: it raises `ready` bit 1 and stops
advancing, and bit 0 comes from the post-switch spin test, so `ready == 3` is
the completion state.

**Every battle entry rides this phase**, not just the field step roll: a
scripted carrier fight (the op-`0x3E FF` / dialogue-engage path,
`World::begin_field_carrier_battle`) and a world-map contact
(`World::begin_world_map_encounter`) both arm the same session `Transition`
instead of flipping the mode on the spot - retail runs the intro overlay for
all of them. A scene with no session of its own (towns, the overworld) gets a
bare bracket installed on demand (`World::install_encounter_bracket`), a
post-battle `Grace` window never swallows a story fight (it is reset before
arming), and the drain into the actual entry runs in every relevant mode:
`live_field_tick` under the live loop, a live-loop-off arm of the `Field`
tick (`--no-live-loop` gates the roll, never an armed fight), and the
`WorldMap` tick (which drains into `World::enter_world_map_battle`).

The **visual** half is live end to end, on both hosts. The five style kernels
are ported in `engine-vm` and drawn by `engine-ui::battle_intro` (re-exported
at its old `engine-render::battle_intro` path), the per-frame working-set
owner the native play window **and** the browser play page each arm for every
encounter - the hosts differ only in how the captured field frame is read back
(see [`host-drift.md`](../tooling/host-drift.md#screen-space-psx-primitives-across-the-two-hosts)):

| Style | Retail tick | Simulation port | Packet builder port |
|---|---|---|---|
| Scatter particles | `FUN_801CFDA0` | `battle_intro_styles::tick_particle_field` (`PARTICLE_TICK_A`) | `battle_intro::emit_particle_field` |
| Scatter with spin-up | `FUN_801D0370` (+ ring tail `FUN_801D1CFC`) | same, `PARTICLE_TICK_B` | same + `emit_spinup_ring` |
| Tile shatter | `FUN_801D0D24` | `battle_intro_tiles::tick_tile_grid` | `battle_intro::emit_tile` |
| Swirl fan | `FUN_801D1888` / `FUN_801D1A20` | `battle_intro_swirl::tick_swirl` | `battle_intro::emit_swirl_band` |
| Screen-strip curtain | `FUN_801D11D0` | `battle_intro_styles::tick_curtain` | `battle_intro::intro_quad_to_screen` |

The chain: `BattleIntro` holds the style's working set between frames and
synchronises its clock from the live transition entity; the one-shot field
frame capture lands the drawn field in the texture pages each style's packets
name (`Renderer::capture_rgba` → `land_capture_rgba` on the native window,
`gl.readPixels` → `play_intro_land_capture` on the page); and the emitted
`ScreenPrim`s composite over the scene - through
`RenderTarget::SceneWithScreenPrims` natively, through the page's
screen-prim pass in the browser.

### The curtain is a render-to-texture, and only its row pass is on screen

`FUN_801D11D0` draws two passes and it does **not** draw them to the same
place. Between them it links draw-environment packets into the ordering table,
and their OT buckets - a higher index draws first - order them against the
strips:

| OT bucket | packet |
|---|---|
| `0x1F4` | `SetDrawOffset(0, 0)` + `SetDrawArea(320, 0, 320, 240)` |
| `0x1EA` | `FUN_801D1D9C(0x1EA, 2, 0x808080)`, the mid-pass emitter |
| `0x1C2` | the column strips |
| `0x190` | `SetDrawArea(0, y, 320, h)` + `SetDrawOffset(0, y)`, the back buffer |
| `0x12C` | the row strips |

So the column pass runs with the draw area on VRAM `(320, 0)` and its offset at
zero, which makes its primitive coordinates absolute VRAM. `CURTAIN_COL_DRAW_BIAS`
(`0x1E0`) is what makes that fit: a column that passes the visibility test -
which re-centres on `0xA0` - lands at `x` in `320..640`, exactly the installed
area. That area is the rect the row pass' texture pages `0x105` / `0x108`
decode to, so **the row pass samples what the column pass just drew**. The
image is warped horizontally into an intermediate and then sliced vertically
out of it; only the second slice reaches the display.

Two consequences for the port, both now carried. The one-shot field capture
belongs in the *columns* rect only (`capture_rects_for`) - the rows rect is the
intermediate, overwritten every frame - and the column pass has to be
rasterised somewhere, which `engine-render::battle_intro`'s
`compose_curtain_intermediate` does on the CPU because a screen-space quad list
has no render-to-VRAM target. Reading the two rects as "two copies of the same
capture" instead left the curtain stretching in one axis only.

The accumulation the effect rides on is carried, and both of its decays are
pinned from the overlay's own image. Retail never clears. The display side:
`FUN_801D11D0` re-arms the screen wash `FUN_8004695C(0x80808)` unconditionally
at the top of **every** frame (`0x801D1228..0x801D1230`), so a scanline drawn
on one frame decays by 8 per channel behind the ones drawn after it - ~31
frames to black. The intermediate side: the mid-pass emitter `FUN_801D1D9C`
(dumped from the `field_battle_intro` image itself,
`ghidra/scripts/funcs/overlay_field_battle_intro_801d1d9c.txt` - the old
aliased-VA caveat is retired) is `FUN_80024EE4`'s shape pointed one screen
right: a five-word `0x2B` semi-transparent quad over `x 0x140..0x140+W,
y -4..H` (the display halfwords `_DAT_1F80038C` / `_DAT_1F80038E` biased by
`0x140`) behind a `SetDrawMode((abr << 5) | 0xE)` packet at the same layer.
With the curtain's `(0x1EA, 2, 0x808080)` arguments that subtracts `0x80` per
channel from the whole intermediate each frame, between the draw-area install
at `0x1F4` and the column strips at `0x1C2` - a culled column ghosts out over
two frames rather than vanishing.

The port carries both: the intermediate persists across frames and decays by
one mid-pass step instead of being cleared, and a CPU model of the display
buffer - seeded from the same field capture retail's init lands in both
display buffers, decayed one wash step per frame, overdrawn with each frame's
row strips - is uploaded into a spare VRAM rect and drawn as textured backdrop
quads behind the live strips, so the gaps between departing rows show the
fading trail rather than black
(`engine-ui::battle_intro::CURTAIN_TRAIL_RECT` + siblings). Two disclosed
approximations: the wash drain (`FUN_80046978`) scales its constant by the
scratchpad brightness byte, taken at full brightness; and retail's display is
double-buffered, so its per-buffer trail may interleave at half this rate -
settling that needs a retail frame capture of one of the three curtain
formations (hypothesis, graded inference).

### The window has no field in it

Retail's transition owns the whole frame - its init writes game mode `9` and
the field renderer does not run again until the completion arm hands over
(details, incl. the capture chain and the per-style fade blend modes, on
[`cutscene.md`](cutscene.md#the-transition-owns-the-whole-frame)). The port has
no such mode: it composites the transition's primitives *over a live scene*,
because that is the only render target that can put a strip over a field.

`battle_intro::backdrop_prim` is what stands in for the absent mode - an opaque
display-rect quad at the farthest OT bucket, emitted on every frame of the
window as `prims[0]`, including the frames a style draws nothing on. Without
it two things went wrong at once, and only the second was obvious: a patch
still at its rest pose drew additively over an identical live copy of itself
and read at double brightness, and once the last particle expired the emitter
returned an empty list - which put the host back on the non-compositing arm and
presented a clean, still-animating field for the rest of the window.

The dry stretch itself is retail's. `FUN_801D0370` decays a moving particle's
colour by `-0x50505` per frame and the tick's top-byte test masks it for good
once that underflows, so the spin-up field expires around a third of the way in
and the fade ramp does not start until `total - 0x18`. Retail spends the gap on
the CD: phase 5 issues the battle-data read and phases 3 and 6 sit in
`FUN_8003DE7C`'s "READ WAIT" poll, and because the completion arm needs
`clock > total` **and** `ready == 3`, the 132 frames are a floor rather than a
length. The port's loads are instant, so the floor is the whole window and the
gap draws as black - the same thing retail draws, for a reason the port does
not have. `every_transition_frame_covers_the_screen` in
`crates/engine-render/src/tests/battle_intro_emitter.rs` pins the invariant.

The session's `Transition` phase length **is** the intro's own
`DAT_801D2458` - 132 display frames, 252 for the swirl
(`battle_intro_styles::intro_duration_frames`,
[`cutscene.md`](cutscene.md#how-long-a-transition-runs-dat_801d2458)) - because
the entity clock counts up to the same number the session counts down from.
Two things depend on it that are easy to read as style bugs when the window is
short: every fade ramp is a lead before it, and the tile shatter's records hold
at their seeded pose until `delay < elapsed * 0x3C` with `delay = rand() % 5000`,
so the grid needs ~84 frames just to finish starting. Per-style packet detail - what each
emitter builds, the dispatcher flag decode, and the two nuances the port
leaves un-carried - is on
[`cutscene.md`](cutscene.md#per-style-emitters-render-track-gtegpu);
`crates/engine-render/src/tests/battle_intro_emitter.rs` pins per-style packet
counts, geometry and OT linkage, and `crates/engine-vm/tests/battle_intro_chain.rs`
the working-set arithmetic.
