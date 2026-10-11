//! **Show Super Arts on the in-battle move list**: add a character's Super Arts
//! to the Tactical-Arts list the Triangle button opens in battle - which retail
//! never draws at all - each row carrying its **name**, its **AP cost** and its
//! **arrow string**, sorted into the list by AP, and only once the player has
//! **performed** it.
//!
//! ## What retail does
//!
//! The in-battle arts list is rendered by `FUN_80034358` (`SCUS_942.54`, one
//! caller at `0x8003238C`). Read off the disassembly, not the C, it is a bare
//! `0..count` walk over the acting character's *learned* art ids, and each row
//! is drawn out of a **20-byte record** in the static arts-name table
//! `DAT_80075EC4` that a linear scan matches on `(character, id)`:
//!
//! ```text
//! 800343c4  lbu   v0,0x74d(v0)   ; count      (A)  -> [sp+0x28], the loop bound
//! 8003440c  lw    v0,0x140(gp)   ; scroll     (the page offset the pager sets)
//! 80034414  addu  a2,s7,v0       ; entry = row + scroll
//! 80034450  lbu   s2,0x74e(v0)   ; id         (B)  -> the id list at +0x74E..
//! 80034460  addiu s5,a1,0x8      ;      a1 = table base, s5 = record + 8
//! 80034464  lbu   v1,0x0(a1)     ;   require rec[+0] == character   <- scan head
//! 80034480  bne   v0,s2,...      ;   require rec[+1] == id
//! 80034488  clear a1             ;   HIT: draw the row                <- scan hit
//! 80034498  lw    a0,0x4(s5)     ;     rec[+0xC] -> name  -> FUN_80036888
//! 800344d8  lbu   s0,-0x6(s5)    ;     rec[+2]   -> AP    -> digit sprites
//! 800345e8  lw    v1,0x0(s5)     ;     rec[+8]   -> arrow glyph string
//! 8003474c  addiu s7,s7,0x1      ;   MISS: the row is silently consumed
//! ```
//!
//! `v0` at (A) is `0x80084140 + character*0x414` - the character record - and
//! at (B) it is that record plus `entry`. The character index itself is
//! `gp+0x874` (`0` Vahn / `1` Noa / `2` Gala / `3` Terra), the same 0-based
//! space the arts-name table's `rec[+0]` byte uses. Rows per page is the
//! drawable height over the row pitch, `0x90 / 0x1C` = **5**.
//!
//! The learned list is kept **sorted by id** (`FUN_801EFBFC` inserts with an
//! ascending shift, `0x801EFD64..0x801EFDB0`), and the table's ids run in
//! **descending AP** (Miracle 99, then the Hyper Arts, then the normal arts) -
//! so the list the player sees is AP-descending, and "sorted by AP" for an added
//! row means **interleaving** it, not putting it at either end.
//!
//! ## The shape of the injection
//!
//! A Super Art has no row in `DAT_80075EC4` (45 records, fifteen regular arts
//! per character) and no learned-art id. So the feature **synthesises a record**:
//! a 16-byte scratch record in dead SCUS space whose `+2` AP byte, `+8` glyph
//! pointer and `+0xC` name pointer are filled per row. Hook (B) then jumps
//! **past the scan** straight to the hit arm with `s5` pointing at that scratch
//! record, so every field is drawn by retail's own code - no re-implemented
//! renderer, and the synthetic row cannot be confused with a real one.
//!
//! | Site | VA | Stock words | Role |
//! |---|---|---|---|
//! | A count | `0x800343C4` | `lbu v0,0x74d(v0)` + `sra a2,a2,0x1f` | `count + performed` |
//! | B id | `0x80034450` | `lbu s2,0x74e(v0)` + `sltiu v1,v1,0x63` | merge the row; a Super Art row is filled and drawn |
//! | W performed | `0x801EFBCC` (PROT 0898) | `lui v1,0x801f` + `addiu v0,zero,1` | record the Super Art the applier just fired |
//! | D pager | `0x801D3748` (PROT 0898) | `0x27BDFFE8` | page while another page exists |
//!
//! ## What "performed" means here, and where it lives
//!
//! Retail has nowhere to record that a Super Art was performed: the learned
//! list holds regular-art ids only, and the Super applier `FUN_801EF9E4` marks
//! nothing. But its match arm at `0x801EFBCC` - the `sw 1 -> 0x801F696C` "a Super
//! fired" flag - runs with `t5` = the character (0-based) and `t2` = sixteen
//! times the index of the Super it matched (the replace-row offset from
//! `0x801EFB04`; `a1`, the loop index, is reused by the copy loop for the bytes
//! it copies and reads as the finisher constant there - the runtime probe
//! caught exactly that), and the character record has one **free,
//! save-persistent byte**: `+0x75D` (save-record `+0x195`), the sixteenth
//! learned-id slot, which a fifteen-art character can never reach and which
//! nothing in the corpus references. Routine (W) is a two-word detour from that
//! arm that sets bit `t2 >> 4` there and keeps a population count in the top
//! three bits:
//!
//! ```text
//! record + 0x75D  =  count << 5  |  performed mask (bit i = trigger-table row i)
//! ```
//!
//! The byte rides in the SC block like the rest of the record, so the rows
//! survive a save/load, an old save starts at zero, and a Super Art appears on
//! the list from the battle after the one it was first performed in (the list is
//! rebuilt every time it opens). It is exactly the "you have done this" flag
//! retail lacks. (W) lives in the tail of the replaced pager - battle-only code
//! in battle-only space - so it costs no SCUS bytes.
//!
//! ## What the row shows
//!
//! - **AP** is the chain's: the sum of the chain arts' `rec[+2]` AP costs, read
//!   off the disc's arts-name table at patch time. A Super Art has no AP of its
//!   own - the chain arts pay it - so the chain's total is the truthful number.
//! - **Name** is chased in RAM. Retail resolves an art record at
//!   `*(*(DAT_801C9360[character]) + 0x58) + 4 + (constant - 0x10) * 0xD0`, and
//!   the display name is that record's `+0x10` field (all measured from retail's
//!   own indexing in `FUN_8004AD80`, the same offsets [`crate::super_art_power`]
//!   edits through). Only the per-Super **byte offset from the record array**
//!   is carried (`4 + (finisher - 0x10) * 0xD0 + 0x10`, one `u16`).
//! - **Arrows** are the Super Art's **physical input** - the seven-to-nine
//!   arrows the player actually types, derived from the trigger pattern with the
//!   retail tokenizer ([`legaia_art::tokenize`]: arts overlap, so a Super's
//!   input is shorter than its chain arts laid end to end; Tri-Somersault is
//!   `↑↓↑↑↑↓↑`). They are carried two bits per arrow (`[count][3 bytes]` per
//!   Super, three bits per arrow: its direction and whether a sub-art ends on
//!   it) and expanded per row into a `[count][0x81 0xA8+dir]*` glyph string in
//!   the same layout retail's own strings use, which the scratch record's `+8`
//!   points at, with `0xFF style` markers wherever the colour changes: the
//!   default (blue) style, the regular art-end style (yellow) on an arrow a
//!   sub-art ends on, and the Miracle-Art orange on the Super Art's final arrow -
//!   so Tri-Somersault reads blue blue yellow blue yellow blue orange: the ends of
//!   Somersault and Cyclone, then the Super's own trigger.
//!
//! ## Where the row sits
//!
//! Hook (B) merges two sorted sequences on the fly. The learned list is
//! id-sorted (= AP-descending); the character's five Super Arts are carried
//! **sorted by chain AP**, each with a **threshold id** `thr` = the lowest id of
//! that character whose AP is at or below the Super's - so a Super Art precedes
//! every learned id `>= thr` and follows the rest. For row `entry` the routine
//! walks both sequences, skipping Super Arts not yet performed, until it has
//! passed `entry` rows; a learned row replays the stock `lbu` at the merged
//! index, a Super Art row fills the scratch record and enters the hit arm.
//!
//! ## Battle-only by construction
//!
//! `FUN_80034358` is reached only through the shared window-content dispatcher
//! `FUN_80031D00`, which also runs from the field and menu overlays. The name
//! chase dereferences `DAT_801C9360`, which is meaningful only while the battle
//! overlay is resident, so both (A) and (B) gate on the master game-mode selector
//! `_DAT_8007B83C == 0x15` ([`BATTLE_MODE`], the same word `FUN_80031D00` itself
//! compares against `0x15` at its entry). Outside battle neither adds a row.
//! Terra has no Super Arts and no writer ever sets her byte, so her list stays
//! retail's.
//!
//! ## (D) is a replacement, not a detour
//!
//! `FUN_801D3748` is an 81-instruction leaf with exactly one caller
//! (`jal` at `0x801D21BC`) and no reference to its interior from anywhere -
//! every branch into its body comes from inside itself. So the whole body is
//! rewritten in place in the overlay. Retail steps the page offset
//! `0 -> 5 -> 10` and then closes the list; the replacement steps while
//! `scroll + 5 < learned + performed` (capped at [`MAX_SCROLL`]), reading the
//! same record byte (B) does. Its spare tail hosts (W).
//!
//! ## Placement
//!
//! Four verified-dead SCUS regions, all of them shared with `--shiny-seru`,
//! `--arts-ap-grant` / `--arts-ap-cost` and `--delilas-challenge`, so this
//! toggle stays in that **mutually exclusive** set:
//!
//! | Region | Holds |
//! |---|---|
//! | [`SCUS_GAP_VA`] (256 B) | routine (B), the scratch record and the glyph buffer |
//! | [`ARENA1_VA`] (256 B) | the row-fill routine (F) and the packed arrows |
//! | [`ARENA2_VA`] (72 B) | routine (A) |
//! | [`SLOT6_VA`] (68 B) | the fifteen 4-byte Super Art records |
//!
//! ## Known cosmetic gap
//!
//! `FUN_801D3444`'s caption thresholds (`< 6`, `< 11`) stay retail, so the
//! Triangle prompt can read "View Hyper Arts list" on a page where it should
//! read "View Next page". The list contents are correct; only that one caption
//! string is stale.
//!
//! No Sony bytes are embedded: the routines are the patcher's own code, and
//! every table entry is derived from the user's own disc (AP costs, thresholds
//! and inputs out of `SCUS_942.54`'s arts-name table) or from
//! [`legaia_art::SUPER_ARTS`], the repo's own capture-validated trigger table.

