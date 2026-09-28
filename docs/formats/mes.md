# MES dialog format

Container format for Legaia's dialog text. Two on-disc variants share an offset table + bytecode tail. The bytecode encoding is a stream of glyph bytes interleaved with substitution opcodes; the interpreter is statically linked into `SCUS_942.54` (it is not overlay-resident).

## Variants

| Variant | Discriminator | Notes |
|---|---|---|
| Compact | First u16 = `0x0404` | Smaller; embedded inline in `data\battle\efect.dat` and similar. |
| Records | First two bytes = `0x44 0x78` | Used by larger dialog blobs; also the form RAM-extracted from town overlays. |

Both share a header → offset table → bytecode body shape.

### Compact variant - fixed header layout

```
+0x00   u32 LE = 0x00000404       ; magic
+0x04   ...                        ; unused padding (0x24 bytes)
+0x28   u32  back_ptr             ; runtime pointer (patched on load)
+0x2C   u32  forward_ptr          ; runtime pointer
+0x30   u32  expanded_size        ; byte count of the expanded blob
+0x34   u32  count                ; number of messages in the offset table
+0x38   i16[8]                    ; per-line metrics array (16 bytes)
+0x48   ...                        ; additional header fields (16 bytes)
+0x58   u16  ?                    ; pre-table header word
+0x5A   u32  ?                    ; pre-table header dword
+0x5E   u16  ?                    ; pre-table header word
+0x60   u16  ?                    ; pre-table header word
+0x62   u24 LE [N]                ; offset table - 3 bytes per entry, up to 0x56 entries
...
+0xC8   bytecode region starts here
```

`count` gives the number of messages; the offset table spans `0x62..0xC8`
(maximal extent = 0x56 u24 entries = 86 messages at this point in the structure).
Each offset is a byte offset from `0xC8` to the start of that message's bytecode.

### Records variant

The Records format has no fixed header. The parser identifies record boundaries
by scanning for recurring `0x44 0x78` marker pairs (at least 4 hits required).
Each marker starts a variable-stride record; the inter-record contents are
the bytecode and any embedded header fields. Full per-record structure is not
yet reversed - capture a town or field overlay to observe how the runtime
parses this variant.

## Bytecode encoding

