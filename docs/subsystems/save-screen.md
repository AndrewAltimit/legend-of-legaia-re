# Save Screen Subsystem

Covers the save-slot selection and write flow used whenever the game writes
progress to the PSX memory card. The save UI lives inside the **menu overlay**
(same 129-function binary as shop, inn, and status screens - not a separate
overlay). Sources: `overlay_save_ui_select.bin` and `overlay_save_ui_saving.bin`
mednafen captures (taken at the slot-select and writing-in-progress states),
both confirmed as the menu overlay by function-address identity; decompiled functions at
`ghidra/scripts/funcs/overlay_menu_801dc6b4.txt`,
`overlay_menu_801daef4.txt`, `overlay_menu_801dafd4.txt`.

## Overlay structure

The save UI is hosted by the menu overlay paged into `0x801C0000..0x801EFFFF`.
No dedicated save-screen overlay exists. All three capture points (shop, save
slot select, saving in progress) produced identical function address sets with
only call-frequency differences in the inventory CSV - confirming a single
shared overlay.

## Key functions

### `FUN_801DC6B4` - save-screen outer dispatcher (856 bytes)

Entry: `()`. Returns `true` when the save flow has terminated (outer state
`> 5`). Drives a 9-case state machine on `_DAT_8007B43C`:

| State | Behaviour |
|---|---|
| 0 | Init: copies party pointers `_DAT_800846D0/D4` → `DAT_801EF0F0/F4`; decodes `_DAT_8007B450` (entry-context pointer) into `DAT_801E46A4` (sub-screen selector, see below); sets `_DAT_8007B440 = 0xF2` (full fade); advances to state 1. |
| 1 | Fade-in wait: advances to state 2 once `_DAT_8007B440 < 0x79`. |
| 2 | Sub-screen dispatch: calls `(*(DAT_801E46A4 * 4 + 0x801E4F40))(_DAT_8007B874)` - indirect function pointer table; pad input masked by `_DAT_8007B874`. |
| 3/4/5 | Fade-out (`_DAT_8007B9D8 = 2`); gated on `_DAT_8007B460 == 0` before advancing. |
| ≥ 6 | Terminal - returns `true`. |

The **entry-context pointer** `_DAT_8007B450` determines which sub-screen opens:

| `_DAT_8007B450` | Sub-screen ID (`DAT_801E46A4`) | Meaning |
|---|---|---|
| `(char*)1` sentinel | `0x2` | Save (from menu entry) |
| `*ptr == '\x01'` | `0x19` | Load from slot |
| `*ptr == '\x07'` | `0x20` | Auto-save path |
| `*ptr == '\r'` | `0x4` | Post-save return |
| `*ptr == '\x00'` | `0x1a` | Cancel / back |

Input is suppressed while `_DAT_8007B440 > 0x79` (mid-fade). After state 2
completes, the fade-out advances states 3 → 4 → 5. The four save-coordinate
words `DAT_801E46BC/C0/C4/C8` are zeroed on init and maintained across the
sub-screen lifetime.

### `FUN_801DAEF4` - save-slot selector (224 bytes, sub-screen 0x2 / 0x1)

Internal step counter in `DAT_801E46AC`:

| Step | Action |
|---|---|
| 0 | Set `_DAT_8007B44C = DAT_801C6EA0` (memory-card handle from overlay init); run actor VM with `&DAT_801E4E30` (slot-select menu bytecode). |
| 1 | Wait on `_DAT_8007BB80 != 0` (menu-active flag); advance to step 2. |
| 2 | Call `FUN_801DD35C(1, 1)` (confirm selection); advance to step 3 on success. |
| 3 | Clear `DAT_801E46A4 = 0` when `_DAT_8007B450 != 0` (return to previous screen). |

Each step calls `func_0x80031D00()` (text-actor tick / MES advance) before
returning.

### `FUN_801DAFD4` - save-slot confirm / saving-in-progress (584 bytes)