mod hook_asm;
mod injection;
mod rows;

pub use injection::SuperArtListInjection;
pub use rows::{SuperArtRow, arrow_code, super_art_rows};

/// PROT entry of the battle-action overlay that hosts the pager and the applier.
pub const OVERLAY_PROT_INDEX: usize =
    legaia_game_tables::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base of that overlay: a VA maps to raw-entry file offset `va - base`.
pub const OVERLAY_BASE_VA: u32 = legaia_game_tables::move_power::BATTLE_OVERLAY_BASE;

/// Super Arts per character, and therefore the most rows the feature can add.
pub const SUPER_ARTS_PER_CHAR: usize = 5;
/// Characters with an arts list (`0` Vahn / `1` Noa / `2` Gala). Terra (`3`) has
/// no arts-name-table rows and is left at retail.
pub const ARTS_CHARACTERS: usize = 3;
/// Rows drawn per page (`drawable height 0x90 / row pitch 0x1C`).
pub const PAGE_ROWS: u32 = 5;
/// Highest page offset the replacement pager will step to. The id list holds 16
/// slots and the feature adds at most 5 rows, so 21 rows = five pages
/// (`0/5/10/15/20`).
pub const MAX_SCROLL: u16 = 20;
/// Difference between an art's **display id** (the arts-name table's `rec[+1]`,
/// which is also what the learned list stores) and its **action constant** (what
/// a Super Art's trigger chain is written in): constant = id + `0x1B`.
pub const ART_CONSTANT_BIAS: u8 = 0x1B;
/// Position in a character's table from which an art is a **normal** art
/// (`FUN_801EED1C`'s `t5 == 0` arm; ordinals `0..4` are the Miracle Art and the
/// three Hyper Arts). Only normal arts feed the tokenizer the input derivation
/// runs, exactly as retail's normal-art arm is the one that writes tokens.
pub const FIRST_NORMAL_ORDINAL: usize = 4;
/// Longest Super Art input the packing carries: nine three-bit arrows plus a
/// four-bit count fill the 32-bit word (`9 * 3 + 4 = 31`); every retail Super
/// is typed in 7..=9.
pub const MAX_INPUT_ARROWS: usize = 9;
/// Most style markers a row can carry: the style changes at most twice around
/// each non-final art end and once before the final arrow (`2 * 2 + 1` for a
/// three-art chain).
pub const MAX_MARKERS: usize = 5;