Reverse-engineered from the four SCUS interpreter functions ([`FUN_8003CA38`](#fun_8003ca38---glyph-stride-walker), [`FUN_80036044`](#fun_80036044---glyph-count), [`FUN_80036888`](#fun_80036888---text-renderer), [`FUN_80036514`](#fun_80036514---substitution-expander)). The same byte-classification table is used by all four; only the action per byte differs.

| Byte range | Stride | Meaning |
|---|---|---|
| `0x00..0x1E` | 1 | End-of-message / line terminator. The walker stops here. |
| `0x1F..0x5D` | 1 | Single-byte glyph (font tile index). |
| `0x5E XX` | 2 | **Alias** - the substitution expander rewrites this in-place to `0xCE (XX-0x2D)`. |
| `0x5F..0xBF` | 1 | Single-byte glyph. |
| `0xC0 XX` | 2 | 2-byte wide glyph (no substitution). |
| `0xC1 XX` | 2 | Substitute character name. Reads name from save record at `0x80084708 + XX*0x414`; XX = 99 means "current party leader" (`DAT_80084597`). |
| `0xC2 XX` | 2 | Substitute item name from `PTR_DAT_8007436C[XX*3]`. |
| `0xC3 XX` | 2 | Substitute magic name from `PTR_s_Magic_800754D0[XX*3]`. |
| `0xC4 XX` | 2 | Substitute item name (different consumer site than `0xC2`; same `PTR_DAT_8007436C` table). |
| `0xC5 XX` | 2 | Substitute **Tactical Art name** from the [arts-name table](art-data.md#arts-name-table-dat_80075ec4) at `DAT_80075EC4`, keyed by `(character = XX>>6, art index = XX&0x3F)`. (Despite the "spell" naming this is the per-character *art* table, not magic - magic names use `0xC3`.) |
| `0xC6 XX` | 2 | 2-byte wide glyph (no substitution; not in any switch case). |
| `0xC7 XX` | 2 | Substitute terrain / quest name from `DAT_80073F24 + XX*8`. |
| `0xC8..0xCD XX` | 2 | 2-byte wide glyph (stride only). |
| `0xCE XX` | 2 | Spacing op. The width-measure increments the glyph counter without emitting; the renderer uses `XX` as a horizontal offset. |
| `0xCF XX` | 2 | Skip 2 bytes (passthrough - `XX` is rendered alone, not paired with the `0xCF` prefix). |
| `0xD0..0xFE` | 1 | Single-byte glyph. |
| `0xFF` | 1 | **Alias** - the substitution expander rewrites this to `0xCF`. |

The "is this a substitution opcode?" gate in `FUN_80036044` is the integer test `(byte + 0x40) < 8`, which catches `0xC0..0xC7`. Within that range the cases `0xC1..0xC5` and `0xC7` are explicit; `0xC0` and `0xC6` fall through to "no substitution" (still 2-byte stride).

## Interpreter functions

### `FUN_8003CA38` - glyph stride walker

16-instruction primitive that returns the count of bytes (= glyphs) until the next terminator. The classification logic is just:

```c
int FUN_8003CA38(byte *p) {
  int n = 0;
  for (; *p > 0x1E; p++) {
    if ((*p & 0xF0) == 0xC0) { p++; n++; }
    n++;
  }
  return n;
}
```

Used by the dialog window pager to compute line lengths cheaply.

### `FUN_80036044` - glyph count

Walks the bytecode and returns a **glyph count**, not a pixel width: every plain byte and every `0xCE` escape adds one, and it never reads the width table at `0x80073F1C`. Adds the substitution dispatch on top of the stride walker - for each `0xC1..0xC5` or `0xC7` byte, it follows the substitution pointer into the corresponding name table and counts that string's glyphs too (a nested `0xC1` inside it is expanded once more). The count paces the dialog typewriter reveal; the pixel measurer is `FUN_80035F04`, and nothing wraps text at run time - see [`dialog-font.md`](dialog-font.md#line-width-and-wrapping). Evidence: the `move v0,a3` return at `0x80036504` over a counter only ever advanced by `addiu ...,0x1` / an expanded-substitution count.

Two properties of the loop are easy to miss. It is sized by a pre-walk that returns the string's **byte** length to its `NUL`, and it decrements that size once per iteration however many bytes the iteration consumed - so each two-byte unit (`0xCE`, `0xCF`, a substitution) runs the loop one byte past the `NUL`, counting whatever follows. And `0xC0` / `0xC6` count nothing: `0xC0` fails the jump-table bound and `0xC6`'s table word is the no-string exit. The field pager calls it at `0x801D8A6C` on the row it is typing, lead byte included, to decide when the row is finished. Port: `legaia_font::typewriter_glyph_count`.

### `FUN_80036888` - text renderer

The actual draw loop. Same byte classification, but emits glyphs into the text-actor buffer and forwards spacing ops to the cursor advancer. Calls [`FUN_80036514`](#fun_80036514---substitution-expander) at the start to expand substitutions into a working buffer.

### `FUN_80036514` - substitution expander

Reads source bytecode from `param_2` and writes expanded bytecode to `param_1`. Two input-time aliases are normalised:

| Source byte | Rewritten as |
|---|---|
| `0x5E XX` | `0xCE (XX - 0x2D)` |
| `0xFF` | `0xCF` |

Then it walks the input and inlines `0xC1..0xC5` / `0xC7` substitutions: each substitution opcode is replaced by the bytes of the substituted name, copied character-by-character.

## Dialog window pager - `FUN_801D84D0`

Lives in the dialog overlay. Distinct from the byte-level interpreter - this is the per-frame state machine that pages text on input. 26 outer states (`_DAT_801F2734`, range `0..0x19`) covering load / scroll / drain / wait-for-input / done. Stores per-line bytecode pointers in `_DAT_801F3540[line]` (16-line buffer at `0x801F3580`). Test `(byte & 0x7F) < 0x20` is used to detect line terminators (catches both `0x00..0x1F` and `0x80..0x9F`).

The engine port lives in `engine-core`: `dialog_window` (the row window, the scroll and the confirm arms), `dialog_pacing` (the typing row's reveal counter and hold) and `dialog::OwnedDialogPanel`, which drives both over the page's decoded rows on the path both play hosts share. `engine-vm` only cites the pager. An earlier sentence here placed the port in `engine-vm`; no pager code was ever there.

### Box geometry

Max lines per box is stored at `_DAT_801F2740`. **Three** init arms pin it to 3, not two: states `0` (`0x801D90BC`), `6` (`0x801D9174`) and `9` (`0x801D920C`) each carry their own `li v0,0x3; sw v0,0x2740(v1)`. State `3` (`0x801D9154`) is not one of them - it runs a four-store prologue and jumps away at `0x801D916C` before reaching the tail. That is the height of a scrolling **row window**, not a page length - see [Row window and scrolling](#row-window-and-scrolling). Other consumers (status / quantity panels) reach the pager with different values written in by their own setup.

The three arms are near-copies but **not** byte-identical: over their 0x98-byte extent they differ in exactly one word, the successor state each writes to `_DAT_801F2734` - state `0` hands off to `1`, state `6` to `7`, state `9` to `0xA`. That one word is the whole behavioural difference between the box-open control bytes, so read it before treating any two of these arms as interchangeable.

Where those successors go is what separates teardown from a fresh box:

- States `1`, `4` and `7` share handler `0x801D8708`. It zeroes the scroll `_DAT_801F2738`, the pending text pointer `_DAT_801F3538` and the typewriter words, then tests `_DAT_801F2734 == 4` and returns when true - so state `4` (reached from `0x24`) **preserves** the row array, which is what makes "next line, same box" work. States `1` and `7` fall through and **clear** the 16-entry row buffer at `_DAT_801F3540` (the init arms `0` / `6` / `9` have already cleared it once). All three are idle: the caller stores the next text pointer in `_DAT_801F3538` and steps the state to `2` / `5` / `8`, whose shared handler `0x801D876C` loads it - and steps back to the idle state while the pointer is still zero.
- State `0xA` is a different handler, `0x801D92A4`: a ramp on `_DAT_801F274C` of `dt << 9` a call toward `0x1000`, which on completion sets `_DAT_801F273C = 0x18` and drops into state `1`.

  The draw reads the ramp as a **collapse**, not an open: the box goes out at `y + h*a/0x2000` with height `h*(0x1000 - a)/0x1000` (`0x801D9970..0x801D99A0`), so it shrinks to its centre line. `_DAT_801F273C` then makes each call return before the draw (`0x801D8630..0x801D864C`) until `dt` steps have counted it down - the box is gone for that long before the fresh one opens. The `town01` trace below spends five pager calls in state `0xA` at `dt = 2`, the four `0x400` steps plus the call that sees `0x1000`.

The `0x27` / `0x28` / `0x29` picker arms write their box rect literally: `x = 0x26`, `y = 0x94 + ((4-N)*0xF)/2`, `w = 0xF4`, `h = 0x38 - (4-N)*0xF` - a 244-wide box whose height shrinks 15 px per absent option, recentred on the 4-option anchor `y = 0x94`. The `0x2A` arm writes a different, fixed rect: `(0xD8, 0x4A, 0x58, 0x1A)`, a small box at the top right. Fields `+0x3C/+0x3E` are the slide's start position for both - see [The picker slide](#the-picker-slide-states-0x11--0x13--0x15--0x17).

### Box render

The pager draws the window each frame through the shared SCUS box emitter: `FUN_80034B6C(skin)` stages the window-skin index (standard reading box = skin `0x61`; a box whose `ctx+0x10` class byte is `2` resets to skin `0`), then `FUN_8002C69C(x, y, w, h)` emits the box. For the main reading box the call is `FUN_8002C69C(ctx+0x12, ctx+0x14 + d, 0xF4, h - 8)` with `h = lines*0xF + 5`; `d` and `h` move only with the `0x48` collapse (state `0xA`, above) - the frame never scrolls, the rows do. An earlier reading here named `d` the scroll. The picker box passes its own rect.

Draw order inside the frame is text first, box last: every packet goes on the same ordering-table entry (`[0x1F8003F4] + 4`) through `FUN_8003D2C4`, which links at the head.
So the GPU meets the last-added packet first and the box renders behind its glyphs.

The rows are clipped. Two draw-area packets bracket them on that entry (`0x801D95A8..0x801D964C` before the rows, `0x801D9860..0x801D9934` after): the one added after, which the GPU meets first, narrows the drawing area to `y = box_y - 1 ..= box_y + lines*0xF - 1`; the one added before restores the full screen, so the frame is not clipped. A row scrolling out of the top loses its top edge, and the row scrolling in below the third slot stays hidden until it rises into the band.

`FUN_8002C69C` composes two layers (see `ghidra/scripts/funcs/8002c69c.txt`):

- **Frame**: 4 corner + 4 tiled edge sprites from the skin's records in the corner/edge table `DAT_80073A00` (32-byte stride, indexed via the class table `DAT_800732A4`, 12-byte stride) - the gold 9-slice family of the system-UI sheet, shared with every menu window.
- **Interior fill** (hardcoded in the function body, not skin data): **two identical semi-transparent gouraud `POLY_G4` quads** (prim code `0x3B`, blend mode 0 = `B/2 + F/2`) spanning the box rect, top vertices RGB `(0x18,0x18,0x28)`, bottom vertices RGB `(0x40,0x40,0xA0)`. Applying mode-0 twice composes to `0.25*back + 0.75*gradient` - the translucent deep-blue panel. The engine mirror bakes the gradient at alpha `191/255` (`engine-core::save_menu_atlas::ATLAS_RECT_DIALOG_FILL`) and draws it as one source-over sprite (`engine-render::dialog_window_chrome_draws_for`).

Hand sprites come from the cursor family `FUN_8002B994`: the **page-advance hand** (kind 1) draws at `(0x10A, box_y + lines*0xF - 0x13)` while state `0x19` waits for confirm; the **option-picker hand** (kind 0) draws at `(box_x - 6, box_y + cursor*0xF)` on the selected row. Option labels render CLUT-7 white (`_DAT_8007B454 = 7`) at `box_x + 0x10`, 15-px row pitch.

### Multi-segment box packing

A field NPC's interaction text is a flat pool of `0x1F`-lead lines, each `0x1F <glyphs> 0x00`. The pager types **consecutive** lines as one **page**: the byte after a line's `0x00` terminator being another line (`(b & 0x7F) < 0x20`, `0x801D8AB4`) means "same page, next row". A page ends only at the post-page control byte the pager reads in state `0x19` (the table below), however many lines precede it - a fourth line scrolls the three-row window rather than waiting ([Row window and scrolling](#row-window-and-scrolling)). So a three-line speech box is three back-to-back `0x1F` lines followed by a single `0x24` (next page); multi-page speech is several such pages chained by `0x24`.

The earlier statement that a box ends after at most three rows was a packing convention read as pager behaviour: nothing in the pager stops at three.

The advance loop in `FUN_80039B7C` (state `0x2`, the `for (; 0x1e < *pbVar4; ...)` walk that skips a line the SM has shown) masks `(*pbVar4 & 0xF0) == 0xC0` and consumes the following data byte as part of the same token. So a `0xC0..=0xCF` escape whose argument byte falls in the `0x00..=0x1E` range - e.g. a `0xC1 0x00` character-name substitution - does **not** terminate the line early; the line ends only at a terminator that is not a `0xC?` escape argument. Every `0xC0..=0xCF` byte is a 2-byte token (see the token table above), so the standard interpreter strides past them correctly.

The rule matters most at a line's **head**: a line can open on an escape, and 113 of the 1586 talkable partition-1 records open their first line that way (`1F C1 00 ...`, the lead character's name, is the common case - `town01` `P1[16]` at record `+0x4C`). A scanner that ends a line at its first `0x00` reads such a line as one byte long, rejects it as a stray marker and resumes inside its text, so every consumer starts one line late.

The engine's segment finder (`man_field_scripts::first_inline_dialog_offset`) walks lines with `legaia_mes::dialog_box::line_end` for that reason; the same walk also stops at the other terminators (`0x01..=0x1E`), which retires the stray `0x1F` bytes in opcode operands that a scan-to-`0x00` used to accept by swallowing the real line after them. Disc-gated pin: `engine-core/tests/dialog_escape_led_line_disc.rs`.

Decoded by `legaia_mes::dialog_box`:

- `pack_page` packs one pager page from a `0x1F` lead: every consecutive line up to the control byte, reporting the terminating `Dispatch`. This is what the engine types.
- `pack_box` packs window-sized chunks, capped at `LINES_PER_BOX = 3`; a cut with another line after it reports `Dispatch::ImplicitNextPage`, which is a packing cut, not a pause.
- `pack_boxes` chains pages while the dispatch continues, stopping at `End` / `Terminate` / a `Picker` / field-VM bytecode.

Pinned on real disc bytes by `field_dialog_boxpack_disc`: the Rim Elm sparring partner's (Tetsu) opening narration packs into three full 3-row pages chained by `0x24`, then a 2-row box that opens the 4-option "do you want something today?" topic menu - and that narration's `Mist appeared, .., but` line keeps its tail past a `0xC1 0x00` escape. Note the pool also holds the NPC's *other* story-branch lines; the contiguous box run stops where the pager hands control back to the field VM (a non-pager control byte), which `Dispatch::Unknown` marks.

### Row window and scrolling

The row table `_DAT_801F3540[]` is a scrolling window over the conversation, not a page buffer, and the pager draws every non-null entry of it. Read off the state handlers (jump table `0x801CEBC0`):

| State | Handler | What it does |
|---|---|---|
| `0x0B` | `0x801D89B8` | Types the row at the table's last index at the [typewriter pace](dialog-font.md#typewriter-pacing); rows above it draw whole. A finished row followed by another line takes the next slot, or - with all three slots full - hands over to `0x0C`. Any other byte ends the page: `_DAT_801F3534 = rows_on_page - 1 + (3 - last_index)`, then `0x19`, or `0x0F` while that is below three. |
| `0x0C` | `0x801D8B5C` | Scrolls: `_DAT_801F2738 -= speed * dt` (`speed` = `_DAT_801F2750`, `0x24`). Past `-0xEF` the table shifts up a slot, the next line enters the last slot and the scroll resets; back to `0x0B`, or to `0x0D` if a press raised the speed. No button is involved. |
| `0x0D` | `0x801D8C64` | Completes the page: one more line into the table a call, the typing row shown whole; with a fourth row in the table, `0x0E`. |
| `0x0E` | `0x801D8D28` | Scrolls the completed page's overflow through at `0x25`, a line entering per row, then `0x0F` or `0x19` as in `0x0B`. |
| `0x0F` | `0x801D8E3C` | Scrolls away the rows the previous page left above this one, at `speed - speed/4` a call, until `_DAT_801F3534` reaches three; then `0x19`. |
| `0x19` | `0x801D8F4C` | Waits for a press with the page-advance hand up, then dispatches on the control byte (below). |

The draw adds `scroll >> 4` to every row's `y` (`0x801D9790`), so a row is `0xF0` scroll units - 15 px - tall; the typing state draws without it. A page turn on `0x24` keeps the table: state `5` puts the next page's first line in the slot below the last row (`0x801D88C8..0x801D89B4`), or goes to `0x0C` first when the table is full, so the previous page stays on screen above the new one until `0x0F` scrolls it away at the new page's end. At `0x19` the window shows exactly that page's rows.

**Confirm while a page types.** Every arm that moves reads the new-press word masked by the confirm buttons (`_DAT_800846D0 | _DAT_800846D4`). In `0x0B` a press stores `speed = 0x25` and jumps to `0x0D` (`0x801D89B8..0x801D8A04`); during a short-row hold (dispatch case `0x10`, `0x801D86BC`) it does the same when `speed` is still `0x24`, and clears the hold; in `0x0C` and `0x0F` it only raises the speed, and `0x0C` then hands over to `0x0D`. State `5` puts the speed back to `0x24` for the next page.

A PCSX-Redux trace of `town01` placement `P1[16]` (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`, the scroll word and the row table as record offsets per vsync) shows every arm at `dt = 2`: a two-row first page; a second page whose first row types beneath the two carried rows and whose next line scrolls in on its own (four calls at `72`), then one carried row scrolled away (`54` a call) before the wait; and, with confirm taps injected, the latch going to `0x25`, state `0x0D` completing the page in two calls, and the scrolls running at `74` and `56`. The engine (`engine-core::dialog_window`) reproduces both traces vsync for vsync - `crates/engine-core/tests/dialog_window_disc.rs`.

### Post-page dispatch (state `0x19`)

When the page is full and the user presses confirm (`_DAT_800846D0` / `_DAT_800846D4`), state `0x19` reads the **next control byte** past the box (`*pbVar14 & 0x7F`) and selects the follow-on state directly:

| Control byte | Next pager state | Effect |
|---|---|---|
| `0x25` | `0` -> `1` | box reset, then row buffer cleared (teardown) |
| `0x24` | `3` -> `4` | continue text on the next line, same box (rows preserved) |
| `0x48` | `9` -> `0xA` | box reset, then the open animation - a fresh box |
| `0x4C` followed by `0xFF` | `6` -> `7` | box reset, then row buffer cleared (teardown) |
| `0x2A` | `0x11` -> `0x12` (slide in from the right -> 2-option picker) | 2-option menu in a small top-right box |
| `0x27` | `0x13` -> `0x14` (slide up from the bottom -> 2-option picker) | 2-option `Yes`/`No`-style menu |
| `0x28` | `0x15` -> `0x16` (slide up from the bottom -> 3-option picker) | 3-option menu |
| `0x29` | `0x17` -> `0x18` (slide up from the bottom -> 4-option picker) | 4-option menu |

**What the pager does and does not decide.** The state numbers above are read straight off the dispatch chain at `0x801D8FDC` and the jump table at `0x801CEBC0`, and the teardown-vs-fresh-box split is settled by the successor handlers. What is *not* in these instructions is the end of the **conversation**: `0x25` and `0x4C 0xFF` clear the row buffer and stop there - the pager neither returns a status nor signals its caller. Whether the dialogue session ends is decided caller-side, in the actor dialog SM `FUN_80039B7C` and the field VM. Treat "end conversation" / "close the dialog" as a reading of the box teardown, not as a property the pager byte carries; the session-level semantics are open.

The picker states are reached only through this press: a prompt that ends on a picker open byte waits in `0x19` with the advance hand like any other page, and the menu opens on the confirm. The stores of `0x11` / `0x13` / `0x15` / `0x17` at `0x801D9058..0x801D909C` are the only writers of those four states in the pager, and they sit behind `0x19`'s press test (`0x801D8FCC..0x801D8FD0`). The engine does the same: `OwnedDialogPanel` leaves a picker prompt waiting, `advance_page` (the press) opens the menu and the next confirm commits the choice. The `--simple-dialogue` fallback panel keeps an immediate open (`opening_menu_at_wait`), because its host has no page-turn press.

**The automatic press.** State `0x19` also counts `_DAT_80073F00` down, before its press test: while the word is positive, each call subtracts the frame step, and the call that takes it to zero or below clears it and replaces the pad word with the confirm binding `0x800846D0` (`0x801D8F4C..0x801D8F88`) - the page turns, or the picker opens, with no button down. Field-VM op `4C 89` is its only writer ([script-vm-menuctrl.md](../subsystems/script-vm-menuctrl.md)); 37 sites in 12 scenes issue it (disc-wide census, the cutscene-heavy `izumi`, `koin1`, `uru`, `dream` among them).

A retail capture that pokes `21` at a page end on `retock_innkeeper_talk_open` (frame step 2, `autorun_dialog_picker_open.lua` with `LEGAIA_PICKER_AUTO=21`) reads `19, 17, .. 1` on the next eleven calls and leaves `0x19` on the twelfth. The engine runs it as `OwnedDialogPanel::tick_at_auto` over `World::dialog.auto_press`, which the op writes; every World dialog path treats the fired press as the player's confirm.

So the picker controls are MES `0x27` / `0x28` / `0x29` **and `0x2A`**:

- The open byte is matched as `byte & 0x7F`, so both the bare `0x27..0x2A` form and the high-bit `0xA7..0xAA` form are accepted; the field corpus stores the bare form.
- Each picker arm computes the box rect from N with immediates (above; there is no per-N table) and clamps the choice cursor at `*(DAT_801c6ea4 + 0xc)`.
- On confirm in the picker (`case 0x12` / `0x14` / `0x16` / `0x18`), the pager reads the **continuation byte at `pbVar14[N*2 + 1]`** (past the N-option jump table) - same `0x24` / `0x48` / `0x4C 0xFF` jump table as the post-page dispatch - and advances. The chosen index lives in `*(DAT_801c6ea4 + 0xc)`.

**`0x2A` is a fourth open byte, not a bare resize.** Its arity is not in the dispatch chain above, which only names the entry state; it is in the shared picker-cursor handler at `0x801D941C`, which reads the option count off the *active* state - `li t1,0x2` as the fall-through, `0x16` -> 3, `0x18` -> 4. State `0x12` therefore takes the 2 arm, the same count `0x27`'s `0x14` takes.

Two further instructions settle that `0x12` is a live picker state and not a resize that falls through to something else. The same handler's `bne v1,0x12` carve-outs at `0x801D9474` and `0x801D94D0` make its cursor *clamp* at both ends where every other picker wraps. And the inline-script control handler `FUN_80038050` lists `case 0x2a` alongside `0x27`/`0x28`/`0x29` in the arm that applies the chosen option's relative jump - so a `0x2A` region is read as a jump table by the code that branches on it.

The corpus agrees structurally: retail `0x2A` sites carry exactly two jump entries, a valid continuation byte at `O+5`, and two label segments after it. Every inn's `Yes`/`No` offer is one of these (see [`subsystems/inn.md`](../subsystems/inn.md#the-trigger-the-pickers-own-jump-table)), which is why a decoder that stops at `0x29` finds no inn menu at all.

### The picker slide (states `0x11` / `0x13` / `0x15` / `0x17`)

No picker is usable on the press that opens it. The odd picker states are a **slide**: the box moves from off screen to its rect over a fixed span, and only the even state after it takes input. Nothing resizes - width and height are constant from the first drawn frame. The jump table at `0x801CEBC0` sends `0x11` to `0x801D92F4`, and `0x13` / `0x15` / `0x17` to one shared handler at `0x801D9350`; the input states `0x12` / `0x14` / `0x16` / `0x18` all go to `0x801D941C`.

- **The press.** Every confirm in `0x19` stores the sentinel `+0x54 = 0x309` and zeroes `+0x14` / `+0x16` / `+0x18` on the pager actor (`0x801D90A0..0x801D90B8`), whatever state it picked - the stores follow the dispatch, so a page turn leaves the sentinel behind as well.
- **The first call** in the odd state sees `+0x54 == 0x309` and initialises the slide: span `+0x50 = 0x18`, count `+0x54 = 0x18`, start `(+0x3C, +0x3E)`, target `(+0x14, +0x16)`, size `(+0x24, +0x26)`. For `0x2A` that is start `(0x150, 0x4A)`, target `(0xD8, 0x4A)`, size `(0x58, 0x1A)` (`0x801D9314..0x801D934C`): the box enters from the right edge. For `0x27` / `0x28` / `0x29` it is start `(0x26, 0xF0)` and the N-option rect above (`0x801D9398..0x801D93F0`): the box rises from the bottom edge.
- **Every later call** subtracts the frame step from the count; the call that takes it to zero or below stores 0 and steps to the even state (`0x801D93F4..0x801D9418`).
- **The draw** (`0x801D9A08..0x801D9ADC`) skips the picker box while the count is the sentinel, and otherwise places it at `target + (start - target) * count / span` per axis (signed division, truncating), with the labels at `x + 0x10` and a 15-px pitch - the labels travel with the box. The option hand (`FUN_8002B994` kind 0) is drawn only once the count is 0 (`0x801D9BB4..0x801D9BE4`).

A PCSX-Redux capture from `retock_inn_stay_prompt` - the retock innkeeper's stay offer, parked on the `0x19` entry of the page that ends on `0x2A` - runs it at frame step 2 (`autorun_dialog_picker_open.lua`, which logs the pager actor's slide fields and the box origin the draw formula gives them):

| Pager call | State | Count | Box origin |
|---|---|---|---|
| the press | `0x19` -> `0x11` | `0x309` | not drawn |
| 1 | `0x11` | 24 | `(336, 74)` - past the right edge |
| 2 .. 12 | `0x11` | 22, 20, .. 2 | `(326, 74)`, `(316, 74)`, .. `(226, 74)` - 10 px a call |
| 13 | `0x12` | 0 | `(216, 74)`, hand drawn; input read from call 14 |

So the menu takes input thirteen pager calls - 26 vsyncs - after the press, and screenshots taken alongside the rows show the box entering from the right. The same capture from `retock_innkeeper_talk_open` (a re-talk after the stay) repeats the sequence call for call. The `0x27` / `0x28` / `0x29` slide has the same count sequence, and a capture of the 4-option case confirms the rise. `town01_tetsu_topic_prompt` parks on the page wait before Tetsu's `0x29` topic list; one press gives:

| Pager call | State | Count | Box origin |
|---|---|---|---|
| the press | `0x19` -> `0x17` | `0x309` | not drawn |
| 1 | `0x17` | 24 | `(38, 240)` - below the bottom edge |
| 2 .. 12 | `0x17` | 22, 20, .. 2 | y `232, 224, 217, 209, 201, 194, 186, 178, 171, 163, 155` at x 38 |
| 13 | `0x18` | 0 | `(38, 148)`, hand drawn; input read from call 14 |

The box is the full 244 x 56 four-row rect from the first drawn call, with its labels moving inside it; y falls by 92 x 2/24 a call, truncated, so the steps run 8, 8, 7 px and repeat. The state changes one vsync after the press, the slide's first call follows on the next pager call, and input opens 26 vsyncs after the state change - the same timing as the `0x2A` slide. The 2- and 3-option lists share the start `(0x26, 0xF0)` and the count sequence; only the target rect differs (the formula above), so each rises `0xF0 - y_target` pixels over the same twelve calls. The same run from the re-loaded state repeats the sequence call for call.

The call that takes the count to zero steps to the even state and branches straight to the draw (`j 0x801D95AC` at `0x801D9414`), past the cursor handler, so the first call that can read Up / Down or confirm is the one after it. The option hand is already up on the zeroing call.

The engine runs the slide once, in `engine-core::dialog_picker_slide`: the panel's press stores the sentinel, each pager call it runs while the menu is open advances the count at the frame step, and `OwnedDialogPanel::picker_rect` / `picker_hand_drawn` / `picker_takes_input` expose the box, the hand and the input gate. Both hosts (the native window's `dialog_stage_layout` and the play page's `play_dialog`) draw the box and its labels at that rect and the hand only at rest; the panel ignores Up / Down and confirm until the slide rests, and a `0x2A` cursor clamps. The resting rects come from `legaia_mes::picker_box_rect` and the starts from `picker_slide_start`. The simplified `--simple-dialogue` panel, which opens its menu without a press, opens it at rest.

### Picker control-region layout

Relative to the open byte at index `O`, a picker occupies:

```text
[ .. 0x1F prompt segment .. 0x00 ]   the box text shown above the menu
O                                     open byte (0x27 / 0x28 / 0x29)
O+1 .. O+N*2                          N option entries, 2 bytes each (i16 LE)
O+N*2+1                               continuation byte (0x24/0x25/0x48/0x4C 0xFF)
                                      OR the first label's 0x1F (immediate-labels)
[ optional 0x4C 0xFF terminate ]
N * [ 0x1F label segment 0x00 ]       the on-screen option labels
```

The on-screen **option labels are standard `0x1F`-lead glyph segments located after the continuation byte** (the pager render loop at `FUN_801D84D0` ~lines 2166-2185 measures each with `FUN_8003CA38` and draws it with `FUN_80036888`). An earlier note that "the option labels are the 2-byte entries between the open byte and the continuation" is **falsified** - those 2-byte entries are the per-option **jump table**, not the labels.

**Two continuation forms.** The byte at `O+N*2+1` is either a post-page dispatch byte (`0x24`/`0x25`/`0x48`/`0x4C`) before the labels, **or** the first label's `0x1F` lead directly - an **immediate-labels** menu with no post-page continuation. The `izumi` book menu uses the dispatch-byte form; **Rim Elm's Tetsu spar menu** (and town01's other pickers) use the immediate-labels form (open `0x29`, 4 jump entries, then the labels - option 2 "I want to practice with you." is the training fight). `parse_picker_at` accepts both (an earlier version required the dispatch byte and so found zero pickers in town01); pinned live by `scripts/pcsx-redux/autorun_tetsu_picker_data.lua` (the spar dialogue buffer) + disc-gated `tetsu_spar_picker_disc`.

Each 2-byte entry is a **signed 16-bit little-endian relative jump**. The inline-script control handler `FUN_80038050` (the per-actor `actor[+0x90]` script stepper, distinct from the pager) applies it on confirm: it reads the cursor at `DAT_801C6EA4+0xC` and sets the script PC `actor[+0x9E]` to

```text
new_pc = (O + 1 + index*2) + i16_LE(entry[index])
```

i.e. the displacement is relative to the **start of that option's own 2-byte entry**. Pinned empirically: across the four story-branch re-emissions of the `izumi` book menu, all four option entries shift by an identical per-emission delta (-518 / -564 / -549) - the signature of relative addressing to a moving site - and every decoded option across the field corpus jumps to a byte inside its own script. Parser `legaia_mes::picker` (`scan_pickers` / `parse_picker_at` + `Picker::jump_target`); disc-gated regression `field_dialog_pickers_disc`.

The engine consumes this directly: `engine_core::dialog::OwnedDialogPanel::from_inline_dialog` attaches the picker when it immediately follows the box's prompt segment; `legaia-engine play-window` draws the option labels under the prompt with an Up/Down cursor, and a confirm press runs `OwnedDialogPanel::confirm_menu` - the engine port of `FUN_80038050`: it applies the chosen option's relative jump, resumes typing at that branch's reply segment, and re-attaches a nested menu if one follows.

For the **faithful** path, `engine_core::inline_dialogue` (`World::step_inline_dialogue`, the port of the dialog SM `FUN_80039B7C`) drives the whole inline interaction script through the real field VM: it executes the control bytecode between text segments (story-flag tests, `SET`/`CLEAR` flag ops, scene changes) and only pauses at each `0x1F` segment to show a box, so a chosen option's branch handler runs its side effects before the reply. It is gated by `World::toggles.use_vm_dialogue` (default `false` at the engine-core level; `legaia-engine play-window` enables it **by default**, with `--simple-dialogue` opting back into the simplified typewriter).

## Live blob example

A town-overlay save state captured a live MES blob in RAM at `0x80109270` (3893 bytes). The header + bytecode structure matches both Compact and Records expectations after small variant-specific tweaks. The blob is used to validate the Rust parser end-to-end.

## CLI

```
mes info       <PATH>             # detect variant + report header
mes disasm     <PATH>             # walk the bytecode, print decoded ops
mes json       <PATH>             # emit machine-readable JSON
mes events     <PATH> [--index N] # walk the interpreter for one message
mes stats-all  <PATH>             # event-type histogram across every message
```

## Related

- [`dialog-font.md`](dialog-font.md) - proportional dialog font in VRAM.
- [`reference/functions.md`](../reference/functions.md) - the four MES interpreter functions. (`FUN_8001FD44` is **not** one of them - it is the scene-change packet. Field dialogue has no dedicated opcode: it is the actor's inline interaction-script MES text, shown by the actor-dialog SM `FUN_80039b7c` + pager `FUN_801D84D0`, triggered by the touch / button-press interaction, not by an opcode (op `0x3E` with `op0 < 100` is the scripted-battle install) - see [`subsystems/script-vm.md` § Field dialogue](../subsystems/script-vm.md#field-dialogue-has-no-opcode).)
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) - field-VM opcode reference. Note `0x3F` is the named scene-change, not a dialog op.
