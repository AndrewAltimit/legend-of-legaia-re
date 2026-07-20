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

Reverse-engineered from the four SCUS interpreter functions ([`FUN_8003CA38`](#fun_8003ca38---glyph-stride-walker), [`FUN_80036044`](#fun_80036044---text-width-measurement), [`FUN_80036888`](#fun_80036888---text-renderer), [`FUN_80036514`](#fun_80036514---substitution-expander)). The same byte-classification table is used by all four; only the action per byte differs.

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

### `FUN_80036044` - text width measurement

Walks the bytecode and returns total width. Adds the substitution dispatch on top of the stride walker - for each `0xC1..0xC5` or `0xC7` byte, it follows the substitution pointer into the corresponding name table and recursively walks that string's width too. Calls itself implicitly by re-running the same `(byte > 0x1F)` loop on the substituted string.

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

The crate-level Rust port of this pager lives in [`crates/engine-vm`](../../crates/engine-vm/README.md) as the dialog-window state machine.

### Box geometry

Max lines per box is stored at `_DAT_801F2740`. **Three** init arms pin it to 3, not two: states `0` (`0x801D90BC`), `6` (`0x801D9174`) and `9` (`0x801D920C`) each carry their own `li v0,0x3; sw v0,0x2740(v1)`. State `3` (`0x801D9154`) is not one of them - it runs a four-store prologue and jumps away at `0x801D916C` before reaching the tail. The standard dialog box scrolls in up to three lines, then state `0xD` advances to the "page full, wait for input" state `0xE`. Other consumers (status / quantity panels) reach the pager with different values written in by their own setup.

The three arms are near-copies but **not** byte-identical: over their 0x98-byte extent they differ in exactly one word, the successor state each writes to `_DAT_801F2734` - state `0` hands off to `1`, state `6` to `7`, state `9` to `0xA`. That one word is the whole behavioural difference between the box-open control bytes, so read it before treating any two of these arms as interchangeable.

Where those successors go is what separates teardown from a fresh box:

- States `1`, `4` and `7` share handler `0x801D8708`. It tests `_DAT_801F2734 == 4` and returns immediately when true - so state `4` (reached from `0x24`) **preserves** the row array, which is what makes "next line, same box" work. States `1` and `7` fall through and **clear** the 16-entry row buffer at `_DAT_801F3540`, tearing the box down. States `1` and `7` are otherwise indistinguishable.
- State `0xA` is a different handler, `0x801D92A4`: a per-frame ramp on `_DAT_801F274C` toward `0x1000` (the box-open animation), which on completion sets `_DAT_801F273C = 0x18` and drops into state `1`.

The picker arms write their box rect literally: `x = 0x26`, `y = 0x94 + ((4-N)*0xF)/2`, `w = 0xF4`, `h = 0x38 - (4-N)*0xF` - a 244-wide box whose height shrinks 15 px per absent option, recentred on the 4-option anchor `y = 0x94`. Fields `+0x3C/+0x3E` are the slide-animation start position (the resize state `0x11` uses the same pair).

### Box render

The pager draws the window each frame through the shared SCUS box emitter: `FUN_80034B6C(skin)` stages the window-skin index (standard reading box = skin `0x61`; a box whose `ctx+0x10` class byte is `2` resets to skin `0`), then `FUN_8002C69C(x, y, w, h)` emits the box. For the main reading box the call is `FUN_8002C69C(ctx+0x12, ctx+0x14 + scroll, 0xF4, lines*0xF + 5 - 8)`; the picker box passes its own rect. Draw order inside the frame is text first, box last (a later-submitted prim lands in a deeper OT slot, so the box renders behind its glyphs).

`FUN_8002C69C` composes two layers (see `ghidra/scripts/funcs/8002c69c.txt`):

- **Frame**: 4 corner + 4 tiled edge sprites from the skin's records in the corner/edge table `DAT_80073A00` (32-byte stride, indexed via the class table `DAT_800732A4`, 12-byte stride) - the gold 9-slice family of the system-UI sheet, shared with every menu window.
- **Interior fill** (hardcoded in the function body, not skin data): **two identical semi-transparent gouraud `POLY_G4` quads** (prim code `0x3B`, blend mode 0 = `B/2 + F/2`) spanning the box rect, top vertices RGB `(0x18,0x18,0x28)`, bottom vertices RGB `(0x40,0x40,0xA0)`. Applying mode-0 twice composes to `0.25*back + 0.75*gradient` - the translucent deep-blue panel. The engine mirror bakes the gradient at alpha `191/255` (`engine-core::save_menu_atlas::ATLAS_RECT_DIALOG_FILL`) and draws it as one source-over sprite (`engine-render::dialog_window_chrome_draws_for`).

Hand sprites come from the cursor family `FUN_8002B994`: the **page-advance hand** (kind 1) draws at `(0x10A, box_y + lines*0xF - 0x13)` while state `0x19` waits for confirm; the **option-picker hand** (kind 0) draws at `(box_x - 6, box_y + cursor*0xF)` on the selected row. Option labels render CLUT-7 white (`_DAT_8007B454 = 7`) at `box_x + 0x10`, 15-px row pitch.

### Multi-segment box packing

A field NPC's interaction text is a flat pool of `0x1F`-lead lines, each `0x1F <glyphs> 0x00`. The SM packs **consecutive** lines into one window of `_DAT_801F2740 = 3` rows: the byte after a line's `0x00` terminator being another `0x1F` means "same box, next row". A box ends after at most three rows, at the post-page control byte the pager reads in state `0x19` (the table below). So a three-line speech box is three back-to-back `0x1F` lines followed by a single `0x24` (next page); multi-page speech is several such boxes chained by `0x24`.

The advance loop in `FUN_80039B7C` (state `0x2`, the `for (; 0x1e < *pbVar4; ...)` walk that skips a line the SM has shown) masks `(*pbVar4 & 0xF0) == 0xC0` and consumes the following data byte as part of the same token. So a `0xC0..=0xCF` escape whose argument byte falls in the `0x00..=0x1E` range - e.g. a `0xC1 0x00` character-name substitution - does **not** terminate the line early; the line ends only at a terminator that is not a `0xC?` escape argument. Every `0xC0..=0xCF` byte is a 2-byte token (see the token table above), so the standard interpreter strides past them correctly.

Decoded by `legaia_mes::dialog_box`:

- `pack_box` packs one box from a `0x1F` lead, capped at `LINES_PER_BOX = 3`, reporting the terminating `Dispatch`.
- `pack_boxes` chains pages while the dispatch continues, stopping at `End` / `Terminate` / a `Picker` / field-VM bytecode.

Pinned on real disc bytes by `field_dialog_boxpack_disc`: the Rim Elm sparring partner's (Tetsu) opening narration packs into three full 3-row pages chained by `0x24`, then a 2-row box that opens the 4-option "do you want something today?" topic menu - and that narration's `Mist appeared, .., but` line keeps its tail past a `0xC1 0x00` escape. Note the pool also holds the NPC's *other* story-branch lines; the contiguous box run stops where the pager hands control back to the field VM (a non-pager control byte), which `Dispatch::Unknown` marks.

### Post-page dispatch (state `0x19`)

When the page is full and the user presses confirm (`_DAT_800846D0` / `_DAT_800846D4`), state `0x19` reads the **next control byte** past the box (`*pbVar14 & 0x7F`) and selects the follow-on state directly:

| Control byte | Next pager state | Effect |
|---|---|---|
| `0x25` | `0` -> `1` | box reset, then row buffer cleared (teardown) |
| `0x24` | `3` -> `4` | continue text on the next line, same box (rows preserved) |
| `0x48` | `9` -> `0xA` | box reset, then the open animation - a fresh box |
| `0x4C` followed by `0xFF` | `6` -> `7` | box reset, then row buffer cleared (teardown) |
| `0x2A` | `0x11` (box resize) | start the box geometry animation |
| `0x27` | `0x13` -> `0x12` (init -> 2-option picker) | 2-option `Yes`/`No`-style menu |
| `0x28` | `0x15` -> `0x14` (init -> 3-option picker) | 3-option menu |
| `0x29` | `0x17` -> `0x16` (init -> 4-option picker) | 4-option menu |

**What the pager does and does not decide.** The state numbers above are read straight off the dispatch chain at `0x801D8FDC` and the jump table at `0x801CEBC0`, and the teardown-vs-fresh-box split is settled by the successor handlers. What is *not* in these instructions is the end of the **conversation**: `0x25` and `0x4C 0xFF` clear the row buffer and stop there - the pager neither returns a status nor signals its caller. Whether the dialogue session ends is decided caller-side, in the actor dialog SM `FUN_80039B7C` and the field VM. Treat "end conversation" / "close the dialog" as a reading of the box teardown, not as a property the pager byte carries; the session-level semantics are open.

So the picker controls are MES `0x27` / `0x28` / `0x29`:

- The open byte is matched as `byte & 0x7F`, so both the bare `0x27..0x29` form and the high-bit `0xA7..0xA9` form are accepted; the field corpus stores the bare form.
- Each picker arm sets the box dimensions from a per-N table (lines 1995-2003 of the dump) and clamps the choice cursor at `*(DAT_801c6ea4 + 0xc)`.
- On confirm in the picker (`case 0x12` / `0x14` / `0x16` / `0x18`), the pager reads the **continuation byte at `pbVar14[N*2 + 1]`** (past the N-option jump table) - same `0x24` / `0x48` / `0x4C 0xFF` jump table as the post-page dispatch - and advances. The chosen index lives in `*(DAT_801c6ea4 + 0xc)`.

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

For the **faithful** path, `engine_core::inline_dialogue` (`World::step_inline_dialogue`, the port of the dialog SM `FUN_80039B7C`) drives the whole inline interaction script through the real field VM: it executes the control bytecode between text segments (story-flag tests, `SET`/`CLEAR` flag ops, scene changes) and only pauses at each `0x1F` segment to show a box, so a chosen option's branch handler runs its side effects before the reply. It is gated by `World::use_vm_dialogue` (default `false` at the engine-core level; `legaia-engine play-window` enables it **by default**, with `--simple-dialogue` opting back into the simplified typewriter).

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
- [`reference/functions.md`](../reference/functions.md) - the four MES interpreter functions. (`FUN_8001FD44` is **not** one of them - it is the scene-change packet. Field dialogue has no dedicated opcode: it is the actor's inline interaction-script MES text, shown by the actor-dialog SM `FUN_80039b7c` + pager `FUN_801D84D0`, triggered by the field-interact op `0x3E` `op0 < 100` - see [`subsystems/script-vm.md` § Field dialogue](../subsystems/script-vm.md#field-dialogue-has-no-opcode).)
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) - field-VM opcode reference. Note `0x3F` is the named scene-change, not a dialog op.