// --- Hook sites (all byte-verified against the US build) ---------------------

/// (A) `lbu v0,0x74d(v0)` - the learned-art count that bounds the walk.
pub const HOOK_COUNT_VA: u32 = 0x8003_43C4;
pub(crate) const HOOK_COUNT_W0: u32 = 0x9042_074D;
/// `sra a2,a2,0x1f` - the displaced second word (`a2` is read again at
/// `0x800343E0`, so the routine has to replay it).
pub(crate) const HOOK_COUNT_W1: u32 = 0x0006_37C3;
const COUNT_RET_VA: u32 = HOOK_COUNT_VA + 8;

/// (B) `lbu s2,0x74e(v0)` - the per-row learned art id.
pub const HOOK_ID_VA: u32 = 0x8003_4450;
pub(crate) const HOOK_ID_W0: u32 = 0x9052_074E;
/// `sltiu v1,v1,0x63` - the displaced second word (the table-terminator test).
pub(crate) const HOOK_ID_W1: u32 = 0x2C63_0063;
const ID_RET_VA: u32 = HOOK_ID_VA + 8;

/// `lbu v1,0x0(a1)` - the scan loop head, fingerprinted, never written: a
/// plain row returns to the stock words in front of it.
pub const SCAN_HEAD_VA: u32 = 0x8003_4464;
pub(crate) const SCAN_HEAD_W: u32 = 0x90A3_0000;
/// `clear a1` - the first instruction of the scan's **hit** arm. A Super Art row
/// jumps straight here with `s5` already pointing at the scratch record, so the
/// `(character, id)` compares never run.
pub const SCAN_HIT_VA: u32 = 0x8003_4488;
pub(crate) const SCAN_HIT_W: u32 = 0x0000_2821;