Internal step counter in `DAT_801E46AC`:

| Step | Action |
|---|---|
| 0 | Clear `_DAT_8007BB98/90/88`; set `_DAT_8007BB94 = 4` (3-slot scrolling list param); run actor VM with `&DAT_801E4E38`; mask `DAT_801E46BC &= 0xFFF`. |
| 1 | Call `FUN_801D688C(&DAT_801E46BC, 3, 1)` (3-item slot list + confirm). Button result: slot 0 → sub-screen 0x1B (card-full/error); slot 1 → validate then run actor VM `&DAT_801E4E54` (advance to step 2); slot 2 → cancel SFX; return 2 → close. |
| 2 | Clear state vars; set `DAT_801E46A4 = 0x1E` (advance to write sub-screen). |

**Save slot validation** (step 1, slot 1 path): scans the save-block existence
table at `&DAT_80084140 + slot * 2 + 0x1818` (byte 0 = slot present,
byte 1 = slot valid) over the range `_DAT_8007B5EA.._DAT_8007B5EC`. A fully
absent table yields error SFX (`func_0x80035bd0(0x23)`).

## Globals used

| Address | Role |
|---|---|
| `_DAT_8007B43C` | Outer state machine discriminant (0..≥6). |
| `_DAT_8007B440` | Screen fade level: `0xF2` = full opaque; `0` = transparent. |
| `_DAT_8007B450` | Entry-context pointer; value determines sub-screen ID. |
| `_DAT_8007B9D8` | Mode discriminant: `1` = save-menu active, `2` = fade-out. |
| `_DAT_8007B44C` | Memory-card handle set to `DAT_801C6EA0` at slot-select init. |
| `_DAT_8007BB80` | Menu-active flag; step 1 waits while zero. |
| `_DAT_8007B5EA` | Save-slot scan start index. |
| `_DAT_8007B5EC` | Save-slot scan end index. |
| `DAT_80084140` | Save-block existence table; stride 2 bytes per slot. Bytes `+0x1818/+0x1819` = present/valid flags. |
| `DAT_801E46A4` | Sub-screen function index (into pointer table at `0x801E4F40`). |
| `DAT_801E46AC` | Sub-screen internal step counter. |
| `DAT_801E46BC/B0/B4` | Per-column save-slot state / pad-input buffer. |

## Sub-screen function pointer table

`FUN_801DC6B4` case 2 dispatches via `0x801E4F40[DAT_801E46A4]`. Full table
read from `overlay_menu.bin` offset `0x24F40` (table base `0x801C0000`):

