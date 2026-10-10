# Settled threads: Text / fonts / dialog

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers how the game puts words on screen: the proportional dialog font and its accented
cells, the field dialog pager (typing, scrolling, page turns, option pickers), the inline dialogue
format inside field scripts, narration crawls, and what the PAL builds change. It is the reference
for anyone translating the game, editing dialogue, or checking that the port's text boxes pace and
branch as retail's do.

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [Pause Items/Magic screens - remaining sub-flows](#pause-itemsmagic-screens---remaining-sub-flows)
- [Inline dialog-box format (`0x1F`-lead segments)](#inline-dialog-box-format-0x1f-lead-segments)
- [PROT 0892 is the card-screen kanji font](#prot-0892-is-the-card-screen-kanji-font)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| When does a sliding picker first take input, and does the inn picker's cursor wrap? | resolved (the call after the count reaches zero; `0x2A` clamps) | `disassembly` | The call that takes the slide count to zero ends `j 0x801D95AC` at `0x801D9414`, past the cursor handler at `0x801D941C`, so the hand is drawn on that call (`0x801D9BB4`) and input is read from the next. Both edge arms of the cursor handler test for state `0x12` (`0x801D9474` / `0x801D94D0`) and store the end index instead of wrapping; every other picker wraps. Ported once as `engine-core::dialog_picker_slide`, read by both hosts through `picker_rect`. |
| Does the PAL (`SCES`) text renderer remap `0xD7` and `0xF8`? | resolved (no - retail draws the placeholder boxes) | `disassembly` + `capture` | All four PAL executables share one renderer (FR `0x80037088`, the `FUN_80036888` sibling) that addresses the glyph cell straight from the byte after one preprocessor pass (FR `0x80036BB4`); the `0xD7` / `0xF8` immediates match SCUS, the French copies of 0897..0899 add none, and no image carries a translation table. A French cold boot with drawn strings rewritten to `D7` / `F8` / `DD` shows the page's labelled boxes beside a correct `Î` from `0xDD` (`autorun_pal_glyph_cells.lua`). [`dialog-font.md`](../../formats/dialog-font.md). |
| Does a PAL executable rewrite text at draw time? | resolved (French only: `oe` / `OE` to the ligatures) | `disassembly` | The French renderer folds `oe` to `0x9C` (œ) and `OE` to `0x9D` (Œ) at `0x80036D14`; the German, Italian and Spanish executables do not, and the French script stores both letters. All four PAL renderers also treat `0xC1 n` with `n >= 8` as an x-tab to the line's start x plus `n` (FR `0x80037138`). |
| How does a dialog picker open? | resolved (it slides in from off screen) | `disassembly` + `capture` | A press in `0x19` stores the sentinel `+0x54 = 0x309`. The odd state's first call (`0x801D92F4` for `0x11`, `0x801D9350` for `0x13` / `0x15` / `0x17`) sets span `+0x50` and count `+0x54` to `0x18` plus start, target and size; each call subtracts the frame step, and at count 0 the shared tail (`0x801D93F4..0x801D9418`) steps to the even state, where the hand and input appear. The box sits at `target + (start - target) * count / span`. `0x2A` slides in from `(0x150, 0x4A)` to `(0xD8, 0x4A)` at size `(0x58, 0x1A)`; `0x27` / `0x28` / `0x29` rise from `(0x26, 0xF0)` to the N-option rect. Captured: input opens 26 vsyncs after the press. [`mes.md`](../../formats/mes.md). |
| Does the USA dialog font carry accented glyphs? | resolved (yes - 32 inked high cells) | `capture` | The font TIM at `PROT.DAT 0x7F40` (boot pack member 3) inks CP437 `0x80..0x90` / `0x95..0x9A`, `0x9C`, `0x9F`, and `0xA0..0xAC` one cell below CP437; the SCUS width table `0x80073F1C` gives 26 of the 32 a zero advance, so they overprint the next letter. Five mednafen states hold the VRAM `(896, 0)` page byte-identical to the disc TIM. Disc-byte evidence; pin `translation_accent_font_real` ([dialog-font.md](../../formats/dialog-font.md#accented-latin-cells)). |
| When does a dialog picker open, and what presses for the player? | resolved (only on a `0x19` press; `_DAT_80073F00` presses automatically) | `disassembly` + `capture` | The picker states `0x11` / `0x13` / `0x15` / `0x17` are stored only behind state `0x19`'s press test (`0x801D8FCC`; stores `0x801D9058..0x801D909C`). While `_DAT_80073F00` is positive each `0x19` call subtracts the frame step, and the call that reaches zero presses confirm (`0x801D8F4C..0x801D8F88`); a poke of 21 reads `19 .. 1` over eleven calls. Op `4C 89` is its only writer (37 sites) ([mes.md](../../formats/mes.md#post-page-dispatch-state-0x19)). |
| What does a confirm press do during a short-row hold? | resolved (clears the hold and latches skip speed `0x25`) | `capture` | On the 72-tick hold the press clears the hold and latches `0x25` on the next call; the call after goes to `0x0D`. The engine matches all 379 vsyncs of the trace (`dialog_window_disc`) ([mes.md](../../formats/mes.md#row-window-and-scrolling)). |
| Does retail wrap dialog text to the box? | resolved (no - no text surface wraps) | `disassembly` | `FUN_80036044` returns the typewriter glyph count (it is not a wrap pre-pass); `FUN_80036888` draws the expanded string with no clip or length test, and the pager `FUN_801D84D0` never measures a row. Line breaks are authored `0x7C` bytes and `0x1F` lines. [`dialog-font.md`](../../formats/dialog-font.md#line-width-and-wrapping). |
| How wide can a field dialog row be? | resolved (244 px at one extra pixel per glyph) | `disassembly` + `capture` | Rows draw at the box x with `DAT_800740E8 = 1` (`0x801D97D8`) in a `0xF4`-wide centre rect (`0x801D99CC`), three rows a page; the `v0_1_tetsu_dialogue_accept` display list steps glyphs `widths[c] + 2` apart. Menu, shop and battle columns: [`dialog-font.md`](../../formats/dialog-font.md#menu-shop-and-battle-columns). |
| What ends an NPC talk? | resolved (`FUN_80038050`'s verdict on the byte after the box) | `disassembly` + `capture` | `FUN_80039B7C` hands the byte after a scanned box to `FUN_80038050` (`jal` at `0x80039C84`) and ends the talk when it returns `0` (`0x80039C8C`). Its jump table `0x80010F38` (bytes `0x21..0x4C`) continues on `0x24` / `0x25` / `0x48`, the option bytes `0x27..0x2A` and `0x4C FF` / `FE`; `0x21` steps past itself and ends; every other byte ends with the cursor left on it. The retail inn capture parks on the `26 9D FE` loop-back, which the next talk runs ([`script-vm.md`](../../subsystems/script-vm.md)). |
| When does op `0x4C`'s sub-`5` acquire refuse? | resolved (two tests) | `disassembly` | In the arm `0x801E2148..0x801E21DC`: a target other than the player whose `+0x94` is zero (`0x801E2148..0x801E2164`), and a target already carrying `0x400` while the scene word `*(_DAT_801C6EA4) + 8` is `0` (`0x801E2168..0x801E218C`). Either leaves `s7 = 0` and the PC on the op. The second is the same test as op `0x43`'s acquire and the dispatcher's halted-target early-out. |
| Where does an NPC's first dialog line start when it opens on an escape? | resolved (at the escape) | `disassembly` | 113 of the 1586 talkable partition-1 records open their first line on an escape, `1F C1 00 ...` (the lead's name) the common case; the `0x00` is `C1`'s argument, not a terminator, so a scan to the first `0x00` starts one line late. `man_field_scripts::first_inline_dialog_offset` walks lines with `dialog_box::line_end` ([`mes.md`](../../formats/mes.md)). |
| What draws the battle command chips' words? | resolved (text: placement-record payload strings) | `disassembly` | Each chip is a screen-element placement record whose `+0x14` payload points at a string: `Auto`, `Command`, `Attack`, `Item`, `Run`, `Begin` in the executable's small-data pool `0x8007B658..0x8007B690`, `Spirit` and the Ra-Seru names in the battle overlay ([`battle.md`](../../subsystems/battle.md#where-the-words-come-from)). |
| How does a localized build lay out its UI strings? | resolved (by length, so pools move while their references stay) | `disassembly` | The Spanish build's `Automatico` sits in another pool from the USA `Auto` while the placement record pointing at it is the same record: strings of up to eight bytes land in the `$gp` small-data pool, longer ones in read-only data (the compiler rule is `inference`). The lift pairs a pool string through the code and data words that reference it ([`pal-localizations.md`](../../tooling/pal-localizations.md)). |
| Do PAL builds keep the narration crawls' page counts? | resolved (no - they reflow with blank pages) | `disassembly` | A crawl's page count is part of the script (`CC F8 80 N`), and the PAL builds reflow a block into more pages with blank `" "` pages between paragraphs - `opdeene` is 14 + 8 pages on USA and 16 + 9 on the Spanish disc ([`pal-localizations.md`](../../tooling/pal-localizations.md)). |
| Where does the narration crawl's geometry come from? | resolved (a scene config block seeded by `CC F8 E8`) | `disassembly` + `capture` | `FUN_80037174` reads `*0x801C6EA4` `+0x4C` top, `+0x4E` line slots, `+0x50` divisor and `+0x52` release count; `FUN_8003A024` resets them to `0x40 / 8 / 4 / 0` and a seed op before each block overwrites the first three. Line pitch is a fixed 16 (`addiu s3,s3,0x10`). The cold-boot `opdeene` capture holds the frame-step floor `DAT_8007B9D8` at 3, so at divisor 4 the crawl climbs a pixel every two frames ([`cutscene.md`](../../subsystems/cutscene.md)). |
| Dialog font extraction | resolved | `capture` | The glyph page lives at VRAM `(896, 0)..(960, 256)`; its on-disc carrier is a plain 4bpp TIM at `PROT.DAT` offset `0x7F40` (framebuffer `(896, 0)`, CLUT `(0, 510)`), byte-identical to the save-state page. Two extractors: `legaia-font`'s `font-extract` from any in-game save state, and `legaia_font::Font::from_disc_tim_and_scus` from the disc alone (the browser pause menu uses it). [`dialog-font.md`](../../formats/dialog-font.md). |
| Inline dialog-box format (`0x1F`-lead segments) | resolved | `disassembly` | [details ↓](#inline-dialog-box-format-0x1f-lead-segments) |
| Tetsu 4-option spar menu mechanism | resolved | `capture` | The menu is a standard `0x29` 4-option **MES inline picker** in the sparring partner's dialogue (cursor `*(0x801C6EA4)+0x0C`; confirming **index 2** "I want to practice with you." starts the spar - live `0x03->0x09->0x15`, driven by the dialog SM not the field VM). It uses the **immediate-labels** form (labels straight after the N jump entries, no continuation byte), which `legaia_mes::picker::parse_picker_at` accepts. Engine: a `CarrierMenu` on `World::carriers.menu` presents the picker and engages the carrier only on the index-2 fight option. Tests: `parses_immediate_labels_picker`, `tetsu_spar_picker_disc`, `carrier_spar_menu_*` ([`mes.md`](../../formats/mes.md#picker-control-region-layout)). |
| Pause Items/Magic screens: remaining sub-flows | resolved | `disassembly` + `capture` | [details ↓](#pause-itemsmagic-screens---remaining-sub-flows) |
| What does PROT 0892 (`card_data`) carry, and who loads it? | resolved (the memory-card screen's JIS X 0208 level-1 kanji font; an `asset::pack` of two TIMs, not a truncated stream) | `disassembly` + `capture` | [details ↓](#prot-0892-is-the-card-screen-kanji-font) |
| Which routine samples the card-screen kanji page (VRAM `(320..447, 256..511)`, CLUT rows 475..482)? | resolved-negative (nothing does - it is loaded and parked) | `disassembly` + `capture` | A sampler would have to carry tpage `0x15`/`0x16` and CBA `0x76C0 + plane * 0x40`; a byte sweep of SCUS plus every based overlay finds neither as an immediate or a data halfword, the uploader `FUN_800198E0` records no handle, and a GPU-FIFO watch at card-screen entry (mode `0x16 -> 0x17`) draws nothing from the page. The USA build ships the Japanese glyph page and never reads it. See [`save-screen.md`](../../subsystems/save-screen.md#the-card-screens-kanji-page-is-never-sampled). |
| What is the battle plaque's element badge, and how is it selected? | resolved (a caret escape inside the monster's own name string) | `disassembly` + census | The badge is not a separate field: the name carries `^A`..`^H`, the decoder takes `letter - 'A'` as the index into the badge strip `0x8B..=0x92`, and element -> caret is the fixed permutation `[4, 3, 0, 2, 1, 5, 6, 7]`. The map is a **zero-exception bijection** over the shipped records (`^H` on Cort is no exception). Parser `MonsterRecord::plaque_badge`; see [`field-menu.md`](../../subsystems/field-menu.md#status-element-badge-on-the-roster-panel). |
| How is the text-cell placement run seeded, and what is the `overlay_0897_801dbc30` dump? | resolved (three placement records, four stores each in a fixed order; the dump is a chimera of two PROT entries) | `disassembly` | The loop is PROT 0898's `0x801D3FC0`, based at `0x80076C10 + 0x408` (placement record 43, bound `sltiu 0x2E`, so records 43..45 - not 46 scratchpad records). Per record it writes `+4`, `+0xC`, `+0xA`, `+2`, and the `+2` store carries the record's **previous** `+0xA`, read before the `+0xA` store lands. The `overlay_0897_801dbc30` dump splices PROT 0897's `0x801EA448` onto this 0898 loop past its `j 0x801EA7AC`, through 0897's over-read. See [`script-vm.md`](../../subsystems/script-vm.md#the-overlay_0897_801dbc30-dump-is-a-chimera-of-two-prot-entries). |
| What does `FUN_80036044` count past a two-byte unit? | resolved (one extra byte past the `NUL` per unit) | `disassembly` | A pre-walk sizes the loop to the string's byte length, and the loop decrements that size once per iteration whatever it consumed, so each `0xCE`, `0xCF` or substitution runs it one byte past the `NUL` and counts what is there; `0xC0` and `0xC6` count nothing. The field pager consumes the count at `0x801D8A6C` ([`mes.md`](../../formats/mes.md#fun_80036044---glyph-count)). |
| How fast does the field pager type, and when does a row end? | resolved (one unit a frame; a row ends on its glyph count) | `disassembly` | Every pager open stores speed `_DAT_801F2754 = 1` (`0x801D9118` and three siblings). A row ends when the reveal counter reaches `FUN_80036044`'s count, and a row under `0x22` units holds `(0x22 - count) * 4` in `_DAT_801F275C`, drained by `32 * DAT_1F800393` a call - `ceil((0x22 - count) / 8)` frames, at most five ([`dialog-font.md`](../../formats/dialog-font.md#typewriter-pacing)). |
| When does the field pager finish a typed row? | resolved (one call after the reveal counter passes the glyph count) | `disassembly` + `capture` | The row gate compares with `slt v1,s0,v1` at `0x801D8A7C`, so a row ends once the count is below the counter, not when they meet. Each typing call adds `min(dt, 4)` capped at three units; a short row then holds `(0x22 - count) * 4` in `_DAT_801F275C`, drained `32 * dt` per call (`0x801D866C..0x801D8690`), and a page's last row never holds (state `0x19` clears it at `0x801D865C`). A town01 trace matches the engine's `dialog_pacing` for 61 of 61 vsyncs ([`dialog-font.md`](../../formats/dialog-font.md#typewriter-pacing)). |
| What does the field pager do when a page holds more rows than the box? | resolved (it scrolls; a page ends only at a control byte) | `disassembly` + `capture` | After a row, `(b & 0x7F) < 0x20` at `0x801D8AB4` looks for another line; with the three slots full the window scrolls (state `0x0C`, scroll word `_DAT_801F2738`) and typing continues with no press. A `0x24` page turn keeps the row table and types beneath the old rows (state 5); at the page end state `0x0F` scrolls the carried rows away. A confirm while typing latches `_DAT_801F2750 = 0x25` and state `0x0D` completes the page in two calls; the page wait is `0x19`. Engine `dialog_window`; the disc test matches retail for 411 of 411 and 335 of 335 vsyncs ([`mes.md`](../../formats/mes.md)). |

### Pause Items/Magic screens - remaining sub-flows

*Status:* resolved. Evidence: `disassembly` + `capture`.

The four sub-flows of the pause Items / Magic screens, and the `0x800` row dim bit, are traced from
disassembly and ported in `engine-ui` / `pause_screens`.

| Sub-flow | Retail | Note |
|---|---|---|
| Window-14 target panel | `FUN_801D0520` | Its preview modes are the permanent-stat Water previews, not an HP-restore preview. |
| PAGE sprite | UI-icon `0x76` | |
| Kind-4 list kernel | `FUN_80032A44` + allocator `FUN_80030104` | SCUS-resident. |
| Class-`0x80..0x82` Use routes | submenus `0xA..0xD` | Single-target apply `FUN_801D8308`; Door of Light / Wind `FUN_801D8A58` / `FUN_801D8B90`; Incense `FUN_801D8D94`. |

The `0x800` dim bit is set at **build time**, never on focus:

- Writer: the SCUS content builder `FUN_80030628`, content-id-3 case. It dispatches on live window
  `+0x1C`, copied from descriptor byte `+0x0` at create (`0x80032990`).
- Rule, in order: equipment is always dim; Door ids `0x88` / `0x89` are scratchpad-gated; the
  field-usable bit `0x2`; then the applicability probe `FUN_8003043C` (battle context gates bit
  `0x4`).
- No focus-dependent write exists. The white-to-grey flip is the kernel's mode-4 park override; a
  capture shows the row words bit-identical across focus states.

Owning page: [field-menu.md](../../subsystems/field-menu.md#items-screen); the row build is at
[field-menu.md](../../subsystems/field-menu.md#use-list-row-build-content-id-3-fun_80030628).

### Inline dialog-box format (`0x1F`-lead segments)

*Status:* resolved (prologue, pager-side dispatch, option-list format, multi-segment box packing).
Evidence: `disassembly`.

Placement-NPC and event dialogue is **inline** in the field-VM interaction record, not in the scene
MES: a run of `0x1F`-lead, `0x00`-terminated segments of MES glyph bytecode, each one line. The
record is ordinary field-VM bytecode around those lines, and the dialog SM `FUN_80039B7C` runs the
field VM over it, handing a text byte to the pager `FUN_801D84D0`.

**Where the text is**

- The opcode-decoded `text_id` is a box-config id; it never resolves through
  `SceneMes::message_offset` (0 of 13 town01 placement-NPC ids resolve).
- The text is recovered structurally, not from the `0x3F` op's `len`: a text-heavy record desyncs
  under linear disassembly (a literal `>` is `0x3E`, the warp / interact opcode; ASCII punctuation
  hits the `0x37` / `0x41` yield bytes), so `len` reads empty for every town01 NPC.
- Each record holds the NPC's **whole** line set - every line of every story-state branch, with
  `Yes` / `No` labels interspersed (Village Elder 80 segments, Val 59). Multi-page speech is
  several `0x1F` segments, not `0x80..=0x9F` control bytes inside one. 36 town01 placements carry
  renderable dialogue (the sparring partner, Meta the dog, villagers, leftover "dummy" developer
  placeholders, and a developer story-flag toggle menu at placement P1[1]).
- No "box-geometry header" exists. The bytes between `script_pc0` and the first `0x1F` are the
  interaction prologue: `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51` (move to tile) /
  `0x4C 0x52` (menu-activation poll). The prologue's story-flag branches `JmpRel` to the segment
  the box opens on.

**The dialog SM `FUN_80039B7C`**

- State 0 calls the field-VM dispatcher `FUN_801DE840` on the stream and enters the pager only when
  the PC rests on a byte with `& 0x7F < 0x20` (a `0x1F` lead or the `0x21` terminator).
- It advances `actor[+0x9C]` 0 -> 1 -> 2 through the segments. Its state-2 advance masks
  `(byte & 0xF0) == 0xC0` and consumes the escape's data byte, so a `0xC?` escape whose argument is
  `0x00..=0x1E` (`0xC1 0x00`) does not end the line.
- Retail's interaction cursor is one instruction past the record's spawn-section `0x21`, not
  `script_pc0` ([`script-vm.md`](../../subsystems/script-vm.md#the-interaction-cursor-one-record-two-consecutive-scripts)).
- A raw `0x21` after the prologue has spawned a record ends the talk (`FUN_8003CF7C` breaks on
  it): `vozz` P1[7] spawns P2[13] and stops.
- The talk ends on `FUN_80038050`'s verdict on the byte after the box (see the "What ends an NPC
  talk?" row); the pager itself only clears rows and returns no status.
- Not the renderer: `FUN_8001EBEC` is a per-character TMD-pose copier (party slots 0..2, indexed by
  the slot-4 freeze flag `_DAT_8007B824`; seven `u32`s from TMD `+0x124..+0x140` or
  `+0x140..+0x15C`, gated on record flag `+0x75E`; see op `0x4C` sub-3 in
  [`script-vm.md`](../../subsystems/script-vm.md)). `FUN_8003AB2C` is the per-frame field-VM
  driver and `FUN_8003BDE0` the partition-record dispatcher.

**Box packing**

- Consecutive `0x1F` lines pack into one window of `_DAT_801F2740 = 3` rows: a `0x00` terminator
  directly followed by `0x1F` is "same box, next row". A fourth row scrolls the window (state
  `0x0C`); the page runs on to the next control byte.
- The contiguous run stops where the pool hands control back to the field VM (a non-pager control
  byte, `Dispatch::Unknown`).

**Post-page dispatch (pager state `0x19`)** reads the control byte past the box, matched as
`byte & 0x7F` (so `0xA7..0xA9` are accepted; the field corpus stores the bare form):

| Byte | Next state | Meaning |
|---|---|---|
| `0x25` | 0 | end |
| `0x24` | 3 | next page, same box (own prologue, jumps away at `0x801D916C`) |
| `0x48` | 9 | new box |
| `0x4C 0xFF` | 6 | terminate |
| `0x2A` | `0x11` -> `0x12` | inn Yes / No picker |
| `0x27` / `0x28` / `0x29` | `0x13` / `0x15` / `0x17` -> `0x14` / `0x16` / `0x18` | 2 / 3 / 4-option picker |
| other | 9 | new box |

- Each odd picker state is the slide-in (see the picker rows above).
- Three arms run the box-reset tail: states 0, 6 **and** 9. They are byte-identical over their
  `0x98` bytes except the `li v0,N` choosing the successor (0 -> 1, 6 -> 7, 9 -> `0xA`).
- `JT[1] == JT[4] == JT[7] == 0x801D8708` is the teardown (early return in state 4, so `0x24` keeps
  its rows); `JT[0xA] == 0x801D92A4` is the box-open animation. So `0x25` and `0x4C 0xFF` behave
  alike and both differ from `0x48` - the port's `End` / `Terminate` versus `NewBox` grouping.

**Option list (picker control region)**

- Layout: `[open][N * 2-byte i16 LE jump table][continuation?][N * 0x1F label segments]`.
- The continuation byte is optional: a post-page dispatch byte (`0x24` / `0x25` / `0x48` /
  `0x4C`), or absent with the labels starting at once (the immediate-labels form - the Tetsu spar
  and town01's other pickers).
- Each 2-byte entry is a signed relative jump `FUN_80038050` applies on confirm:
  `new_pc = (open + 1 + index*2) + i16_LE(entry[index])`. The entries are not the labels. The four
  `izumi` re-emissions shift every entry by one per-emission delta, and every option lands
  in-bounds.
- Each picker arm sets the box size from a per-N table and clamps the cursor at
  `*(DAT_801C6EA4 + 0xC)`; on confirm it reads the continuation byte at `open[N*2 + 1]` through
  the post-page table.

**Port**

| Piece | Location | Pin |
|---|---|---|
| First-line finder | `man_field_scripts::first_inline_dialog_offset` (printable-ratio gated); `classify_placement` -> `PlacementKind::Npc::dialog_inline` | |
| Segment pool | `dialog::decode_inline_segments` | `field_actor_placements_disc::inline_dialogue_decodes_into_full_segment_pool` |
| Prologue decode | `field_disasm::LinearWalker` | `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode` |
| Box packing | `legaia_mes::dialog_box` (`pack_box` / `pack_boxes`, `LINES_PER_BOX = 3`, `Dispatch`) | `field_dialog_boxpack_disc` (all 561 town01 boxes <= 3 lines; the spar opening = three `0x24`-chained pages -> a 4-option `Picker`; the `Mist appeared, .., but` line survives its `0xC1 0x00`) |
| Pickers | `legaia_mes::picker` (`scan_pickers` / `parse_picker_at` / `Picker::jump_target`) | `field_dialog_pickers_disc` (config `On` / `Off` / `Exit`, shop haggling, the Genesis-Tree quiz; in-bounds jumps) |
| Dialog SM | `engine_core::inline_dialogue`, `World::step_inline_dialogue` (PORT `FUN_80039B7C`) | `field_interact_dialogue_disc`, `inline_dialogue_prologue_selects_segment_by_story_flag`, `..._falls_back_when_it_cannot_reach_a_segment` |

- The dialog SM path runs the record through the field VM, so an option's branch executes its flag
  ops and scene changes before the reply. It is gated by `World::toggles.use_vm_dialogue`, the
  `play-window` default; `--simple-dialogue` types `World::npcs.dialog` through
  `OwnedDialogPanel::from_inline_dialog` instead.
- The untruncated record is kept as `World::npcs.dialog_prologue`
  (`man_field_scripts::placement_interaction_record`, entered at the interaction cursor); the
  runner falls back to the first segment when the prologue reaches none.

Owning page: [`mes.md`](../../formats/mes.md#dialog-window-pager---fun_801d84d0) (pager and
[picker layout](../../formats/mes.md#picker-control-region-layout)).

### PROT 0892 is the card-screen kanji font

*Status:* resolved. Evidence: `disassembly` + `capture`.

PROT 0892 (`card_data`) is an `asset::pack` of two TIMs carrying a JIS X 0208 level-1 kanji font
for the memory-card screen; the entry is 33 sectors.

- **Loader:** mode-22 `CARD INIT` `FUN_8002574C` only - alloc `0x19000` at `0x800257D0`, retail
  leg `li a0,0x37e` + `jal 0x8003eb98` at `0x8002580C` (raw TOC `0x37E` = extraction 0892), then a
  pack walk at `0x8002581C..0x80025850` handing each member to `FUN_800198E0`.
- **Members:** two `0x8220`-byte 4bpp TIMs, CLUT `(0, 475)`, pages `(320, 256)` / `(384, 256)`.
- **Encoding:** the CLUT is a bit-plane selector over a 1bpp font packed four planes deep - 12 px
  pitch, 2,965 inked cells (the level-1 kanji count), plane 0 in ku-ten order.

Owning page: [`data-field.md`](../../formats/data-field.md); the page is never sampled in the USA
build ([`save-screen.md`](../../subsystems/save-screen.md#the-card-screens-kanji-page-is-never-sampled)).