/// The proportional-text renderer the list draws every art name through -
/// fingerprinted because the injected name pointer is what it will be handed.
pub const GLYPH_FN_VA: u32 = 0x8003_6888;
pub(crate) const GLYPH_FN_W0: u32 = 0x27BD_FFC0;

/// (D) `FUN_801D3748`, the Triangle list pager, in the battle-action overlay.
pub const PAGER_VA: u32 = 0x801D_3748;
/// Original body length in words - the replacement plus routine (W) must fit.
pub const PAGER_WORDS: usize = 81;
pub(crate) const PAGER_W0: u32 = 0x27BD_FFE8;
/// `lw v0,0x598(a1)` - the pad-mask read that gates the whole routine.
pub(crate) const PAGER_MASK_W: u32 = 0x8CA2_0598;
/// `lbu v1,0x74d(v0)` - the pager's own read of the learned-art count.
pub(crate) const PAGER_COUNT_W: u32 = 0x9043_074D;
/// `addiu sp,sp,0x18` - the last word of the body.
pub(crate) const PAGER_LAST_W: u32 = 0x27BD_0018;
/// The list-open sound cue the pager tails into.
const PAGER_SOUND_FN_VA: u32 = 0x8003_19A8;

/// (W) the Super applier's match arm: `lui v1,0x801f` / `addiu v0,zero,1` /
/// `sw v0,0x696c(v1)` (the "a Super fired" flag) / `addiu a1,zero,5`. Entered
/// both by fall-through and by the `beq v0,zero` at `0x801EFB24`, with `t5` =
/// the character (0-based) and `t2` = the matched trigger-table row `* 16`.
pub const HOOK_PERFORMED_VA: u32 = 0x801E_FBCC;
pub(crate) const HOOK_PERFORMED_W0: u32 = 0x3C03_801F;
pub(crate) const HOOK_PERFORMED_W1: u32 = 0x2402_0001;
/// The flag store the routine returns to (`sw v0,0x696c(v1)`), fingerprinted.
pub const PERFORMED_RET_VA: u32 = HOOK_PERFORMED_VA + 8;
pub(crate) const PERFORMED_RET_W: u32 = 0xAC62_696C;
/// The applier's entry word, fingerprinted so a re-based overlay is refused.
pub const APPLIER_VA: u32 = 0x801E_F9E4;
pub(crate) const APPLIER_W0: u32 = 0x27BD_FFF8;