| ID | Function | Role |
|---|---|---|
| `0x00` | `FUN_801DD12C` | 2-state final-exit screen: state 0 invokes actor `&DAT_801E4A78` (terminal display); state 1 waits `_DAT_8007BB80 == 0`, then sets `DAT_801E46A0 = 0xF2` and exit code `_DAT_8007B43C = 3` |
| `0x01` | `FUN_801D6B20` | `FUN_801DAEF4` slot selector path |
| `0x02` | `FUN_801D6E18` | save entry (from menu entry-context `(char*)1`) |
| `0x03` | `FUN_801D6D38` | 2-state Yes/No confirm with default cursor `1`: actor `&DAT_801E4BD4`, picker `FUN_801D688C(&DAT_801E46D0, 2, 1)`; cursor `1` returns to current sub-screen (`0x01`), cursor `0` advances to `0x00` (exit), cancel returns to `0x01` |
| `0x04` | `FUN_801DD1B8` | post-save return path |
| `0x05` | `FUN_801D7C00` | (unknown) |
| `0x06` | `FUN_801D7E50` | (unknown) |
| `0x07` | `FUN_801D8734` | (unknown) |
| `0x08` | `FUN_801DD26C` | 2-state actor + pad-release-wait: state 0 invokes actor `&DAT_801E4CA4`; state 1 waits `_DAT_8007BB80 == 0` AND no button held (`_DAT_8007B874 & (_DAT_800846D4 \| _DAT_800846D0) == 0`), advances to `0x05` |
| `0x09` | `FUN_801D7FF8` | (unknown) |
| `0x0A` | `FUN_801D8308` | (unknown) |
| `0x0B` | `FUN_801D8A58` | 3-state Yes/No confirm with exit branch: state 0 invokes actor `&DAT_801E4CBC`; state 1 picker on cursor `0` invokes second actor `&DAT_801E4A78` + sfx via `func_0x80042310(0x88, 1)` and advances to state 2, otherwise goes to `0x06`; state 2 waits `_DAT_8007BB80 == 0`, then sets `DAT_801E46A0 = 0xF2`, exit code `_DAT_8007B43C = 4` |
| `0x0C` | `FUN_801D8B90` | (unknown) |
| `0x0D` | `FUN_801D8D94` | (unknown) |
| `0x0E` | `FUN_801D8F10` | (unknown) |
| `0x0F` | `FUN_801D9110` | (unknown) |
| `0x10` | `FUN_801D9280` | (unknown) |
| `0x11` | `FUN_801D9594` | (unknown) |
| `0x12` | `FUN_801D98F0` | 2-state scrollable picker: state 0 sets `_DAT_8007BB94 = 4`, actor `&DAT_801E4D88`; state 1 picker `FUN_801D688C(&DAT_801E46C4, DAT_80084594, 1)` (count from save-block existence table). Confirm → `0x13`, cancel → `0x01` |
| `0x13` | `FUN_801D99F0` | (unknown) |
| `0x14` | `FUN_801D9C14` | per-character record serialisation (0x414 bytes, `char_id` stride) |
| `0x15` | `FUN_801DA2A0` | (unknown) |
| `0x16` | `FUN_801DD310` | no-op tick: tail-calls `func_0x80031D00` (frame-end / actor-tick flush) with no other work |
| `0x17` | `FUN_801DD330` | thin wrapper invoking the generic picker `FUN_801DA9F8(start=0, end=9, init=0x30, return_subscreen=1)` |
| `0x18` | `FUN_801DAE24` | save-card driver entry. State 0 installs the card handle (`_DAT_8007B44C = DAT_801C6EA0`) and invokes actor `&DAT_801E4E28`; state 1 waits `_DAT_8007BB80 == 0`; state 2 calls `FUN_801DD35C(1, 2)` (saving-overlay main; drives `FUN_801E3294` libcd state machine via the per-frame ticker `FUN_801E1114`); state 3 returns to sub-screen `0x01` |
| `0x19` | `FUN_801DAEF4` | load-from-slot path (entry-context `*ptr == '\x01'`) |
| `0x1A` | `FUN_801DAFD4` | save-slot confirm / saving-in-progress - advances to `0x1E` on confirm |
| `0x1B` | `FUN_801DB21C` | card-full / error screen |
| `0x1C` | `FUN_801DB380` | (unknown) |
| `0x1D` | `FUN_801DB7F4` | (unknown) |
| `0x1E` | `FUN_801DBC5C` | 4-state spinner: state 0 inits + calls `FUN_801D6628(&DAT_801E4EE4)`; state 1 waits for `_DAT_8007BB80 == 0`; state 2 reads two inventory bytes at `0x80084140 + 0x1818 + _DAT_8007BB88*2` and advances to `0x1F` on user-confirm (`_DAT_8007BB94 == 2`) or back to `0x1A` on cancel; state 3 returns to `0x1A` |
| `0x1F` | `FUN_801DBD94` | D-pad quantity-input screen (state 0 init + actor invoke; state 1 ±1/±10 on the dpad clamped to `[1, DAT_801E46B8]`, on confirm applies money delta `_DAT_8008459C += (price * qty) >> 1` and walks live inventory at `0x80084140 + 0x1818` for a non-empty slot; state 2 returns to `0x1A` after a brief delay). NOT the save-card writer - actual libcd I/O lives in `FUN_801E3294` (see "Libcd I/O state machine" section below); `FUN_8001A8B0(SC_base=0x80084140, staging=0x801E5120, 0x1A18)` is plain memcpy used in both directions (post-read or pre-write staging copy) |
| `0x20` | `FUN_801DC1CC` | auto-save path (entry-context `*ptr == '\x07'`) |

