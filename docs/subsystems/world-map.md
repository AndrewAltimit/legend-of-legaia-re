# World Map Subsystem

Covers the overworld traversal mode: normal walk view and the debug top-down view.
Sources: `overlay_world_map.bin` (mc4/mc7) and `overlay_world_map_top.bin` (mc8) captures
from mednafen save states; decompiled at `ghidra/scripts/funcs/overlay_dialog_801e76d4.txt`,
`overlay_dialog_801ead98.txt`, and `801cfc40.txt`.

## Overlay structure

Two world-map overlay variants are paged into `0x801C0000..0x801EFFFF`:

| Variant | First prologue | Triggered by |
|---|---|---|
| Normal walk (`overlay_world_map`) | `0x801CFC40` | Standard world-map mode |
| Top-view debug (`overlay_world_map_top`) | `0x801CE850` | Debug toggle combo (see below) |

Both variants share the core field VM (`FUN_801DE840`), move-VM extension (`FUN_801D362C`),
and all rendering helpers. The top-view variant adds extra rendering code that starts ~0x1400
bytes earlier in the code window.

The view-mode toggle flag lives at `DAT_801F2B94` (outside the 192 KB extraction window
`0x801C0000..0x801EFFFF`; not captured in the binary dump).

## Key functions

### `FUN_801E76D4` — world map controller (9320 bytes)

Entry: `(ctx_ptr)`. Handles:

1. **Top-view debug toggle** — fires when `_DAT_8007B98C != 0` (debug flag) AND
   `_DAT_8007B850 == 0x4A` (pad mask) AND `_DAT_8007B874 == 0x40` (held mask).
   On trigger: `DAT_801F2B94 ^= 1` (flips walk/top-view), captures current actor
   camera position into `_DAT_801F35A8/AA/AC`, clears `ctx[+0x54]` and `ctx[+0x50]`,
   calls `FUN_80035C10`.

2. **Top-view camera controls** (active when `DAT_801F2B94 != 0`):
   - `_DAT_8007B850 & 0x1000` / `0x4000` → `_DAT_80089120 -= 8` / `+= 8` (X scroll)
   - `_DAT_8007B850 & 0x2000` / `0x8000` → `_DAT_80089118 -= 8` / `+= 8` (Z scroll)
   - `_DAT_8007B850 & 0x20` / `0x80` → `_DAT_8007B794 += 0x14` / `-= 0x14` (azimuth)
   - `_DAT_8007B850 & 8` / `2` → `_DAT_8007B6F4 -= 4` / `+= 4` (zoom/height)
   - Bit `DAT_801F2B95 & 1`: enables `FUN_801E75DC` (overlay animation step)
   - Bit `DAT_801F2B95 & 2`: second animation flag

3. **Normal-walk path** (`DAT_801F2B94 == 0`): standard per-frame world-map update
   (field VM tick, actor step, camera follow via motion VM).

### `FUN_801EAD98` — world map debug menu renderer (7280 bytes)

Entry: `(ctx_ptr, x, y, scroll_idx, max_visible)`. Renders a vertically scrolling
menu list for the world map developer menu. String table at `0x801CF344..`:

| Index | Label |
|---|---|
| 0 | `MAP_CHANGE` (or `CLOSED` when `_DAT_8007B868 != 0`) |
| 1 | `CARD_OPTION` (or `CLOSED`) |
| 2 | `PLAYER_STATUS` |
| 3 | `CAMERA` — shows `_DAT_80089120/_DAT_80089118` as `000 000` |
| 4 | `ENCOUNT` — shows encounter rate from `DAT_8007B5F8` |
| 5 | `OTHER_SETTINGS` |
| 6 | `BGM_CALL` — shows `_DAT_801F2E90` as `00` |
| 7 | `DEBUG` |
| … | At least 24 entries total (bounds check `local_40 > 0x17`) |

Called by `FUN_801ECA08` when the debug menu panel is active
(`ctx[+0x54]` mod-6 dispatch resolves to cases 1 or 3).

### `FUN_801CA08` — world map panel sizer / menu caller (256 bytes)

Entry: `(ctx_ptr, row_start, row_end, col_idx, ...)`. Computes panel height
`= (row_end - row_start + 1) * 8`; vertical offset `= 0xD0 - height` (centres
a 208-pixel viewport). Writes height/offset into a panel descriptor at
`0x801F2B98 + col_idx * 28`. Dispatches on `ctx[+0x54]` (6-way JT at
`0x801CF4CC`); cases 1 and 3 call `FUN_801EAD98` to draw the menu list.

### `FUN_801EE90C` — world map text-box dispatcher (128 bytes)

Entry: `(ctx_ptr)`. Dispatches on `ctx[+0x54]` via a 15-entry jump table at
`0x801CF5FC`. When `ctx[+0x54] >= 15` but `< 10`: falls through to
`FUN_80031D00` (text-actor tick — advances the MES bytecode one frame).

### `FUN_801CFC40` — world map sprite batcher (524 bytes, top-view only)

Entry: `(actor_ptr, ?, screen_x, screen_y, ?, ?)`. When `_DAT_8007B6B8 == 0x20`
delegates to `FUN_801CF9F4`; otherwise writes actor screen coordinates into
GPU registers `0x1F800020/22/24` from `actor[+0x14/+0x16/+0x18]`, then
iterates the sprite-descriptor list at `DAT_801C93C8`. Present only in the
`world_map_top` overlay variant.

### `FUN_801DA51C` — world map entity tick (260 bytes)

Entry: `(entity_ptr)`. 5-state dispatcher on `entity[+0x8A]` (jump table at
`0x801CEC28`). When `_DAT_80083808 == 0` and the entity state is 0: calls
`FUN_800243F0` (BGM/asset resolver) to look up the scene associated with the
entity's location. Handles pad-button checks against `_DAT_8007BB38` for
entity interaction. Called once per world-map entity per frame by the entity
pool tick loop.

## Globals used

| Address | Role |
|---|---|
| `DAT_801F2B94` | View-mode flag: `0` = walk, `1` = top-view debug. Outside 192 KB extraction window. |
| `DAT_801F2B95` | Top-view animation bitfield (`& 1` = anim-A enable, `& 2` = anim-B). |
| `_DAT_80089120` | Top-view camera X scroll (adjusted ±8 per D-pad frame). |
| `_DAT_80089118` | Top-view camera Z scroll (adjusted ±8 per D-pad frame). |
| `_DAT_8007B794` | Top-view azimuth (adjusted ±0x14 per frame). |
| `_DAT_8007B6F4` | Top-view zoom/height (adjusted ±4 per frame). |
| `_DAT_8007B868` | Door/portal open flag: `0` = open (MAP_CHANGE/CARD_OPTION visible), `1` = CLOSED. |
| `_DAT_8007B6B8` | Game-mode discriminator (value `0x20` = alternate sprite path). |
| `_DAT_80083808` | World-map entity activation gate. |