// Globals the routines touch, all read straight out of the retail code above.
/// `gp+0x874` - the acting character index (0-based).
const GP_CHARACTER: u16 = 0x874;
/// Base of the four live character records; `+ character*0x414` is one record.
const CHAR_RECORD_BASE: u32 = 0x8008_4140;
/// `record + 0x74D` - the learned-art count.
const LEARNED_COUNT_OFF: u16 = 0x74D;
/// `record + 0x74E` - the first of sixteen learned-art id slots.
const LEARNED_LIST_OFF: u16 = 0x74E;
/// `record + 0x75D` - the sixteenth id slot, never reachable (fifteen arts per
/// character) and referenced by nothing: the performed byte.
pub const PERFORMED_OFF: u16 = 0x75D;
/// The performed byte's mask bits (`bit i` = trigger-table row `i`).
pub const PERFORMED_MASK: u8 = 0x1F;
/// The performed byte's count field starts at this bit.
pub const PERFORMED_COUNT_SHIFT: u32 = 5;
/// Master game-mode selector `_DAT_8007B83C` (`sh`-stored `u16`).
const GAME_MODE_VA: u32 = 0x8007_B83C;
/// The game mode the arts list is meaningful in.
pub const BATTLE_MODE: u16 = 0x15;
/// `DAT_801C9360` - the per-character battle command-data pointer table.
const CMD_TABLE_VA: u32 = 0x801C_9360;
/// `+0x58` of a command-data block: the pointer the art record array hangs off.
const ART_BLOCK_PTR_OFF: u16 = 0x58;
/// The `+4` retail adds to that pointer to reach art-record grid index `0`.
const ART_BLOCK_BIAS: u32 = 4;
/// Per-art-record stride inside that array.
pub const ART_RECORD_STRIDE: u32 = 0xD0;
/// `record + 0x10` - the record's display-name field.
const ART_NAME_FIELD_OFF: u32 = 0x10;

// Scratch-record field offsets, in the arts-name-table record's own layout.
/// `+2` - the AP cost the digit loop draws.
const SCRATCH_AP_OFF: u16 = 0x2;
/// `+8` - the command-glyph string pointer (aimed at the glyph buffer).
const SCRATCH_ARROWS_OFF: u16 = 0x8;
/// `+0xC` - the display-name pointer.
const SCRATCH_NAME_OFF: u16 = 0xC;
/// Bytes of scratch record reserved (the record's own stride is `0x14`, but only
/// `+0..+0x10` is ever read through `s5 = record + 8`).
pub const SCRATCH_BYTES: usize = 0x10;
/// The glyph buffer: `[count]` then up to [`MAX_INPUT_ARROWS`] arrow glyphs and
/// [`MAX_MARKERS`] style markers, two bytes each.
pub const GLYPH_BUF_BYTES: usize = 1 + 2 * (MAX_INPUT_ARROWS + MAX_MARKERS);
/// High byte of a style-marker glyph. The renderer takes `0xFF lo` as "draw
/// every following glyph in style `lo`" and gives it no width; the style is the
/// arrow sprite's CLUT (`gp+0x13c` -> CLUT id `0x7F86 + style`, VRAM row 510).
pub const MARKER_HI: u8 = 0xFF;
/// The default arrow style every row starts in - blue.
pub const STYLE_DEFAULT: u8 = 1;
/// The style regular rows switch to before their last arrow - yellow.
pub const STYLE_ART_END: u8 = 6;
/// The style a Super Art row switches to before its final arrow - the Miracle
/// Art orange (style 9; Hyper Arts are all-yellow; style 2 on the same row is
/// a red retail never uses here).
pub const STYLE_SUPER_END: u8 = 9;
/// High byte of every arrow glyph, and the low byte of the first (Right); the
/// four arrows are `0xA8..=0xAB` = Right / Left / Up / Down.
pub const GLYPH_HI: u8 = 0x81;
pub const GLYPH_LO_BASE: u8 = 0xA8;

/// One Super Art's 4-byte record in the injected table (`SUP`), in per-character
/// **AP-sorted** order: `+0` chain AP, `+1` `thr | bit << 5`, `+2` name offset
/// (`u16`, from the art-record array).
pub const SUP_STRIDE: u32 = 4;
/// Per-Super packed arrows, one little-endian `u32`: bits `3k..3k+1` = arrow
/// `k`'s direction code, bit `3k+2` = "an art ends on this arrow", bits
/// `27..30` = the arrow count.
pub const ARROWS_STRIDE: u32 = 4;
/// Bits per packed arrow (`dir dir end`).
pub const ARROW_BITS: u32 = 3;
/// Bit position of the arrow count inside the packed word.
pub const ARROWS_COUNT_SHIFT: u32 = 27;
/// Bit position of the marker count inside the `SUP` record's name-offset
/// halfword (name offsets stay under `0x2000`).
pub const MARKERS_SHIFT: u32 = 13;

#[cfg(test)]
mod tests;