The table ends at `0x1F`; entries past `0x20` are the start of the MES bytecode
section (`0x85826B82` etc.) and are not function pointers.

### Load/save dispatch (`FUN_801DD35C`)

The saving-overlay's main routine is shared between the load and save paths.
Sub-screens `0x18` (save) and `0x19` (load) are structurally identical
3-state drivers - they install the card handle, invoke a direction-specific
display actor, then call `FUN_801DD35C(1, op)` repeatedly until it returns
non-zero. The op selector distinguishes direction:

| Sub-screen | Driver | Display actor | Call | Direction |
|---|---|---|---|---|
| `0x18` | `FUN_801DAE24` | `&DAT_801E4E28` | `FUN_801DD35C(1, 2)` | save (RAM → card) |
| `0x19` | `FUN_801DAEF4` | `&DAT_801E4E30` | `FUN_801DD35C(1, 1)` | load (card → RAM) |

Both install `_DAT_8007B44C = DAT_801C6EA0` (PSX libC card handle) on state 0,
so the same global handle is used in both directions. On success both return
to sub-screen `0x01` (the slot picker). Both directions share the same
saving-overlay state machine; the load branch's bulk memcpy
`FUN_8001A8B0(SC_base=0x80084140, staging=0x801E5120, 0x1A18)` is the
post-libcd-read copy (staging buffer → SC RAM).

### Libcd I/O state machine (`FUN_801E3294`)

The actual PSX memory-card calls live in `FUN_801E3294` (in the menu
overlay, also captured in the saving overlay), a 5-state libcd
state-machine driver:

| State (`DAT_801EF188`) | Action |
|---|---|
| `0` | Init: call BIOS-A thunk `FUN_8006EE14(chan)`, advance to `1`. |
| `1` | Poll `FUN_801E3900()`; on result `4` finalise with `FUN_8006EE34` (calls BIOS-B `_card_write` thunk pair); on `1` advance to `2`. |
| `2` | Step: call `FUN_801E39A8` + BIOS-A thunk `FUN_8006EE24(chan)`, advance to `3`. |
| `3` | Wait; same dispatch shape as state 1. |
| `4` | Cleanup: stash result in `DAT_801EF184/180`, reset to `0`. |

The channel argument is `chan = port * 16 + sub_op`. Status strings
printed during the loop (`"NOT_CARD"`, `"card_sts:%d old:%d"`,
`"not card count:%d"`) confirm this drives the libcd lifecycle.
`FUN_8006EE34` is the actual write helper: it calls BIOS-B(0x50) via
`FUN_8006EE7C`, then BIOS-B(0x4E) via `FUN_8006EE6C` with `(chan, 0x3F, 0)`.

### Save-block directory enumeration (`FUN_801E1208`)

After `FUN_801E3294` finishes a directory scan, `FUN_801E1208` walks the
15-entry libcd directory table at `0x801F32A8` (entry stride `0x28`),
matching each filename against the region-specific Legend of Legaia
prefix using BIOS-A(0x18) `strncmp` (`FUN_80056748`):

| Prefix string | Region |
|---|---|
| `BASCUS-94254PRO_` | USA (Legend of Legaia, SCUS-94254) |
| `BISCPS-10059PRO_` | JP (Legend of Legaia, SCPS-10059) |

The 2-digit slot number is parsed from positions `[10..11]` of the
matched entry and used to write a per-slot record at
`slot_idx * 0x40 + 0x801F2A88` plus a present-marker at
`0x801F2A48 + slot_idx`. `_DAT_801F01F0` carries the available block
count from the prior `FUN_801E3BA0` call.

