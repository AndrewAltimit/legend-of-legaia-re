# Global hue-ramp palette at VRAM row 479

The retail Legaia engine maintains a 15-slot CLUT "rainbow ramp" at
VRAM `(fb_x=0..240, fb_y=479)`. The ramp is materialised in main RAM
at `0x800F19xx` during early engine init, DMA'd into VRAM, and then
relied on as **persistent state** by every non-battle scene. Field
and town NPC TMDs sample the lower half of the ramp via CBA cells
`0x77C8..0x77CF` (row 479 slots 8..15); sprite and effect prims
sample the upper half.

Implementation: [`crates/asset/src/npc_palette.rs`](../../crates/asset/src/npc_palette.rs).

## Layout

```
fb_x   slot   peak  shape
   0     0      20   hue wheel at 5-bit peak intensity = 20 (very bright)
  16     1      19
  32     2      17
  48     3      16
  64     4      15
  80     5      13
  96     6      12
 112     7      11
 128     8       9   <-- town/field NPC anchor: CBA 0x77C8
 144     9       8       CBA 0x77C9
 160    10       6       CBA 0x77CA
 176    11       5       CBA 0x77CB
 192    12       4       CBA 0x77CC
 208    13       2       CBA 0x77CD
 224    14       1       CBA 0x77CE
 240    --       0   intentionally left empty (CBA 0x77CF)
```

Each 16-pixel slot holds a 16-entry CLUT in BGR555:

- Entry 0 is the canonical `0x0000` (transparent black).
- Entries 1..15 traverse a HSV-style hue cycle (B → C → G → Y → R → M → B)
  in 5-bit color space, at a fixed peak value per slot.
- Peak values across the 15 active slots form a smooth descending
  ramp `[20, 19, 17, 16, 15, 13, 12, 11, 9, 8, 6, 5, 4, 2, 1]`.

The exact runtime generator (the MIPS function that writes the bytes
into `0x800F19xx`) has not been pinned - see *Open questions* below.

## Why this matters

Town01's NPC TMDs reference CBA cells that fall on row 479 slots
8..14. The 32-byte CLUT payloads at those VRAM positions are
**absent from disc**: a brute-force scan of every PROT entry and
`SCUS_942.54` for the canonical slot-8 byte sequence returns zero
hits, and even an 8-byte prefix isn't found anywhere on the disc.
Without those CLUT rows the engine's targeted-upload pre-pass drops
every textured prim that samples them as `MissingClut`, costing
roughly 21 percentage points of the town01 prim-keep ratio (78.6%
without the ramp vs. 99.3% with it).

Because the bytes are stable across every retail save state we have
access to (the only saves where row 479 differs are battle scenes,
which overwrite the row with battle-overlay content), the engine's
clean-room scene-build pre-pass can paint the ramp in verbatim from
a corpus-confirmed capture. The Rust API is:

```rust
use legaia_asset::npc_palette;
npc_palette::apply_global_hue_ramp(&mut vram);
```

`SceneResources::build` and `SceneResources::build_targeted` both
call this at the end of every scene-build pre-pass, so consumers
get the ramp for free.

## Cross-save corroboration

The hue ramp is verifiable from any pair of retail save states the
mednafen toolkit can read. Across the bundled corpus, row 479
fb_x=0..240 hashes identically for every non-battle save and differs
only in battle scenes (where the row is repurposed by the battle
overlay). Slots 17..19, 52..53, and 56..63 of the same row stay
zero across all saves; slots 20..51 are per-scene palettes that fall
outside this module's scope.

The CLI for reproducing the corroboration is in
[`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md) -
extract VRAM with `mednafen-state vram-dump --out-bin`, slice row 479
at byte offset `0xEFE00`, and hash 32-byte windows at 16-pixel stride.

## Open questions

- **Writer location**: static analysis sweeps every imported program
  (`SCUS_942.54` plus the captured overlays) for any LUI+ADDIU /
  load / store pair whose effective address lands in
  `0x800F1800..0x800F1B00` and returns zero hits. That rules out
  direct absolute addressing; the writer accesses the staging buffer
  through an indirect base pointer (e.g. via `$gp`, via a struct
  field, or via a pointer stored in another global). Pinning the
  function requires either a watchpoint-style RAM diff across a
  very-early-boot save state pair (currently unavailable) or a
  decompilation pass over the init code that runs before the title
  screen.
- **Generator algorithm**: the 15-entry hue sequence is suspiciously
  non-uniform - per-segment sample counts are `2, 3, 2, 3, 3, 2` and
  positions don't fit any of the obvious closed-form distributions
  (`(i*N)//15`, midpoint, rounded). The slots are likely **not**
  scaled copies of one base table either; the peak progression
  `(9 → 8 → 6 → 5 → 4 → 2 → 1)` skips intermediate values (7 and 3)
  in a way that breaks `slot[N] = slot[0] * peak[N] / peak[0]`. The
  algorithm probably operates segment-by-segment in 5-bit space with
  integer rounding rules we haven't reconstructed yet.
- **Slot 15**: row 479 fb_x=240 stays zero in every retail save. A
  4bpp prim that addresses CBA `0x77CF` therefore samples a
  transparent palette in retail too - either those prims are culled
  by a different path in the renderer (e.g. early-out on all-zero
  CLUT) or they're tolerated as rendering errors. The engine's
  targeted-upload filter currently keeps them; they may still
  render as black quads.
- **Upper slots in non-battle scenes**: slots 16, 54, 55 also hold
  globally-stable content. These are outside the active "ramp" but
  appear to be installed by the same init code. Folding them in is
  a follow-up.