The per-frame ticker `FUN_801E1114` is the single static caller wiring
the trio together: it calls `FUN_801E3294(DAT_801EF18C, 0)` every frame
to advance the libcd state machine, and when `_DAT_801F021C == 3` (save
commit) it sequences `FUN_801E3AF0` (open `"bu%d_%d"` channel) →
`FUN_801E3BA0` (block-count query) → `FUN_801E1208` (directory walk).

### Per-character status preview (`FUN_801D9C14`, sub-screen `0x14`)

Per-character menu preview function. Reads from the character record at
`char_id * 0x414 + 0x80084A9E` and uses `DAT_801EF0C8` as a staging
buffer for the displayed stat read-back. State 0 calls `FUN_801CF650`,
which is the **equipment-effect stat aggregator** for the selected
character: it walks the 5 equipment slots in the character record, looks
each equipment ID up in the 12-byte-stride table at `0x80074368`, and
when `prop[id*12].byte_0 == 1` reads a 5-byte stat-bonus block from
`0x80074F68 + prop[id*12].byte_1 * 8`, summing the bonuses into
`DAT_801EF09C..DAT_801EF098` (5 stat totals - HP/MP/Atk/Def/Spd or
similar). This is **not** a memory-card write primitive.

## Relationship to `legaia_save`

The memory-card write calls through `_DAT_8007B44C` (PSX LibC card handle set
from `DAT_801C6EA0`). The in-engine LGSF format (`legaia_save::SaveFile` with
`SaveExt`) is the clean-room counterpart. The `crates/save` constants
`RETAIL_STORY_FLAGS_OFFSET`, `RETAIL_INVENTORY_OFFSET`, and `SAVE_GAME_DATA_RAM_BASE`
expose all confirmed offsets; use `read_retail_story_flags` / `read_retail_inventory`
to slice them from a raw SC block.

## Story-flag persistence vs. scratchpad word

Two distinct global-state stores share the *name* "story flags" but live in
unrelated regions, and **the SC save/load path does not sync between them**:

| Store | Address | Size | Persists in SC? | Touched by save/load |
|---|---|---|---|---|
| Wide bitmap | RAM `0x80085600..0x80085800` | 512 B (4096 bits) | Yes — at SC offset `0x14C0` | Yes, via the bulk RAM→card transfer at `FUN_8001A8B0(0x80084340, card, ...)` (live RAM region containing the bitmap is part of the linear SC body) |
| Scratchpad word | RAM `0x1F800394` | 4 B (32 bits) | No | No |

The scratchpad word `_DAT_1F800394` is the field-VM transient that opcodes
`0x2E` (set bit), `0x2F` (clear bit), and `0x30` (test bit) operate on.
Static-reader sweep across `ghidra/scripts/funcs/*.txt` (`python3
scripts/scan_funcs_for_addr_range.py --lo 0x1F800394 --hi 0x1F800398`)
finds **one** non-RMW writer: `FUN_8001DCF8` at PC `0x8001E17C`, which
seeds it from the game-mode descriptor table:

```c
_DAT_1f800394 = (uint)*(ushort *)(&DAT_800707a0 + _DAT_8007b83c * 0x18);
```

`DAT_800707A0` is `mode_table[0].param` (the mode table at `0x8007078C` has
24-byte stride; the `param` field sits at offset `+0x14`). So the scratchpad
word's lower 16 bits are re-initialised on every mode switch from the
mode's `param` constant; the upper 16 bits start zeroed and are only ever
written by the script-VM bit ops. No retail code path copies between
`0x80085600..0x80085800` and `0x1F800394` in either direction.

In `legaia_save::SaveExt`, `story_flag_bits` mirrors the wide bitmap and
round-trips through the LGSF v3 extension block; `story_flags` mirrors the
scratchpad word and round-trips through the LGSF v1 prelude. The two fields
are independently populated — that matches retail.

## Retail SC block layout

Verified by cross-referencing mednafen save-state RAM dumps against real MCR saves.
The game data region (`block+0x200` onward) is a contiguous linear copy of live RAM
starting at `0x80084340` (`SAVE_GAME_DATA_RAM_BASE`). Any live-RAM field can be
located via `block_offset = 0x200 + (ram_addr - 0x80084340)`.

| Offset in SC block | Size | Field |
|---|---|---|
| `0x0000` | 2 | `SC` magic |
| `0x0002` | 1 | icon flags (`0x11` = 1 frame, 16-color) |
| `0x0004` | 92 | save title (Shift-JIS, null-padded) |
| `0x0060` | 32 | 16-color icon palette (16 × u16 LE BGR5) |
| `0x0080` | 128 | icon pixels (16×16 @ 4bpp) |
| `0x0100` | 256 | (duplicate icon frame or padding) |
| `0x0200` | 0x66F | display/global header (see below) |
| `0x086F` | 0x414 × N | character records (Vahn, Noa, Gala, Terra…) |
| `0x14C0` | 0x200 | story-flag bitmap (mirrors RAM `0x80085600..0x80085800`) |
| `0x1818` | 0x90 | inventory array — 72 × `(item_id: u8, count: u8)` (mirrors RAM `0x80085958..0x800859E8`) |

**Display header** (`0x0200..0x086E`):

| Offset | Size | Field |
|---|---|---|
| `+0x000` | 8 | Current location name (ASCII, null-padded), e.g. `Rim Elm` |
| `+0x054` | 12 | Primary character display name (for save-select screen) |
| `+0x208` | 8 | CDNAME label of most-recently-visited scene (e.g. `town0b`) |
| `+0x218` | 8 | CDNAME label of previous scene (e.g. `town01`) |

**Character records**: `CHARACTER_RECORD_SIZE` (0x414) bytes each, name at record+0x000.
Minimum 4 records observed: Vahn, Noa, Gala, Terra. Empty slots have all-zero bytes;
`read_retail_char_records` stops at first all-zero record.

`legaia_save::card::read_retail_char_records(sc_block, max_records)` implements extraction.
Constants: `RETAIL_GAME_DATA_OFFSET` (0x200), `RETAIL_CHAR_RECORD_HEADER_SIZE` (0x66F),
`RETAIL_CHAR_RECORD_STRIDE` (0x414). All re-exported from the `legaia_save` crate root.

## Sprite asset sources (Continue → Load screen)

The retail Continue → Load screen overlays a "Load" header panel and
N blue SLOT pills on top of the dimmed title art. Asset sources:

| Visible element | Confirmed source | Notes |
|---|---|---|
| Title art behind (wordmark, NEW GAME / CONTINUE, copyright) | `PROT 0888` title TIM | Same atlas the title menu samples; rendered dimmed during SaveSelect. |
| **`Load` panel TIM + CLUT** | **`PROT.DAT[0x018E0]` system-UI sprite sheet, CLUT row 2** | 4bpp 256x192 TIM in the unindexed pre-`init_data` PROT.DAT gap. CLUT block uploads to VRAM `(fb_x=0, fb_y=511)`; the panel-specific row (row 2 of the 16x16 CLUT block) uploads to VRAM `(32, 511)`. Byte-confirmed: the 32-byte CLUT signature appears at exactly one place in the disc corpus (PROT.DAT offset 0x1934). Constants exported by `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_*`. |
| `Load` panel **9-slice tile geometry** | **PINNED — engine renders byte-perfect** | Retail composes the 81x29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. Per-tile rects below; all exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and rendered by `legaia_engine_render::save_select_chrome_draws_for`. No interior fill sprite is drawn — the "marbled blue" look is the dimmed title art bleeding through the empty middle of the 9-slice frame. |
| **"Load" text glyphs** | **PINNED at `PROT.DAT[0x11218]` menu-glyph atlas CLUT row 13** | 4 glyphs at source `(192,32)`, `(240,64)`, `(16,64)`, `(64,64)` — each 14x15. CLUT row 13 signature byte-equal at `PROT.DAT[0x113CC]`. Engine port currently renders this text via the engine font; wiring the menu-glyph atlas as the title-text source is a follow-up. |
| `SLOT 1` pill | `PROT 0899 + 0x16908 (33, 97, 45, 15)` decoded with CLUT 7 | Saturated blue baked label; byte-equal to retail. |
| `SLOT 2` pill | `PROT 0899 + 0x16908 (33, 113, 45, 15)` decoded with CLUT 7 | Stacked directly below the SLOT 1 pill in the source atlas. |
| Hand cursor | **OPEN** | Neither the save-menu TIM nor the menu-glyph atlas carries it. Likely lives in another pre-`init_data` gap TIM. |

### Pinned 9-slice tile rects (system-UI TIM CLUT row 2)

All rects are `(u, v, w, h)` in 256x192 source-page-pixel coords;
all exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*`.

| Tile | dst (fb_x, fb_y) | src (u, v, w, h) |
|---|---|---|
| Top-left corner | (6, 4) | (160, 0, 4, 4) |
| Top-right corner | (83, 4) | (188, 0, 4, 4) |
| Bottom-left corner | (6, 29) | (160, 28, 4, 4) |
| Bottom-right corner | (83, 29) | (188, 28, 4, 4) |
| Top edge ×3 | (10, 4) / (34, 4) / (58, 4) | (164, 0, 24, 4) |
| Top edge remainder | (82, 4) | (164, 0, 1, 4) |
| Bottom edge ×3 | (10, 29) / (34, 29) / (58, 29) | (164, 28, 24, 4) |
| Bottom edge remainder | (82, 29) | (164, 28, 1, 4) |
| Left edge | (6, 8) | (160, 4, 4, 21) |
| Right edge | (83, 8) | (188, 4, 4, 21) |

### How the panel TIM was pinned

A capture+decode pipeline against PCSX-Redux save state slot 9 (parked
on the load screen):

1. `bash scripts/pcsx-redux/run_probe.sh --lua scripts/pcsx-redux/autorun_load_screen_dump.lua --sstate ~/Tools/pcsx-redux/SCUS94254.sstate9 --frames 180`
   writes `load_screen_fb.{raw,meta}` (the rendered 320×228 framebuffer)
   and `load_screen_ram.bin` (full 2 MiB main RAM).
2. `python3 scripts/pcsx-redux/extract_vram_from_sstate.py ~/Tools/pcsx-redux/SCUS94254.sstate9 captures/load_screen_dump/<iso>/`
   gunzips the save state, finds the `GPU.vram` protobuf field (tag
   `0x1A 0x80 0x80 0x40`), and writes the 1 MiB raw BGR555 VRAM blob.
3. `python3 scripts/pcsx-redux/decode_vram.py vram.bin vram.png`
   renders the 1024×512 VRAM as a PNG so texture pages and CLUT rows
   are visible.
4. Cross-reference the panel-CLUT bytes at VRAM (32, 511) against
   `extracted/PROT.DAT` byte-by-byte: the 32-byte signature matches
   exactly one location (offset 0x1934 = CLUT row 2 of the TIM at
   0x018E0). That TIM's pixel block decoded with CLUT row 2 contains
   the full in-game menu UI atlas (HP/MP panels, money displays,
   battle chrome, equipment frames, and the load-screen panel
   chrome).

### Current engine port status

The engine port (`legaia_engine_core::save_menu_atlas` +
`legaia_engine_render::save_select_chrome_draws_for`) ships the SLOT
pills byte-equal to retail and uses a **speculative** PROT 0899
sub-rect for the panel pending the 9-slice tile-geometry pin. The
byte-confirmed system-UI TIM is declared in
`legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET` /
`OVERLAY_SYSTEM_UI_PANEL_CLUT_ROW`; switching the atlas builder over
to it is gated on the GPULog probe.
