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

use anyhow::{Result, bail};

use legaia_art::queue::{ActionConstant, Character, Command};

use crate::mips::*;
use crate::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, Edit, OVERLAY_TABLE_RANGES,
    SCUS_GAP_END_VA, SCUS_GAP_VA, SCUS_TABLE_RANGES, SLOT6_END_VA, SLOT6_VA,
};
use crate::super_art_power::{GRID_BIAS, super_arts_for};

/// PROT entry of the battle-action overlay that hosts the pager and the applier.
pub const OVERLAY_PROT_INDEX: usize = legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base of that overlay: a VA maps to raw-entry file offset `va - base`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

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
/// Two-bit arrow codes = glyph low byte minus [`GLYPH_LO_BASE`].
pub fn arrow_code(c: Command) -> u8 {
    match c {
        Command::Right => 0,
        Command::Left => 1,
        Command::Up => 2,
        Command::Down => 3,
    }
}

// --- Routine assembly --------------------------------------------------------

/// Word offset of a PC-relative branch from `from` (the branch's own address)
/// to `to`. Branches reach ±128 KB, so every dead-space region can be a target.
fn br(from: u32, to: u32) -> i16 {
    let d = (to as i64 - (from as i64 + 4)) / 4;
    i16::try_from(d).expect("branch target in range")
}

/// (A) `count -> count + performed`. `v0` is the character record when the hook
/// fires, so the performed byte is read **before** the displaced `lbu`
/// replaces it with the learned count; the count field is its top three bits.
/// Gated on battle mode. `disp = [lbu v0,0x74d(v0), sra a2,a2,0x1f]`.
pub(crate) fn assemble_count(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        lbu(T1, V0, PERFORMED_OFF),                    // 0  t1 = performed byte
        disp[0],                                       // 1  v0 = learned count
        lui(T3, hi(GAME_MODE_VA)),                     // 2
        lh(T3, T3, lo(GAME_MODE_VA)),                  // 3
        srl(T1, T1, PERFORMED_COUNT_SHIFT),            // 4  performed count
        addiu(T3, T3, (-(BATTLE_MODE as i32)) as u16), // 5  mode - 0x15
        bne(T3, ZERO, 2),                              // 6  not in battle -> skip
        nop(),                                         // 7  delay
        addu(V0, V0, T1),                              // 8  count += performed
        j(ret),                                        // 9
        disp[1],                                       // 10 delay: sra a2,a2,0x1f
    ]
}

/// (B) resolve the row's art id by merging the id-sorted learned list with the
/// character's AP-sorted, performed Super Arts.
///
/// Entered with `v0` = record + `entry`, `a2` = `entry`, `a0` = `0x80070000`
/// (the table-base `lui`, read again by the word after the hook - preserved) and
/// `v1` = the table's first byte (the displaced `sltiu` tests it - preserved).
/// Clobbers `t0`-`t9` and `s2`/`s5`, all dead or about to be written by retail
/// at this point; `ra` is dead (spilled in the prologue). Uses no `mult`/`div`.
///
/// A learned row lands on `PLAIN` with `t4` = its index into the id list and
/// replays the stock `lbu` there; a Super Art row branches (cross-region) to the
/// fill routine `fill_va` with `t5` = the character's `SUP` base, `t6` = the
/// Super's sorted index and `t8` = the character `* 4`.
///
/// `disp = [lbu s2,0x74e(v0), sltiu v1,v1,0x63]`.
pub(crate) fn assemble_id(
    disp: [u32; 2],
    base_va: u32,
    sup_va: u32,
    fill_va: u32,
    ret: u32,
) -> Vec<u32> {
    const LOOP: i32 = 20;
    const PICKSUP: i32 = 39;
    const NEXTK: i32 = 41;
    const PICKLRN: i32 = 43;
    const PLAIN0: i32 = 47;
    const PLAINI: i32 = 48;
    let at = |i: i32| base_va + (i as u32) * 4;
    let body = vec![
        subu(V0, V0, A2),                              // 0  v0 = the character record
        lui(T3, hi(GAME_MODE_VA)),                     // 1
        lh(T3, T3, lo(GAME_MODE_VA)),                  // 2
        lbu(T0, V0, PERFORMED_OFF),                    // 3  t0 = performed byte
        addiu(T3, T3, (-(BATTLE_MODE as i32)) as u16), // 4  mode - 0x15
        bne(T3, ZERO, (PLAIN0 - 6) as i16),            // 5  not in battle -> plain
        andi(T0, T0, u16::from(PERFORMED_MASK)),       // 6  delay: the mask
        beq(T0, ZERO, (PLAIN0 - 8) as i16),            // 7  nothing performed -> plain
        lw(T8, GP, GP_CHARACTER),                      // 8  delay: character
        lbu(T2, V0, LEARNED_COUNT_OFF),                // 9  t2 = learned count
        addiu(T9, V0, LEARNED_LIST_OFF),               // 10 t9 = the id list
        sll(T8, T8, 2),   // 11 t8 = character * 4 (F keys the command table with it)
        sll(T7, T8, 2),   // 12
        addu(T7, T7, T8), // 13 character * 20 = * 5 * SUP_STRIDE
        lui(T5, hi(sup_va)), // 14
        addiu(T5, T5, lo(sup_va)), // 15
        addu(T5, T5, T7), // 16 t5 = &SUP[character*5]
        or(T4, ZERO, ZERO), // 17 i = 0 (learned cursor)
        or(T6, ZERO, ZERO), // 18 k = 0 (Super cursor)
        or(T7, A2, ZERO), // 19 r = entry (rows to pass)
        // LOOP: pick the next merged row.
        sltiu(T3, T6, SUPER_ARTS_PER_CHAR as u16), // 20 k < 5 ?
        beq(T3, ZERO, (PICKLRN - 22) as i16),      // 21 no Super left -> learned
        sll(T3, T6, 2),                            // 22 delay: k * 4
        addu(T3, T3, T5),                          // 23 t3 = &SUP[k]
        lbu(T1, T3, 1),                            // 24 thr | bit << 5
        nop(),                                     // 25
        srl(T1, T1, PERFORMED_COUNT_SHIFT),        // 26 the Super's bit
        srlv(T1, T0, T1),                          // 27 mask >> bit
        andi(T1, T1, 1),                           // 28
        beq(T1, ZERO, (NEXTK - 30) as i16),        // 29 not performed -> next k
        sltu(T1, T4, T2),                          // 30 delay: i < learned count ?
        beq(T1, ZERO, (PICKSUP - 32) as i16),      // 31 learned exhausted -> Super
        lbu(T1, T3, 1),                            // 32 delay: thr | bit << 5
        addu(T3, T9, T4),                          // 33
        lbu(T3, T3, 0),                            // 34 t3 = list[i]
        andi(T1, T1, u16::from(PERFORMED_MASK)),   // 35 t1 = thr
        sltu(T1, T3, T1),                          // 36 list[i] < thr -> learned first
        bne(T1, ZERO, (PICKLRN - 38) as i16),      // 37
        nop(),                                     // 38
        // PICKSUP: the Super Art at k is the next row.
        beq(T7, ZERO, br(at(PICKSUP), fill_va)), // 39 r == 0 -> fill and draw it
        addiu(T7, T7, 0xFFFF),                   // 40 delay: r -= 1 (F ignores t7)
        // NEXTK:
        j(at(LOOP)),      // 41
        addiu(T6, T6, 1), // 42 delay: k += 1
        // PICKLRN: the learned art at i is the next row.
        beq(T7, ZERO, (PLAINI - 44) as i16), // 43 r == 0 -> it is this row
        addiu(T7, T7, 0xFFFF),               // 44 delay: r -= 1 (PLAINI ignores t7)
        j(at(LOOP)),                         // 45
        addiu(T4, T4, 1),                    // 46 delay: i += 1
        // PLAIN0: no Super Art rows at all - the learned index is the entry.
        or(T4, A2, ZERO), // 49
        // PLAINI: learned index i -> the stock load, re-based.
        addu(V0, V0, T4),              // 50
        lbu(S2, V0, LEARNED_LIST_OFF), // 51 == disp[0]
        j(ret),                        // 52
        disp[1],                       // 53 delay: sltiu v1,v1,0x63
    ];
    debug_assert_eq!(body[51], disp[0], "the learned path replays the stock load");
    debug_assert_eq!(body.len(), PLAINI as usize + 4);
    body
}

/// (F) fill the scratch record for the Super Art (B) picked and enter the draw.
///
/// Entered from (B) with `t5` = `&SUP[character*5]`, `t6` = the Super's sorted
/// index `k`, `t8` = the character `* 4`. Writes the scratch record's `+2` AP
/// and `+0xC` name pointer (chased through `DAT_801C9360[character] -> +0x58 ->
/// + name offset`), expands the Super's packed arrows into the glyph buffer the
/// scratch record's own `+8` points at, then jumps to the scan's hit arm with
/// `s5` = scratch + 8.
///
/// The glyph string is `[n + markers]` then, per arrow, a `0xFF style` marker
/// whenever the style changes and the arrow glyph: default style, the
/// art-end style on an arrow a sub-art ends on, the Super-end style on the
/// final arrow - so Tri-Somersault reads blue blue yellow blue yellow blue orange.
/// The packed word is consumed three bits at a time (`dir dir end`), the count
/// having been read off its top bits first.
///
/// The packed arrows are addressed **relative to the SUP record**: both tables
/// share the `character*5 + k` order and the 4-byte stride, so
/// `&arrows[row] = &SUP[row] + (arrows_va - sup_va)` - which is what keeps a
/// Noa or Gala row from reading Vahn's arrows.
pub(crate) fn assemble_fill(
    base_va: u32,
    sup_va: u32,
    arrows_va: u32,
    scratch_va: u32,
) -> Vec<u32> {
    const LOOP: i32 = 25;
    const CMP: i32 = 32;
    const GLYPH: i32 = 38;
    let delta = arrows_va.wrapping_sub(sup_va);
    let body = vec![
        sll(T3, T6, 2),                          // 0
        addu(T3, T3, T5),                        // 1  t3 = &SUP[k]
        lbu(T1, T3, 0),                          // 2  t1 = chain AP
        lhu(T7, T3, 2),                          // 3  t7 = name offset | markers << 13
        lui(T9, hi(CMD_TABLE_VA)),               // 4
        addu(T9, T9, T8),                        // 5  + character * 4
        lw(T9, T9, lo(CMD_TABLE_VA)),            // 6  the command-data block
        lui(T5, hi(scratch_va)),                 // 7  (delay filler)
        lw(T9, T9, ART_BLOCK_PTR_OFF),           // 8  the art-record array
        addiu(T5, T5, lo(scratch_va)),           // 9  t5 = scratch (filler)
        srl(T6, T7, MARKERS_SHIFT),              // 10 t6 = marker count
        andi(T7, T7, (1 << MARKERS_SHIFT) - 1),  // 11 t7 = name offset
        addu(T9, T9, T7),                        // 12 t9 = the name
        sb(T1, T5, SCRATCH_AP_OFF),              // 13 scratch[+2]   = AP
        sw(T9, T5, SCRATCH_NAME_OFF),            // 14 scratch[+0xC] = name
        lui(T2, hi(delta)),                      // 15
        addu(T3, T3, T2),                        // 16 &SUP[k] + hi(delta)
        lw(T3, T3, lo(delta)),                   // 17 t3 = the packed arrows word
        lw(T9, T5, SCRATCH_ARROWS_OFF),          // 18 t9 = the glyph buffer
        srl(T4, T3, ARROWS_COUNT_SHIFT),         // 19 t4 = arrow count n
        addu(T6, T6, T4),                        // 20 glyph count = markers + n
        sb(T6, T9, 0),                           // 21 buf[0]
        addiu(T9, T9, 1),                        // 22
        or(T8, ZERO, ZERO),                      // 23 k = 0
        ori(T6, ZERO, u16::from(STYLE_DEFAULT)), // 24 current style
        // LOOP: k counts the arrow being drawn, one-based, from here on.
        addiu(T8, T8, 1),                          // 25 k += 1
        andi(T7, T3, 4),                           // 26 an art ends here ?
        beq(T7, ZERO, (CMP - 28) as i16),          // 27 no -> default style
        ori(T1, ZERO, u16::from(STYLE_DEFAULT)),   // 28 delay: want = default
        bne(T8, T4, (CMP - 30) as i16),            // 29 not the last arrow ->
        ori(T1, ZERO, u16::from(STYLE_ART_END)),   // 30 delay: want = art end
        ori(T1, ZERO, u16::from(STYLE_SUPER_END)), // 31 the final arrow
        // CMP: emit a marker only when the style changes.
        beq(T1, T6, (GLYPH - 33) as i16), // 32 same style -> no marker
        ori(T2, ZERO, u16::from(MARKER_HI)), // 33 delay
        sb(T2, T9, 0),                    // 34
        sb(T1, T9, 1),                    // 35
        addiu(T9, T9, 2),                 // 36
        or(T6, T1, ZERO),                 // 37 current = want
        // GLYPH: the arrow itself.
        andi(T1, T3, 3),                         // 38 dir 0..3
        addiu(T1, T1, u16::from(GLYPH_LO_BASE)), // 39 low byte
        ori(T2, ZERO, u16::from(GLYPH_HI)),      // 40
        sb(T2, T9, 0),                           // 41
        sb(T1, T9, 1),                           // 42
        srl(T3, T3, ARROW_BITS),                 // 43 next packed arrow
        sltu(T2, T8, T4),                        // 44 k < n ?
        bne(T2, ZERO, (LOOP - 46) as i16),       // 45
        addiu(T9, T9, 2),                        // 46 delay
        j(SCAN_HIT_VA),                          // 47 draw it
        addiu(S5, T5, SCRATCH_ARROWS_OFF),       // 48 delay: s5 = scratch + 8
    ];
    debug_assert_eq!(base_va % 4, 0);
    body
}

/// (W) record the Super Art the applier just matched, in place of the match
/// arm's first two words. `t5` = the character; the matched trigger-table row
/// arrives as `t2 = row * 16` (the replace-row offset `sll t2,a1,0x4` at
/// `0x801EFB04`, which the copy loop never writes - `a1` itself is reused by
/// that loop for the bytes it copies, so at the arm it is the last replacement
/// byte, not the row). Sets the row's bit in the record's performed byte and
/// bumps its count field once, then returns to the flag store with `v0`/`v1`
/// as retail left them.
pub(crate) fn assemble_performed(ret: u32) -> Vec<u32> {
    let body = vec![
        lui(V1, 0x801F),                           // 0  replay
        addiu(V0, ZERO, 1),                        // 1  replay
        srl(T3, T2, 4),                            // 2  row = t2 >> 4
        ori(T4, ZERO, 1),                          // 3
        sllv(T3, T4, T3),                          // 4  t3 = the row's bit
        sll(T0, T5, 6),                            // 5
        addu(T0, T0, T5),                          // 6  * 65
        sll(T0, T0, 2),                            // 7  * 260
        addu(T0, T0, T5),                          // 8  * 261
        sll(T0, T0, 2),                            // 9  * 0x414
        lui(T1, hi(CHAR_RECORD_BASE)),             // 10
        addiu(T1, T1, lo(CHAR_RECORD_BASE)),       // 11
        addu(T0, T0, T1),                          // 12 t0 = the character record
        lbu(T2, T0, PERFORMED_OFF),                // 13 t2 = performed byte
        nop(),                                     // 14 load delay
        and(T4, T2, T3),                           // 15 already set ?
        bne(T4, ZERO, 3),                          // 16 yes -> nothing to do
        or(T2, T2, T3),                            // 17 delay: set the bit
        addiu(T2, T2, 1 << PERFORMED_COUNT_SHIFT), // 18 count += 1
        sb(T2, T0, PERFORMED_OFF),                 // 19
        j(ret),                                    // 20
        nop(),                                     // 21
    ];
    debug_assert_eq!(body[0], HOOK_PERFORMED_W0);
    debug_assert_eq!(body[1], HOOK_PERFORMED_W1);
    body
}

/// (D) the whole replacement pager, assembled at [`PAGER_VA`]. Same prologue,
/// pad gate, character-record walk and open/close tail as retail; the page step
/// becomes "advance while another page exists", and the row count it compares
/// against is `learned + performed`, read from the same record byte the
/// renderer's hooks read.
pub(crate) fn assemble_pager(base_va: u32) -> Vec<u32> {
    const TOGGLE: i32 = 38;
    const EXIT: i32 = 52;
    let exit_va = base_va + (EXIT as u32) * 4;
    let body = vec![
        addiu(SP, SP, 0xFFE8),               // 0  frame
        sw(RA, SP, 0x10),                    // 1
        lui(V0, 0x8008),                     // 2
        addiu(A1, V0, 0x4140),               // 3  a1 = 0x80084140
        lw(V0, A1, 0x598),                   // 4  v0 = the accepted pad mask
        nop(),                               // 5
        and(A0, A0, V0),                     // 6
        beq(A0, ZERO, (EXIT - 8) as i16),    // 7  not our button -> return
        lui(V0, 0x8008),                     // 8  delay
        lw(V1, V0, 0xBB8C),                  // 9  v1 = character (gp+0x874)
        nop(),                               // 10
        sll(V0, V1, 6),                      // 11
        addu(V0, V0, V1),                    // 12
        sll(V0, V0, 2),                      // 13
        addu(V0, V0, V1),                    // 14
        sll(V0, V0, 2),                      // 15 v0 = character * 0x414
        addu(V0, V0, A1),                    // 16 + 0x80084140
        lbu(V1, V0, PERFORMED_OFF),          // 17 v1 = performed byte
        lbu(A0, V0, LEARNED_COUNT_OFF),      // 18 a0 = learned count
        srl(V1, V1, PERFORMED_COUNT_SHIFT),  // 19 performed count
        addu(A0, A0, V1),                    // 20 a0 = rows the list will draw
        beq(A0, ZERO, (EXIT - 22) as i16),   // 21 nothing to show -> return
        lui(V0, 0x801F),                     // 22 delay
        lbu(V0, V0, 0x4E09),                 // 23 v0 = the list-state flag
        nop(),                               // 24
        andi(V0, V0, 1),                     // 25
        beq(V0, ZERO, (TOGGLE - 27) as i16), // 26 list closed -> open it
        lui(A1, 0x8008),                     // 27 delay
        lw(V1, A1, 0xB458),                  // 28 v1 = page offset
        nop(),                               // 29
        sltiu(V0, V1, MAX_SCROLL),           // 30 offset < MAX_SCROLL ?
        beq(V0, ZERO, (TOGGLE - 32) as i16), // 31 no -> close
        addiu(V0, V1, PAGE_ROWS as u16),     // 32 delay: v0 = offset + 5
        sltu(V0, V0, A0),                    // 33 another page exists ?
        beq(V0, ZERO, (TOGGLE - 35) as i16), // 34 no -> close
        addiu(V1, V1, PAGE_ROWS as u16),     // 35 delay: v1 = offset + 5
        j(exit_va),                          // 36
        sw(V1, A1, 0xB458),                  // 37 delay: advance one page
        // TOGGLE: retail's own open/close tail, unchanged.
        lui(V0, 0x801F),                   // 38
        lbu(V1, V0, 0x4E09),               // 39
        nop(),                             // 40
        addiu(V1, V1, 1),                  // 41
        andi(A0, V1, 0x81),                // 42
        andi(V1, V1, 1),                   // 43
        beq(V1, ZERO, (EXIT - 45) as i16), // 44 closed -> return
        sb(A0, V0, 0x4E09),                // 45 delay: store either way
        lui(V0, 0x801F),                   // 46
        sb(ZERO, V0, 0x4E08),              // 47
        lui(V0, 0x8008),                   // 48
        ori(A0, ZERO, 1),                  // 49
        jal(PAGER_SOUND_FN_VA),            // 50
        sw(ZERO, V0, 0xB458),              // 51 delay: open on page 0
        // EXIT:
        lw(RA, SP, 0x10),    // 52
        nop(),               // 53
        jr(RA),              // 54
        addiu(SP, SP, 0x18), // 55
    ];
    debug_assert_eq!(body.len(), EXIT as usize + 4);
    body
}

// --- Table derivation --------------------------------------------------------

/// One Super Art's row in the injected tables, derived from the user's own disc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtRow {
    pub character: Character,
    /// Display name (from [`legaia_art::SUPER_ARTS`]) - carried for reporting
    /// only; the injected row chases the name out of RAM.
    pub name: &'static str,
    /// Action constant of the finisher, and therefore the art record's grid key.
    pub finisher: u8,
    /// Row of the character's five in the resident trigger table = the bit the
    /// applier detour sets in the performed byte.
    pub trigger_row: u8,
    /// Position among the character's five once sorted by AP (descending; ties
    /// keep trigger-table order) - the row's index in the injected `SUP` table.
    pub sorted_index: u8,
    /// Trigger-chain arts as **display ids**, in trigger order (duplicates kept).
    pub chain_ids: Vec<u8>,
    /// The chain arts' names as the disc's own arts-name table spells them.
    pub chain_names: Vec<String>,
    /// Sum of the chain arts' AP costs - what the row displays and sorts by.
    pub ap: u8,
    /// Lowest display id of this character whose AP is at or below `ap`: the
    /// Super Art is listed before every learned id `>= thr`.
    pub thr: u8,
    /// Byte offset of the record's `+0x10` name from the art-record array
    /// (`4 + (finisher - 0x10) * 0xD0 + 0x10`).
    pub name_offset: u16,
    /// The physical input - what the player types - derived with the retail
    /// tokenizer from the trigger pattern.
    pub input: Vec<Command>,
    /// Input positions the chain's arts end on (the arrows the tokenizer wrote
    /// the starter over), ascending; the last is always the final arrow.
    pub ends: Vec<usize>,
}

impl SuperArtRow {
    /// The input as `L`/`R`/`D`/`U` letters, for reports.
    pub fn input_letters(&self) -> String {
        self.input
            .iter()
            .map(|c| match c {
                Command::Left => 'L',
                Command::Right => 'R',
                Command::Down => 'D',
                Command::Up => 'U',
            })
            .collect()
    }

    /// The style each arrow is drawn in: default, [`STYLE_ART_END`] where a
    /// sub-art ends, [`STYLE_SUPER_END`] on the final arrow.
    pub fn styles(&self) -> Vec<u8> {
        let n = self.input.len();
        (0..n)
            .map(|k| {
                if k + 1 == n {
                    STYLE_SUPER_END
                } else if self.ends.contains(&k) {
                    STYLE_ART_END
                } else {
                    STYLE_DEFAULT
                }
            })
            .collect()
    }

    /// How many style markers the row's glyph string carries: one per change
    /// of style, starting from the default.
    pub fn marker_count(&self) -> usize {
        let mut cur = STYLE_DEFAULT;
        let mut n = 0;
        for st in self.styles() {
            if st != cur {
                n += 1;
                cur = st;
            }
        }
        n
    }

    /// The glyph string routine (F) builds: `[glyphs]`, then per arrow a style
    /// marker whenever the style changes and the arrow glyph itself.
    pub fn glyph_string(&self) -> Vec<u8> {
        let mut out = vec![(self.input.len() + self.marker_count()) as u8];
        let mut cur = STYLE_DEFAULT;
        for (c, st) in self.input.iter().zip(self.styles()) {
            if st != cur {
                out.extend_from_slice(&[MARKER_HI, st]);
                cur = st;
            }
            out.push(GLYPH_HI);
            out.push(GLYPH_LO_BASE + arrow_code(*c));
        }
        out
    }

    /// The packed arrows word (F) expands.
    pub fn packed_arrows(&self) -> [u8; ARROWS_STRIDE as usize] {
        let mut w: u32 = (self.input.len() as u32) << ARROWS_COUNT_SHIFT;
        for (k, &c) in self.input.iter().enumerate() {
            let end = u32::from(self.ends.contains(&k));
            w |= (u32::from(arrow_code(c)) | (end << 2)) << (k as u32 * ARROW_BITS);
        }
        w.to_le_bytes()
    }

    /// The 4-byte `SUP` record: AP, `thr | trigger_row << 5`, and the name
    /// offset with the marker count in its top three bits.
    pub fn sup_record(&self) -> [u8; SUP_STRIDE as usize] {
        let word = self.name_offset | ((self.marker_count() as u16) << MARKERS_SHIFT);
        let [lo_, hi_] = word.to_le_bytes();
        [
            self.ap,
            self.thr | (self.trigger_row << PERFORMED_COUNT_SHIFT),
            lo_,
            hi_,
        ]
    }
}

/// Derive all fifteen rows from `scus`, in `character * 5 + sorted_index`
/// order. Every chain entry has to land on a real arts-name-table row of the
/// same character, every trigger pattern has to derive a unique physical input
/// through the retail tokenizer, and every threshold has to resolve, or the
/// whole thing is refused - the checks that keep the tables honest against the
/// disc rather than against the curated table.
pub fn super_art_rows(scus: &[u8]) -> Result<Vec<SuperArtRow>> {
    let table = legaia_art::arts_table::raw_records_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: parse the SCUS arts-name table"))?;
    let names = legaia_art::arts_table::parse_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: decode the SCUS arts-name table"))?;
    let mut out = Vec::with_capacity(ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR);
    for ch in Character::all() {
        let supers = super_arts_for(ch);
        if supers.len() != SUPER_ARTS_PER_CHAR {
            bail!(
                "show-super-arts: {ch:?} has {} Super Arts, expected {SUPER_ARTS_PER_CHAR}",
                supers.len()
            );
        }
        // This character's rows, in table order (= ascending id = descending AP).
        let mut mine: Vec<_> = table.iter().filter(|r| r.character == ch).collect();
        mine.sort_by_key(|r| r.index);
        if mine.is_empty() {
            bail!("show-super-arts: {ch:?} has no arts-name-table rows");
        }
        // The tokenizer catalog: normal arts only, grid order.
        let catalog: Vec<(ActionConstant, &[Command])> = mine
            .iter()
            .enumerate()
            .filter(|(ordinal, _)| *ordinal >= FIRST_NORMAL_ORDINAL)
            .filter_map(|(_, r)| {
                let c = ActionConstant::from_byte(r.index.checked_add(ART_CONSTANT_BIAS)?)?;
                Some((c, r.commands.as_slice()))
            })
            .collect();

        let mut rows: Vec<SuperArtRow> = Vec::with_capacity(SUPER_ARTS_PER_CHAR);
        for (trigger_row, s) in supers.iter().enumerate() {
            let chain = s.art_sequence();
            if chain.is_empty() {
                bail!(
                    "show-super-arts: Super Art {} has an empty trigger chain - refusing",
                    s.name
                );
            }
            let mut ap: u32 = 0;
            let mut chain_ids = Vec::with_capacity(chain.len());
            let mut chain_names = Vec::with_capacity(chain.len());
            for c in chain.iter().copied() {
                let id = c.checked_sub(ART_CONSTANT_BIAS).ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: {}'s chain entry {c:#x} is below the art-constant \
                         base {ART_CONSTANT_BIAS:#x}",
                        s.name
                    )
                })?;
                let rec = mine.iter().find(|r| r.index == id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: {}'s chain art (constant {c:#x} -> id {id}) has no \
                         row in this disc's arts-name table - refusing",
                        s.name
                    )
                })?;
                ap += u32::from(rec.ap);
                chain_ids.push(id);
                chain_names.push(
                    names
                        .iter()
                        .find(|e| e.character == ch && e.index == id)
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                );
            }
            let ap = u8::try_from(ap).map_err(|_| {
                anyhow::anyhow!(
                    "show-super-arts: {}'s chain costs {ap} AP, past the one byte the row \
                     draws - refusing",
                    s.name
                )
            })?;
            if s.finisher < GRID_BIAS {
                bail!(
                    "show-super-arts: {}'s finisher constant {:#x} is below the art-record grid \
                     base {GRID_BIAS:#x} - refusing",
                    s.name,
                    s.finisher
                );
            }
            let name_offset = ART_BLOCK_BIAS
                + u32::from(s.finisher - GRID_BIAS) * ART_RECORD_STRIDE
                + ART_NAME_FIELD_OFF;
            let name_offset = u16::try_from(name_offset).map_err(|_| {
                anyhow::anyhow!("show-super-arts: {}'s name offset overflows u16", s.name)
            })?;
            // The Super sits before every id whose AP is at or below its own.
            let thr = mine
                .iter()
                .find(|r| r.ap <= ap)
                .map(|r| r.index)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: no art of {ch:?} costs {ap} AP or less - the \
                         threshold cannot resolve",
                    )
                })?;
            if thr > PERFORMED_MASK {
                bail!(
                    "show-super-arts: {}'s threshold id {thr} does not fit the five bits the \
                     record packs it into - refusing",
                    s.name
                );
            }
            let input = legaia_art::derive_super_input(&catalog, s.find).ok_or_else(|| {
                anyhow::anyhow!(
                    "show-super-arts: {}'s trigger pattern derives no unique physical input \
                     against this disc's arts-name table - refusing",
                    s.name
                )
            })?;
            if input.len() > MAX_INPUT_ARROWS {
                bail!(
                    "show-super-arts: {}'s input is {} arrows, past the {MAX_INPUT_ARROWS} \
                     the packing carries - refusing",
                    s.name,
                    input.len()
                );
            }
            let ends = legaia_art::art_ends(&catalog, &input);
            if ends.len() != chain.len() || ends.last() != Some(&(input.len() - 1)) {
                bail!(
                    "show-super-arts: {}'s input {:?} ends {} arts at {:?}, but its chain has \
                     {} - refusing",
                    s.name,
                    input,
                    ends.len(),
                    ends,
                    chain.len()
                );
            }
            if name_offset >= 1 << MARKERS_SHIFT {
                bail!(
                    "show-super-arts: {}'s name offset {name_offset:#x} collides with the marker \
                     count field - refusing",
                    s.name
                );
            }
            rows.push(SuperArtRow {
                character: ch,
                name: s.name,
                finisher: s.finisher,
                trigger_row: trigger_row as u8,
                sorted_index: 0,
                chain_ids,
                chain_names,
                ap,
                thr,
                name_offset,
                input,
                ends,
            });
        }
        for r in &rows {
            if r.marker_count() > MAX_MARKERS {
                bail!(
                    "show-super-arts: {}'s row needs {} style markers, past the {MAX_MARKERS} \
                     the buffer holds - refusing",
                    r.name,
                    r.marker_count()
                );
            }
        }
        // AP-descending, ties in trigger-table order (stable sort).
        rows.sort_by_key(|r| std::cmp::Reverse(r.ap));
        for (k, r) in rows.iter_mut().enumerate() {
            r.sorted_index = k as u8;
        }
        out.extend(rows);
    }
    Ok(out)
}

// --- Planning ----------------------------------------------------------------

/// A planned "show Super Arts" injection: every same-size write, plus the exact
/// landing addresses so an oracle can pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListInjection {
    pub edits: Vec<Edit>,
    /// The fifteen derived rows, in `character * 5 + sorted_index` order.
    pub rows: Vec<SuperArtRow>,
    pub count_va: u32,
    pub id_va: u32,
    pub fill_va: u32,
    pub performed_va: u32,
    pub sup_va: u32,
    pub arrows_va: u32,
    pub scratch_va: u32,
    pub buf_va: u32,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Resolve a SCUS VA to a file offset in the image.
fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: can't resolve SCUS VA {va:#x}"))
}

/// Read a `u32` from `scus` at `va` and require it to be `expect`.
fn expect_scus(scus: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = scus_off(scus, va)?;
    let got = read_word(scus, off)?;
    if got != expect {
        bail!(
            "show-super-arts: SCUS {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Read a `u32` from the overlay at `va` and require it to be `expect`.
fn expect_overlay(ov: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let got = read_word(ov, off)?;
    if got != expect {
        bail!(
            "show-super-arts: PROT {OVERLAY_PROT_INDEX} {va:#x} = {got:#010x}, \
             expected {expect:#010x} (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Refuse a region that overlaps a live data table, even if its bytes are zero.
fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "show-super-arts {what} region {va:#x}..+{len} overlaps live table \
                 {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm a hosted span really is all-zero dead space in this image.
fn assert_zero(scus: &[u8], va: u32, len: usize) -> Result<()> {
    let off = scus_off(scus, va)?;
    let region = scus
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("show-super-arts: {va:#x}..+{len} past end of SCUS"))?;
    if region.iter().any(|&b| b != 0) {
        bail!(
            "show-super-arts: {va:#x}..+{len} is not all-zero dead space \
             (build / collision) - refusing"
        );
    }
    Ok(())
}

/// One placed span inside a dead-space region, checked against its end.
struct Region {
    va: u32,
    end: u32,
    cursor: u32,
    name: &'static str,
}

impl Region {
    fn new(va: u32, end: u32, name: &'static str) -> Self {
        Self {
            va,
            end,
            cursor: va,
            name,
        }
    }

    /// Carve `len` bytes, rounding the start up to `align` first.
    fn take(&mut self, len: u32, align: u32, what: &str) -> Result<u32> {
        let at = self.cursor.next_multiple_of(align);
        let end = at + len;
        if end > self.end {
            bail!(
                "show-super-arts: {what} ({len} B) overruns {} {:#x}..{:#x} \
                 ({} B already used)",
                self.name,
                self.va,
                self.end,
                self.cursor - self.va
            );
        }
        self.cursor = end;
        Ok(at)
    }

    fn used(&self) -> u32 {
        self.cursor - self.va
    }
}

impl SuperArtListInjection {
    /// Plan every edit. Needs the `SCUS_942.54` image (hook sites, arts-name
    /// table, dead-space hosts) and the raw PROT 0898 entry (the pager and the
    /// applier). Refuses, without touching anything, if the build isn't the
    /// recognized US layout, a hosted region isn't dead space, a Super Art's
    /// chain or input doesn't resolve against this disc's arts-name table, or a
    /// routine overruns its region.
    pub fn plan(scus: &[u8], ov0898: &[u8]) -> Result<Self> {
        // Fingerprint every site first; capture the words the routines replay.
        let count_disp = [
            expect_scus(scus, HOOK_COUNT_VA, HOOK_COUNT_W0)?,
            expect_scus(scus, HOOK_COUNT_VA + 4, HOOK_COUNT_W1)?,
        ];
        let id_disp = [
            expect_scus(scus, HOOK_ID_VA, HOOK_ID_W0)?,
            expect_scus(scus, HOOK_ID_VA + 4, HOOK_ID_W1)?,
        ];
        // Never written, but the hooks' correctness depends on both staying put:
        // a plain row returns in front of the scan and a Super Art row enters
        // the scan's hit arm.
        expect_scus(scus, SCAN_HEAD_VA, SCAN_HEAD_W)?;
        expect_scus(scus, SCAN_HIT_VA, SCAN_HIT_W)?;
        expect_scus(scus, GLYPH_FN_VA, GLYPH_FN_W0)?;
        expect_overlay(ov0898, PAGER_VA, PAGER_W0)?;
        expect_overlay(ov0898, PAGER_VA + 0x10, PAGER_MASK_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x44, PAGER_COUNT_W)?;
        expect_overlay(ov0898, PAGER_VA + 0x12C, jal(PAGER_SOUND_FN_VA))?;
        expect_overlay(
            ov0898,
            PAGER_VA + (PAGER_WORDS as u32 - 1) * 4,
            PAGER_LAST_W,
        )?;
        expect_overlay(ov0898, APPLIER_VA, APPLIER_W0)?;
        expect_overlay(ov0898, HOOK_PERFORMED_VA, HOOK_PERFORMED_W0)?;
        expect_overlay(ov0898, HOOK_PERFORMED_VA + 4, HOOK_PERFORMED_W1)?;
        expect_overlay(ov0898, PERFORMED_RET_VA, PERFORMED_RET_W)?;
        assert_not_in_tables(
            PAGER_VA,
            (PAGER_WORDS * 4) as u32,
            OVERLAY_TABLE_RANGES,
            "pager",
        )?;

        // The fifteen rows, derived from this disc's own tables.
        let rows = super_art_rows(scus)?;
        let want = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
        if rows.len() != want {
            bail!(
                "show-super-arts: derived {} rows, expected {want}",
                rows.len()
            );
        }

        // --- Placement -------------------------------------------------------
        let mut gap = Region::new(SCUS_GAP_VA, SCUS_GAP_END_VA, "the SCUS rodata gap");
        let mut arena1 = Region::new(ARENA1_VA, ARENA1_END_VA, "arena 1");
        let mut arena2 = Region::new(ARENA2_VA, ARENA2_END_VA, "arena 2");
        let mut slot6 = Region::new(SLOT6_VA, SLOT6_END_VA, "slot 6");

        let id_len = (assemble_id(id_disp, 0, 0, 0, 0).len() * 4) as u32;
        let id_va = gap.take(id_len, 4, "the id routine")?;
        let scratch_va = gap.take(SCRATCH_BYTES as u32, 4, "the scratch record")?;
        let buf_va = gap.take(GLYPH_BUF_BYTES as u32, 1, "the glyph buffer")?;

        let fill_len = (assemble_fill(0, 0, 0, 0).len() * 4) as u32;
        let fill_va = arena1.take(fill_len, 4, "the fill routine")?;
        let arrows_va = arena1.take(want as u32 * ARROWS_STRIDE, 4, "the packed arrows")?;

        let count_len = (assemble_count(count_disp, 0).len() * 4) as u32;
        let count_va = arena2.take(count_len, 4, "the count routine")?;

        let sup_va = slot6.take(want as u32 * SUP_STRIDE, 4, "the Super Art records")?;

        // Every hosted span has to be dead space and outside every live table.
        for (r, what) in [
            (&gap, "gap 1"),
            (&arena1, "arena 1"),
            (&arena2, "arena 2"),
            (&slot6, "slot 6"),
        ] {
            assert_not_in_tables(r.va, r.used(), SCUS_TABLE_RANGES, what)?;
            assert_zero(scus, r.va, r.used() as usize)?;
        }

        // --- Assembly --------------------------------------------------------
        let count = assemble_count(count_disp, COUNT_RET_VA);
        let id = assemble_id(id_disp, id_va, sup_va, fill_va, ID_RET_VA);
        let fill = assemble_fill(fill_va, sup_va, arrows_va, scratch_va);
        debug_assert_eq!((id.len() * 4) as u32, id_len);
        debug_assert_eq!((fill.len() * 4) as u32, fill_len);

        let sup: Vec<u8> = rows.iter().flat_map(|r| r.sup_record()).collect();
        let arrows: Vec<u8> = rows.iter().flat_map(|r| r.packed_arrows()).collect();
        // The scratch record: `+8` is fixed at the glyph buffer; `+2` and `+0xC`
        // are filled per row by (F).
        let mut scratch = vec![0u8; SCRATCH_BYTES];
        let a = SCRATCH_ARROWS_OFF as usize;
        scratch[a..a + 4].copy_from_slice(&buf_va.to_le_bytes());

        // The pager replacement plus routine (W) in its tail, nop-padded out to
        // the original body length so no stale retail instruction survives.
        let mut pager = assemble_pager(PAGER_VA);
        let performed_va = PAGER_VA + (pager.len() as u32) * 4;
        let performed = assemble_performed(PERFORMED_RET_VA);
        pager.extend_from_slice(&performed);
        if pager.len() > PAGER_WORDS {
            bail!(
                "show-super-arts: the replacement pager + performed routine ({} words) does \
                 not fit FUN_801D3748 ({PAGER_WORDS} words)",
                pager.len()
            );
        }
        pager.resize(PAGER_WORDS, nop());

        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let scus_edit = |off: usize, bytes: Vec<u8>| Edit {
            prot_index: None,
            file_off: off,
            bytes,
        };
        let ov_edit = |va: u32, bytes: Vec<u8>| Edit {
            prot_index: Some(OVERLAY_PROT_INDEX),
            file_off: (va - OVERLAY_BASE_VA) as usize,
            bytes,
        };
        let edits = vec![
            // Detours over the renderer's two sites (two words each).
            scus_edit(scus_off(scus, HOOK_COUNT_VA)?, detour(count_va)),
            scus_edit(scus_off(scus, HOOK_ID_VA)?, detour(id_va)),
            // Routines + tables into the verified-dead SCUS regions.
            scus_edit(scus_off(scus, count_va)?, words_to_bytes(&count)),
            scus_edit(scus_off(scus, id_va)?, words_to_bytes(&id)),
            scus_edit(scus_off(scus, fill_va)?, words_to_bytes(&fill)),
            scus_edit(scus_off(scus, sup_va)?, sup),
            scus_edit(scus_off(scus, arrows_va)?, arrows),
            scus_edit(scus_off(scus, scratch_va)?, scratch),
            // The pager (with W in its tail), replaced whole inside the overlay,
            // and the applier's two-word detour into W.
            ov_edit(PAGER_VA, words_to_bytes(&pager)),
            ov_edit(HOOK_PERFORMED_VA, detour(performed_va)),
        ];

        Ok(Self {
            edits,
            rows,
            count_va,
            id_va,
            fill_va,
            performed_va,
            sup_va,
            arrows_va,
            scratch_va,
            buf_va,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a branch's word offset back out of an encoded instruction.
    fn br_off(w: u32) -> i32 {
        ((w & 0xffff) as u16 as i16) as i32
    }

    /// Every store opcode, so a test can assert what a routine writes.
    fn is_store(w: u32) -> bool {
        (0x28..=0x2b).contains(&(w >> 26))
    }

    /// The register an instruction writes, for the ones these routines use.
    fn dest_reg(w: u32) -> Option<u32> {
        let op = w >> 26;
        match op {
            0 => match w & 0x3f {
                0x08 => None, // jr
                _ if w == 0 => None,
                _ => Some((w >> 11) & 0x1f),
            },
            0x02 | 0x04 | 0x05 | 0x06 | 0x07 | 0x28 | 0x29 | 0x2b => None,
            0x03 => Some(RA), // jal
            _ => Some((w >> 16) & 0x1f),
        }
    }

    const ID_VA: u32 = SCUS_GAP_VA;
    const SCRATCH_VA: u32 = SCUS_GAP_VA + 0xD0;
    const BUF_VA: u32 = SCRATCH_VA + 0x10;
    const FILL_VA: u32 = ARENA1_VA;
    const ARROWS_VA: u32 = ARENA1_VA + 0xC8; // past the 49-word fill routine
    const COUNT_VA: u32 = ARENA2_VA;
    const SUP_VA: u32 = SLOT6_VA;

    fn id_routine() -> Vec<u32> {
        assemble_id([HOOK_ID_W0, HOOK_ID_W1], ID_VA, SUP_VA, FILL_VA, ID_RET_VA)
    }

    fn fill_routine() -> Vec<u32> {
        assemble_fill(FILL_VA, SUP_VA, ARROWS_VA, SCRATCH_VA)
    }

    // --- (A) the count hook --------------------------------------------------

    #[test]
    fn count_routine_reads_the_performed_byte_before_the_stock_load_clobbers_v0() {
        let r = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], COUNT_RET_VA);
        assert_eq!(r[0], lbu(T1, V0, PERFORMED_OFF), "v0 is still the record");
        assert_eq!(r[1], HOOK_COUNT_W0, "then the displaced count load");
        assert_eq!(r[3], lh(T3, T3, lo(GAME_MODE_VA)), "reads the game mode");
        assert_eq!(r[4], srl(T1, T1, 5), "count field = top three bits");
        assert_eq!(r[5], addiu(T3, T3, 0xFFEB), "mode - 0x15");
        let skip = 6 + 1 + br_off(r[6]);
        assert_eq!(skip, 9, "outside battle the add is skipped");
        assert_eq!(r[8], addu(V0, V0, T1), "count += performed");
        assert_eq!(r[9], j(COUNT_RET_VA));
        assert_eq!(r[10], HOOK_COUNT_W1, "replays sra a2,a2,0x1f");
        assert_eq!(r.len(), 11);
        assert!(
            !r.iter().copied().any(is_store),
            "no store in the count hook"
        );
        // hi/lo are live across (A) (`mult` at 0x800343A0, `mfhi` at
        // 0x800343D4): no mult/div, and only temporaries + v0 written.
        for (i, w) in r.iter().enumerate() {
            let special = (w >> 26) == 0 && (w & 0x3f) != 0;
            assert!(
                !(special && (0x18..=0x1b).contains(&(w & 0x3f))),
                "mult at {i}"
            );
            if let Some(d) = dest_reg(*w) {
                assert!(
                    (T0..=T7).contains(&d) || d == V0 || d == A2,
                    "instruction {i} writes r{d}"
                );
            }
        }
    }

    // --- (B) the id hook -----------------------------------------------------

    #[test]
    fn id_routine_gates_and_falls_back_to_the_stock_load() {
        let r = id_routine();
        assert_eq!(r[0], subu(V0, V0, A2), "v0 becomes the record base");
        assert_eq!(r[3], lbu(T0, V0, PERFORMED_OFF), "reads the performed byte");
        assert_eq!(r[6], andi(T0, T0, 0x1F), "keeps the mask bits");
        let plain0 = r.len() - 5;
        for i in [5usize, 7] {
            let target = i as i32 + 1 + br_off(r[i]);
            assert_eq!(target as usize, plain0, "guard at {i} lands on PLAIN0");
        }
        assert_eq!(r[plain0], or(T4, A2, ZERO), "PLAIN0: index = entry");
        assert_eq!(r[plain0 + 1], addu(V0, V0, T4));
        assert_eq!(r[plain0 + 2], HOOK_ID_W0, "then replays the stock load");
        assert_eq!(r[plain0 + 3], j(ID_RET_VA));
        assert_eq!(r[plain0 + 4], HOOK_ID_W1);
    }

    #[test]
    fn id_routine_preserves_what_the_stock_words_after_it_read() {
        // 0x80034458 `beq v1,zero` tests the displaced sltiu's v1 (loaded at
        // 0x8003444c), and 0x8003445c `addiu a1,a0,0x5ec4` reads a0. Neither
        // may be written on the learned path.
        for (i, w) in id_routine().iter().enumerate() {
            if *w == HOOK_ID_W1 {
                continue; // the replayed sltiu itself writes v1
            }
            if let Some(d) = dest_reg(*w) {
                assert_ne!(d, A0, "instruction {i} writes a0");
                assert_ne!(d, V1, "instruction {i} writes v1");
                assert!(
                    (T0..=T9).contains(&d) || d == V0 || d == S2,
                    "instruction {i} writes r{d}"
                );
            }
        }
    }

    #[test]
    fn id_routine_branches_land_where_the_comments_say() {
        let r = id_routine();
        assert_eq!(r[41], j(ID_VA + 20 * 4), "NEXTK closes on LOOP");
        assert_eq!(r[45], j(ID_VA + 20 * 4), "PICKLRN's advance closes on LOOP");
        assert_eq!(21 + 1 + br_off(r[21]), 43, "no Super left -> PICKLRN");
        assert_eq!(29 + 1 + br_off(r[29]), 41, "not performed -> NEXTK");
        assert_eq!(31 + 1 + br_off(r[31]), 39, "learned exhausted -> PICKSUP");
        assert_eq!(37 + 1 + br_off(r[37]), 43, "learned first -> PICKLRN");
        assert_eq!(43 + 1 + br_off(r[43]), 48, "this learned row -> PLAINI");
        // The Super Art hit is a cross-region branch straight into (F); its delay
        // slot decrements r, which (F) never reads.
        let target = ID_VA as i64 + 39 * 4 + 4 + i64::from(br_off(r[39])) * 4;
        assert_eq!(target, FILL_VA as i64, "this Super row -> FILL");
        assert_eq!(r[40], addiu(T7, T7, 0xFFFF));
        assert_eq!(r[44], addiu(T7, T7, 0xFFFF), "PLAINI never reads t7 either");
        assert_eq!(r.len(), 52);
    }

    #[test]
    fn id_routine_load_delays_are_covered() {
        let r = id_routine();
        // lh t3 (2) -> used at 4; lbu t0 (3) -> 6; lw t8 (8) -> 11; lbu t2 (9)
        // -> 30; lbu t1 (24) -> 26; lbu t1 (32) -> 35; lbu t3 (34) -> 36.
        for (load, first_use) in [
            (2usize, 4usize),
            (3, 6),
            (8, 11),
            (9, 30),
            (24, 26),
            (32, 35),
            (34, 36),
        ] {
            let dest = (r[load] >> 16) & 0x1f;
            let filler = r[load + 1];
            let (rs, rt) = ((filler >> 21) & 0x1f, (filler >> 16) & 0x1f);
            let is_branch = matches!(filler >> 26, 0x04 | 0x05);
            assert!(
                is_branch || (rs != dest && rt != dest),
                "delay slot after the load at {load} reads it"
            );
            assert!(first_use >= load + 2, "load at {load} used too early");
        }
    }

    #[test]
    fn id_routine_never_moves_the_stack_pointer_or_stores() {
        for w in id_routine() {
            let is_addiu_sp = (w >> 26) == 0x09 && ((w >> 21) & 0x1f) == SP;
            assert!(!is_addiu_sp, "the id hook borrows retail's frame");
            assert!(!is_store(w), "the merge writes no memory");
        }
    }

    /// Model of the merge (B) encodes: the row at `entry` given the id-sorted
    /// learned list, the performed mask (bits = trigger rows) and the
    /// AP-sorted Super records `(thr, trigger_row)`.
    fn merged_row(learned: &[u8], mask: u8, sup: &[(u8, u8)], entry: usize) -> Result<Row, ()> {
        let (mut i, mut k, mut r) = (0usize, 0usize, entry);
        loop {
            let sup_next = loop {
                if k >= sup.len() {
                    break None;
                }
                if mask >> sup[k].1 & 1 == 1 {
                    break Some(k);
                }
                k += 1;
            };
            let take_super = match sup_next {
                None => false,
                Some(k) => i >= learned.len() || learned[i] >= sup[k].0,
            };
            if take_super {
                if r == 0 {
                    return Ok(Row::Super(k));
                }
                r -= 1;
                k += 1;
            } else {
                if i >= learned.len() {
                    return Err(());
                }
                if r == 0 {
                    return Ok(Row::Learned(learned[i]));
                }
                r -= 1;
                i += 1;
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Row {
        Learned(u8),
        Super(usize),
    }

    #[test]
    fn merge_model_interleaves_by_threshold() {
        // Vahn: learned Miracle(0), Burning Flare(1), Cyclone(4), Somersault(12);
        // Supers sorted by AP: k0 thr=1 (66 AP: before every hyper),
        // k1 thr=1 (60), k2 thr=1 (54), k3 thr=1 (54), k4 thr=1 (54).
        let learned = [0u8, 1, 4, 12];
        let sup = [(1u8, 4u8), (1, 0), (1, 1), (1, 2), (1, 3)];
        // Only Tri-Somersault (trigger row 0 = sorted k1) performed.
        let mask = 1 << 0;
        let rows: Vec<Row> = (0..5)
            .map(|e| merged_row(&learned, mask, &sup, e).unwrap())
            .collect();
        assert_eq!(
            rows,
            vec![
                Row::Learned(0),
                Row::Super(1),
                Row::Learned(1),
                Row::Learned(4),
                Row::Learned(12)
            ]
        );
        // Everything performed: Miracle, then all five Supers, then the rest.
        let rows: Vec<Row> = (0..9)
            .map(|e| merged_row(&learned, 0x1F, &sup, e).unwrap())
            .collect();
        assert_eq!(rows[0], Row::Learned(0));
        assert_eq!(
            rows[1..6],
            [
                Row::Super(0),
                Row::Super(1),
                Row::Super(2),
                Row::Super(3),
                Row::Super(4)
            ]
        );
        assert_eq!(
            rows[6..],
            [Row::Learned(1), Row::Learned(4), Row::Learned(12)]
        );
        // Nothing learned, one performed: the Super is row 0.
        // (sorted k0 carries trigger row 4, so mask bit 4 lights k0.)
        assert_eq!(merged_row(&[], 0x10, &sup, 0), Ok(Row::Super(0)));
        // Nothing performed: pure learned list.
        assert_eq!(merged_row(&learned, 0, &sup, 2), Ok(Row::Learned(4)));
    }

    // --- (F) the fill routine ------------------------------------------------

    #[test]
    fn fill_routine_chases_the_name_and_writes_only_scratch_and_buffer() {
        let r = fill_routine();
        assert_eq!(r[2], lbu(T1, T3, 0), "chain AP");
        assert_eq!(r[3], lhu(T7, T3, 2), "name offset | markers");
        assert_eq!(r[4], lui(T9, hi(CMD_TABLE_VA)));
        assert_eq!(r[5], addu(T9, T9, T8), "t8 = character * 4 from (B)");
        assert_eq!(r[6], lw(T9, T9, lo(CMD_TABLE_VA)), "the command-data block");
        assert_eq!(r[8], lw(T9, T9, ART_BLOCK_PTR_OFF), "the art-record array");
        assert_eq!(
            r[10],
            srl(T6, T7, MARKERS_SHIFT),
            "marker count off the top bits"
        );
        assert_eq!(r[11], andi(T7, T7, 0x1FFF), "name offset off the low bits");
        assert_eq!(r[12], addu(T9, T9, T7), "+ name offset");
        // The arrows are addressed relative to the SUP record (same order and
        // stride), so a Noa/Gala row never reads Vahn's arrows.
        let delta = ARROWS_VA.wrapping_sub(SUP_VA);
        assert_eq!(r[15], lui(T2, hi(delta)));
        assert_eq!(r[16], addu(T3, T3, T2));
        assert_eq!(
            r[17],
            lw(T3, T3, lo(delta)),
            "the packed word from &SUP[k] + delta"
        );
        assert_eq!(
            r[18],
            lw(T9, T5, SCRATCH_ARROWS_OFF),
            "the buffer is the scratch's own +8"
        );
        assert_eq!(r[19], srl(T4, T3, ARROWS_COUNT_SHIFT), "n off the top bits");
        assert_eq!(r[20], addu(T6, T6, T4), "glyph count = markers + n");
        assert_eq!(r[24], ori(T6, ZERO, 1), "rows start in the default style");
        // Style selection: default, art-end on an end bit, Super-end on the last.
        assert_eq!(r[26], andi(T7, T3, 4), "the end bit of the current arrow");
        assert_eq!(r[28], ori(T1, ZERO, 1));
        assert_eq!(r[30], ori(T1, ZERO, 6));
        assert_eq!(r[31], ori(T1, ZERO, 9), "Miracle orange on the final arrow");
        assert_eq!(27 + 1 + br_off(r[27]), 32, "not an end -> CMP");
        assert_eq!(29 + 1 + br_off(r[29]), 32, "an end, not last -> CMP");
        assert_eq!(32 + 1 + br_off(r[32]), 38, "same style -> GLYPH");
        assert_eq!(r[43], srl(T3, T3, 3), "three bits per arrow");
        assert_eq!(45 + 1 + br_off(r[45]), 25, "the glyph loop closes");
        assert_eq!(r[47], j(SCAN_HIT_VA), "then the hit arm");
        assert_eq!(
            r[48],
            addiu(S5, T5, SCRATCH_ARROWS_OFF),
            "s5 = scratch + 8 in the delay slot"
        );
        assert_eq!(r.len(), 49);
        let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
        assert_eq!(stores[0], sb(T1, T5, SCRATCH_AP_OFF));
        assert_eq!(stores[1], sw(T9, T5, SCRATCH_NAME_OFF));
        assert_eq!(stores[2], sb(T6, T9, 0), "glyph count");
        assert_eq!(stores[3..5], [sb(T2, T9, 0), sb(T1, T9, 1)], "a marker");
        assert_eq!(
            stores[5..],
            [sb(T2, T9, 0), sb(T1, T9, 1)],
            "an arrow glyph"
        );
        assert_eq!(stores.len(), 7);
        // Load delays: lbu t1 (2) -> 13; lhu t7 (3) -> 10; lw t9 (6) -> 8;
        // lw t9 (8) -> 12; lw t3 (17) -> 19; lw t9 (18) -> 21.
        for (load, first_use) in [
            (2usize, 13usize),
            (3, 10),
            (6, 8),
            (8, 12),
            (17, 19),
            (18, 21),
        ] {
            let dest = (r[load] >> 16) & 0x1f;
            let filler = r[load + 1];
            let (rs, rt) = ((filler >> 21) & 0x1f, (filler >> 16) & 0x1f);
            assert!(
                rs != dest && rt != dest,
                "delay slot after the load at {load} reads it"
            );
            assert!(first_use >= load + 2);
        }
    }

    #[test]
    fn packed_arrows_round_trip_through_the_expander_model() {
        // The expander: per arrow k, dir = word >> 3k & 3, end = word >> 3k & 4,
        // n = word >> 27; a marker whenever the style changes.
        let row = SuperArtRow {
            character: Character::Vahn,
            name: "Tri-Somersault",
            finisher: 0x2B,
            trigger_row: 0,
            sorted_index: 1,
            chain_ids: vec![12, 4, 12],
            chain_names: vec![],
            ap: 60,
            thr: 1,
            name_offset: 0x1614,
            input: vec![
                Command::Up,
                Command::Down,
                Command::Up,
                Command::Up,
                Command::Up,
                Command::Down,
                Command::Up,
            ],
            ends: vec![2, 4, 6],
        };
        let w = u32::from_le_bytes(row.packed_arrows());
        assert_eq!(w >> ARROWS_COUNT_SHIFT, 7);
        let mut dirs = Vec::new();
        let mut ends = Vec::new();
        for k in 0..7 {
            let f = (w >> (3 * k)) & 7;
            dirs.push(GLYPH_LO_BASE + (f & 3) as u8);
            if f & 4 != 0 {
                ends.push(k);
            }
        }
        // Retail's glyph codes: 0x81A8 Right / A9 Left / AA Up / AB Down.
        let want: Vec<u8> = "UDUUUDU"
            .chars()
            .map(|c| match c {
                'U' => 0xAA,
                'D' => 0xAB,
                'L' => 0xA9,
                _ => 0xA8,
            })
            .collect();
        assert_eq!(dirs, want);
        assert_eq!(ends, vec![2, 4, 6]);
        // blue blue yellow blue yellow blue orange: five style changes.
        assert_eq!(row.styles(), vec![1, 1, 6, 1, 6, 1, 9]);
        assert_eq!(row.marker_count(), 5);
        assert_eq!(
            row.glyph_string(),
            vec![
                12, 0x81, 0xAA, 0x81, 0xAB, 0xFF, 6, 0x81, 0xAA, 0xFF, 1, 0x81, 0xAA, 0xFF, 6,
                0x81, 0xAA, 0xFF, 1, 0x81, 0xAB, 0xFF, 9, 0x81, 0xAA
            ]
        );
        assert_eq!(
            row.sup_record(),
            [60, 1, 0x14, 0x16 | (5 << 5)],
            "markers ride the name offset's top bits"
        );
        assert_eq!(row.input_letters(), "UDUUUDU");
    }

    #[test]
    fn arrow_codes_are_the_glyph_low_byte_offsets() {
        for (c, lo_) in [
            (Command::Right, 0xA8u8),
            (Command::Left, 0xA9),
            (Command::Up, 0xAA),
            (Command::Down, 0xAB),
        ] {
            assert_eq!(GLYPH_LO_BASE + arrow_code(c), lo_);
            let [hi_, lo2] = legaia_art::arts_table::command_to_glyph(c);
            assert_eq!(
                (hi_, lo2),
                (GLYPH_HI, lo_),
                "matches the arts-table decoder"
            );
        }
    }

    // --- (W) the performed hook -----------------------------------------------

    #[test]
    fn performed_routine_sets_one_bit_and_counts_it_once() {
        let r = assemble_performed(PERFORMED_RET_VA);
        assert_eq!(r[0], HOOK_PERFORMED_W0, "replays lui v1,0x801f");
        assert_eq!(r[1], HOOK_PERFORMED_W1, "replays addiu v0,zero,1");
        assert_eq!(
            r[2],
            srl(T3, T2, 4),
            "row = t2 >> 4, NOT a1 (the copy loop clobbers a1)"
        );
        assert_eq!(r[4], sllv(T3, T4, T3), "bit = 1 << row");
        assert_eq!(r[13], lbu(T2, T0, PERFORMED_OFF));
        assert_eq!(r[15], and(T4, T2, T3), "already performed?");
        assert_eq!(16 + 1 + br_off(r[16]), 20, "then straight to the return");
        assert_eq!(r[18], addiu(T2, T2, 0x20), "count field += 1");
        assert_eq!(r[19], sb(T2, T0, PERFORMED_OFF), "the only store");
        assert_eq!(r[20], j(PERFORMED_RET_VA), "back to the flag store");
        assert_eq!(r.len(), 22);
        let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
        assert_eq!(stores.len(), 1);
        // Clobbers only t0-t4 besides the two replayed retail writes, and reads
        // t2 before it overwrites it.
        for (i, w) in r.iter().enumerate().skip(2) {
            if let Some(d) = dest_reg(*w) {
                assert!((T0..=T4).contains(&d), "instruction {i} writes r{d}");
            }
        }
        // Model: bit set once, count bumps once.
        let apply = |byte: u8, row: u8| -> u8 {
            let bit = 1u8 << row;
            if byte & bit != 0 {
                byte
            } else {
                (byte | bit) + 0x20
            }
        };
        let b = apply(0, 0);
        assert_eq!(b, 0x21);
        assert_eq!(apply(b, 0), 0x21, "performing it again changes nothing");
        assert_eq!(apply(b, 4), 0x51);
        assert_eq!(apply(0x51, 1) >> 5, 3);
    }

    // --- (D) the pager -------------------------------------------------------

    #[test]
    fn pager_fits_the_original_body_with_the_performed_routine_and_its_branches_land() {
        let r = assemble_pager(PAGER_VA);
        let w = assemble_performed(PERFORMED_RET_VA);
        assert!(
            r.len() + w.len() <= PAGER_WORDS,
            "{} + {} words",
            r.len(),
            w.len()
        );
        assert_eq!(r[0], addiu(SP, SP, 0xFFE8), "same frame as retail");
        assert_eq!(r[r.len() - 2], jr(RA));
        assert_eq!(r[r.len() - 1], addiu(SP, SP, 0x18));
        assert_eq!(
            r[17],
            lbu(V1, V0, PERFORMED_OFF),
            "reads the performed byte"
        );
        assert_eq!(
            r[18],
            lbu(A0, V0, LEARNED_COUNT_OFF),
            "and the learned count"
        );
        assert_eq!(r[19], srl(V1, V1, 5));
        assert_eq!(r[20], addu(A0, A0, V1), "rows = learned + performed");
        let exit = r.len() - 4;
        let toggle = 38usize;
        assert_eq!(r[toggle], lui(V0, 0x801F), "toggle arm starts at 38");
        for (i, want) in [
            (7usize, exit),
            (21, exit),
            (44, exit),
            (26, toggle),
            (31, toggle),
            (34, toggle),
        ] {
            let target = i as i32 + 1 + br_off(r[i]);
            assert_eq!(target as usize, want, "branch at {i}");
        }
        assert_eq!(r[36], j(PAGER_VA + exit as u32 * 4), "j lands on EXIT");
        assert_eq!(r[50], jal(PAGER_SOUND_FN_VA), "keeps the open cue");
    }

    #[test]
    fn pager_steps_pages_while_another_page_exists() {
        let step = |offset: u16, rows: u32| -> Option<u16> {
            if offset < MAX_SCROLL && u32::from(offset) + PAGE_ROWS < rows {
                Some(offset + PAGE_ROWS as u16)
            } else {
                None
            }
        };
        assert_eq!(step(0, 21), Some(5));
        assert_eq!(step(15, 21), Some(20));
        assert_eq!(step(20, 21), None, "closes after the last page");
        assert_eq!(step(0, 2), None);
        assert_eq!(step(0, 6), Some(5));
        assert_eq!(step(5, 6), None);
        assert_eq!(step(0, 0), None);
    }

    // --- Table derivation ----------------------------------------------------

    #[test]
    fn chain_conversion_is_the_documented_constant_offset() {
        for s in legaia_art::SUPER_ARTS {
            for c in s.art_sequence() {
                assert!(
                    c >= ART_CONSTANT_BIAS,
                    "{}: chain constant {c:#x} is below the base",
                    s.name
                );
            }
            assert!(
                s.art_sequence().len() >= 2,
                "{} would trigger on a single art",
                s.name
            );
            assert!(
                s.finisher >= GRID_BIAS,
                "{}: finisher below the grid",
                s.name
            );
        }
        assert_eq!(legaia_art::SUPER_ARTS.len(), 15);
    }

    #[test]
    fn performed_byte_layout_holds_five_bits_and_a_count() {
        assert_eq!(PERFORMED_MASK, (1 << SUPER_ARTS_PER_CHAR) - 1);
        assert_eq!(PERFORMED_COUNT_SHIFT, SUPER_ARTS_PER_CHAR as u32);
        const { assert!(SUPER_ARTS_PER_CHAR < 1 << (8 - PERFORMED_COUNT_SHIFT)) };
        assert_eq!(
            PERFORMED_OFF,
            LEARNED_LIST_OFF + 15,
            "the sixteenth id slot"
        );
        // The packing carries the longest input plus its count in one word.
        const { assert!(MAX_INPUT_ARROWS as u32 * ARROW_BITS + 4 <= 32) };
        const { assert!(MAX_INPUT_ARROWS < 1 << (32 - ARROWS_COUNT_SHIFT)) };
        const { assert!(MAX_MARKERS < 1 << (16 - MARKERS_SHIFT)) };
        assert_eq!(GLYPH_BUF_BYTES, 29, "nine arrows + five markers");
    }

    /// The glyph string (F) builds for an input with the given art ends: the
    /// same model `SuperArtRow::glyph_string` encodes.
    fn expected_glyph_string(input: &[Command], ends: &[usize]) -> Vec<u8> {
        let row = SuperArtRow {
            character: Character::Vahn,
            name: "",
            finisher: 0x2B,
            trigger_row: 0,
            sorted_index: 0,
            chain_ids: vec![],
            chain_names: vec![],
            ap: 0,
            thr: 0,
            name_offset: 0,
            input: input.to_vec(),
            ends: ends.to_vec(),
        };
        row.glyph_string()
    }

    // --- Execution ------------------------------------------------------------
    //
    // A tiny R3000 subset - exactly the instructions these routines use, delay
    // slots included - so the assembled words run against the model instead of
    // being read. Memory is a sparse byte map; anything unmapped reads as zero.

    use std::collections::HashMap;

    struct Cpu {
        r: [u32; 32],
        pc: u32,
        mem: HashMap<u32, u8>,
        steps: usize,
    }

    impl Cpu {
        fn new() -> Self {
            Self {
                r: [0; 32],
                pc: 0,
                mem: HashMap::new(),
                steps: 0,
            }
        }
        fn load(&mut self, va: u32, bytes: &[u8]) {
            for (i, b) in bytes.iter().enumerate() {
                self.mem.insert(va + i as u32, *b);
            }
        }
        fn load_words(&mut self, va: u32, w: &[u32]) {
            self.load(va, &words_to_bytes(w));
        }
        fn rd8(&self, a: u32) -> u8 {
            *self.mem.get(&a).unwrap_or(&0)
        }
        fn rd16(&self, a: u32) -> u16 {
            u16::from_le_bytes([self.rd8(a), self.rd8(a + 1)])
        }
        fn rd32(&self, a: u32) -> u32 {
            u32::from_le_bytes([
                self.rd8(a),
                self.rd8(a + 1),
                self.rd8(a + 2),
                self.rd8(a + 3),
            ])
        }
        fn wr8(&mut self, a: u32, v: u8) {
            self.mem.insert(a, v);
        }
        fn wr32(&mut self, a: u32, v: u32) {
            self.load(a, &v.to_le_bytes());
        }
        /// Execute one instruction (with its delay slot for control flow).
        fn exec(&mut self, w: u32) {
            let op = w >> 26;
            let rs = ((w >> 21) & 31) as usize;
            let rt = ((w >> 16) & 31) as usize;
            let rd = ((w >> 11) & 31) as usize;
            let sa = (w >> 6) & 31;
            let imm = (w & 0xffff) as u16;
            let simm = imm as i16 as i32 as u32;
            let mut next = self.pc + 4;
            let mut branch: Option<u32> = None;
            match op {
                0 => match w & 0x3f {
                    0x00 => self.r[rd] = self.r[rt] << sa,
                    0x02 => self.r[rd] = self.r[rt] >> sa,
                    0x03 => self.r[rd] = ((self.r[rt] as i32) >> sa) as u32,
                    0x04 => self.r[rd] = self.r[rt] << (self.r[rs] & 31),
                    0x06 => self.r[rd] = self.r[rt] >> (self.r[rs] & 31),
                    0x08 => branch = Some(self.r[rs]),
                    0x21 => self.r[rd] = self.r[rs].wrapping_add(self.r[rt]),
                    0x23 => self.r[rd] = self.r[rs].wrapping_sub(self.r[rt]),
                    0x24 => self.r[rd] = self.r[rs] & self.r[rt],
                    0x25 => self.r[rd] = self.r[rs] | self.r[rt],
                    0x2b => self.r[rd] = u32::from(self.r[rs] < self.r[rt]),
                    f => panic!("unsupported SPECIAL funct {f:#x} at {:#x}", self.pc),
                },
                0x02 => branch = Some((self.pc & 0xF000_0000) | ((w & 0x03ff_ffff) << 2)),
                0x03 => {
                    self.r[31] = self.pc + 8;
                    branch = Some((self.pc & 0xF000_0000) | ((w & 0x03ff_ffff) << 2));
                }
                0x04 => {
                    if self.r[rs] == self.r[rt] {
                        branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                    }
                }
                0x05 => {
                    if self.r[rs] != self.r[rt] {
                        branch = Some(self.pc.wrapping_add(4).wrapping_add(simm << 2));
                    }
                }
                0x09 => self.r[rt] = self.r[rs].wrapping_add(simm),
                0x0b => self.r[rt] = u32::from(self.r[rs] < simm),
                0x0c => self.r[rt] = self.r[rs] & u32::from(imm),
                0x0d => self.r[rt] = self.r[rs] | u32::from(imm),
                0x0f => self.r[rt] = u32::from(imm) << 16,
                0x21 => self.r[rt] = self.rd16(self.r[rs].wrapping_add(simm)) as i16 as i32 as u32,
                0x23 => self.r[rt] = self.rd32(self.r[rs].wrapping_add(simm)),
                0x24 => self.r[rt] = u32::from(self.rd8(self.r[rs].wrapping_add(simm))),
                0x25 => self.r[rt] = u32::from(self.rd16(self.r[rs].wrapping_add(simm))),
                0x28 => self.wr8(self.r[rs].wrapping_add(simm), self.r[rt] as u8),
                0x2b => self.wr32(self.r[rs].wrapping_add(simm), self.r[rt]),
                o => panic!("unsupported opcode {o:#x} at {:#x}", self.pc),
            }
            self.r[0] = 0;
            if let Some(target) = branch {
                // Delay slot, then the jump. Load delays are not modelled - the
                // routines are asserted delay-safe by the static tests above.
                let slot = self.rd32(self.pc + 4);
                self.pc += 4;
                self.exec_plain(slot);
                next = target;
            }
            self.pc = next;
            self.steps += 1;
        }
        /// A delay-slot instruction (never itself a branch in these routines).
        fn exec_plain(&mut self, w: u32) {
            let saved = self.pc;
            self.exec(w);
            assert_eq!(self.pc, saved + 4, "branch in a delay slot");
        }
        /// Run until the PC reaches one of `stops`, or give up.
        fn run_until(&mut self, stops: &[u32]) -> u32 {
            while !stops.contains(&self.pc) {
                assert!(self.steps < 10_000, "runaway at {:#x}", self.pc);
                let w = self.rd32(self.pc);
                self.exec(w);
            }
            self.pc
        }
    }

    /// One simulated character: which arts are learned, which Supers performed
    /// (bits = trigger rows), and the AP-sorted SUP records `(ap, thr, row)`.
    struct Scene {
        character: u32,
        learned: Vec<u8>,
        performed: u8,
        sup: Vec<(u8, u8, u8)>,
        arrows: Vec<Vec<Command>>,
        ends: Vec<Vec<usize>>,
    }

    fn cmd_va() -> u32 {
        0x8010_0000
    }
    fn art_array_va() -> u32 {
        0x8010_1000
    }

    /// Lay the scene out in a fresh CPU with every routine loaded at its test VA.
    fn cpu_for(scene: &Scene, in_battle: bool) -> Cpu {
        let mut cpu = Cpu::new();
        cpu.load_words(ID_VA, &id_routine());
        cpu.load_words(FILL_VA, &fill_routine());
        cpu.load_words(
            COUNT_VA,
            &assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], COUNT_RET_VA),
        );
        // The character record.
        let rec = CHAR_RECORD_BASE + scene.character * 0x414;
        cpu.wr8(
            rec + u32::from(LEARNED_COUNT_OFF),
            scene.learned.len() as u8,
        );
        for (i, id) in scene.learned.iter().enumerate() {
            cpu.wr8(rec + u32::from(LEARNED_LIST_OFF) + i as u32, *id);
        }
        let count = scene.performed.count_ones() as u8;
        cpu.wr8(
            rec + u32::from(PERFORMED_OFF),
            scene.performed | (count << PERFORMED_COUNT_SHIFT),
        );
        // Globals: game mode, character, command table -> art array.
        cpu.load(
            GAME_MODE_VA,
            &(if in_battle { BATTLE_MODE } else { 0x0C }).to_le_bytes(),
        );
        cpu.r[GP as usize] = 0x8007_0000;
        cpu.wr32(0x8007_0000 + u32::from(GP_CHARACTER), scene.character);
        cpu.wr32(CMD_TABLE_VA + scene.character * 4, cmd_va());
        cpu.wr32(cmd_va() + u32::from(ART_BLOCK_PTR_OFF), art_array_va());
        // The SUP records for every character (only this one's matter) and the
        // packed arrows, in AP-sorted order.
        for (k, &(ap, thr, row)) in scene.sup.iter().enumerate() {
            let name_offset = ART_BLOCK_BIAS
                + u32::from(0x2B + row - GRID_BIAS) * ART_RECORD_STRIDE
                + ART_NAME_FIELD_OFF;
            let at = SUP_VA + (scene.character * 5 + k as u32) * SUP_STRIDE;
            let arrows = &scene.arrows[k];
            let markers =
                expected_glyph_string(arrows, &scene.ends[k])[0] as u16 - arrows.len() as u16;
            let [lo_, hi_] = ((name_offset as u16) | (markers << MARKERS_SHIFT)).to_le_bytes();
            cpu.load(at, &[ap, thr | (row << PERFORMED_COUNT_SHIFT), lo_, hi_]);
            let mut w: u32 = (arrows.len() as u32) << ARROWS_COUNT_SHIFT;
            for (n, &c) in arrows.iter().enumerate() {
                let end = u32::from(scene.ends[k].contains(&n));
                w |= (u32::from(arrow_code(c)) | (end << 2)) << (n as u32 * ARROW_BITS);
            }
            cpu.load(
                ARROWS_VA + (scene.character * 5 + k as u32) * ARROWS_STRIDE,
                &w.to_le_bytes(),
            );
        }
        // Scratch record: +8 -> the glyph buffer.
        cpu.wr32(SCRATCH_VA + u32::from(SCRATCH_ARROWS_OFF), BUF_VA);
        cpu
    }

    /// Run hook (B) for `entry`; returns what the row resolved to.
    fn run_row(scene: &Scene, entry: u32, in_battle: bool) -> Row {
        let mut cpu = cpu_for(scene, in_battle);
        let rec = CHAR_RECORD_BASE + scene.character * 0x414;
        cpu.r[V0 as usize] = rec + entry;
        cpu.r[A2 as usize] = entry;
        cpu.r[A0 as usize] = 0x8007_0000;
        cpu.r[V1 as usize] = 0; // rec[+0] of the table's first record (Vahn)
        cpu.pc = ID_VA;
        match cpu.run_until(&[ID_RET_VA, SCAN_HIT_VA]) {
            pc if pc == ID_RET_VA => {
                assert_eq!(
                    cpu.r[V1 as usize], 1,
                    "the replayed sltiu v1,v1,0x63 saw v1 = 0"
                );
                assert_eq!(cpu.r[A0 as usize], 0x8007_0000, "a0 preserved");
                Row::Learned(cpu.r[S2 as usize] as u8)
            }
            _ => {
                assert_eq!(cpu.r[S5 as usize], SCRATCH_VA + 8, "s5 = scratch + 8");
                // Which Super did the fill routine draw? Recover it from what it
                // wrote and check every field against the scene.
                let ap = cpu.rd8(SCRATCH_VA + u32::from(SCRATCH_AP_OFF));
                let name = cpu.rd32(SCRATCH_VA + u32::from(SCRATCH_NAME_OFF));
                let k = scene
                    .sup
                    .iter()
                    .position(|&(a, _, row)| {
                        a == ap
                            && name
                                == art_array_va()
                                    + ART_BLOCK_BIAS
                                    + u32::from(0x2B + row - GRID_BIAS) * ART_RECORD_STRIDE
                                    + ART_NAME_FIELD_OFF
                    })
                    .expect("scratch AP + name pair names one SUP record");
                let want = expected_glyph_string(&scene.arrows[k], &scene.ends[k]);
                let got: Vec<u8> = (0..want.len() as u32)
                    .map(|i| cpu.rd8(BUF_VA + i))
                    .collect();
                assert_eq!(
                    got, want,
                    "glyph string for Super {k} of character {}",
                    scene.character
                );
                Row::Super(k)
            }
        }
    }

    fn vahn_scene(learned: &[u8], performed: u8) -> Scene {
        scene_for(0, learned, performed)
    }

    /// The same five Supers under any character index (arrows differ per
    /// character so cross-character addressing is exercised, not just Vahn's).
    fn scene_for(character: u32, learned: &[u8], performed: u8) -> Scene {
        // AP-sorted: Rolling Combo (66, row 4), Tri-Somersault (60, row 0),
        // Maximum Blow / Fire Tackle / Power Slash (54, rows 1..3). thr = 1 for
        // all (Vahn's first sub-66 art is Burning Flare, id 1, 50 AP).
        use Command::*;
        let mut sc = Scene {
            character,
            learned: learned.to_vec(),
            performed,
            sup: vec![(66, 1, 4), (60, 1, 0), (54, 1, 1), (54, 1, 2), (54, 1, 3)],
            arrows: vec![
                vec![Up, Down, Right, Left, Left, Down, Up, Up, Left],
                vec![Up, Down, Up, Up, Up, Down, Up],
                vec![Down, Right, Up, Down, Left, Left, Down],
                vec![Left, Right, Left, Left, Down, Right, Up],
                vec![Down, Right, Up, Down, Up, Down, Left],
            ],
            // Where each chain's arts end (`legaia_art::art_ends`): Rolling
            // Combo 3/5/8, the three-art seven-arrow chains 2/4/6.
            ends: vec![
                vec![3, 5, 8],
                vec![2, 4, 6],
                vec![2, 4, 6],
                vec![2, 4, 6],
                vec![2, 4, 6],
            ],
        };
        // Rotate the arrows per character so a wrong table base is visible.
        for _ in 0..character {
            sc.arrows.rotate_left(1);
            sc.ends.rotate_left(1);
        }
        sc
    }

    #[test]
    fn executed_id_routine_agrees_with_the_merge_model_everywhere() {
        // Every subset of a five-art learned list x every performed mask x every
        // row: the assembled routine and the Rust model must resolve identically.
        let pool = [0u8, 1, 4, 12, 14];
        for lm in 0u8..32 {
            let learned: Vec<u8> = pool
                .iter()
                .enumerate()
                .filter(|(i, _)| lm >> i & 1 == 1)
                .map(|(_, &id)| id)
                .collect();
            for performed in 0u8..32 {
                let scene = vahn_scene(&learned, performed);
                let sup: Vec<(u8, u8)> =
                    scene.sup.iter().map(|&(_, thr, row)| (thr, row)).collect();
                let total = learned.len() + performed.count_ones() as usize;
                for entry in 0..total {
                    let want = merged_row(&learned, performed, &sup, entry).expect("model");
                    let got = run_row(&scene, entry as u32, true);
                    assert_eq!(
                        got, want,
                        "learned {learned:?} performed {performed:#07b} entry {entry}"
                    );
                }
            }
        }
    }

    #[test]
    fn executed_id_routine_is_plain_outside_battle_and_with_nothing_performed() {
        let learned = [0u8, 4, 12];
        for entry in 0..3u32 {
            let scene = vahn_scene(&learned, 0x1F);
            assert_eq!(
                run_row(&scene, entry, false),
                Row::Learned(learned[entry as usize]),
                "field / menu"
            );
            let scene = vahn_scene(&learned, 0);
            assert_eq!(
                run_row(&scene, entry, true),
                Row::Learned(learned[entry as usize]),
                "none performed"
            );
        }
    }

    #[test]
    fn executed_count_routine_adds_the_performed_count_only_in_battle() {
        for (performed, in_battle, want) in [
            (0x1Fu8, true, 3 + 5u32),
            (0x05, true, 3 + 2),
            (0x1F, false, 3),
        ] {
            let scene = vahn_scene(&[0, 4, 12], performed);
            let mut cpu = cpu_for(&scene, in_battle);
            let rec = CHAR_RECORD_BASE;
            cpu.r[V0 as usize] = rec;
            cpu.r[A2 as usize] = 0xFFFF_FFF0; // sra a2,a2,0x1f replays on it
            cpu.pc = COUNT_VA;
            // The stock word inside the routine reads the count out of the record.
            cpu.run_until(&[COUNT_RET_VA]);
            assert_eq!(
                cpu.r[V0 as usize], want,
                "performed {performed:#x} battle {in_battle}"
            );
            assert_eq!(cpu.r[A2 as usize], 0xFFFF_FFFF, "sra a2,a2,0x1f replayed");
        }
    }

    #[test]
    fn executed_performed_routine_records_each_super_once() {
        let mut cpu = Cpu::new();
        let w = assemble_performed(PERFORMED_RET_VA);
        const W_VA: u32 = 0x801D_3828;
        cpu.load_words(W_VA, &w);
        let rec = CHAR_RECORD_BASE + 2 * 0x414; // Gala
        let byte = |cpu: &Cpu| cpu.rd8(rec + u32::from(PERFORMED_OFF));
        for (row, want) in [
            (3u32, 0x28u8),
            (3, 0x28),
            (0, 0x49),
            (4, 0x79),
            (1, 0x9B),
            (2, 0xBF),
        ] {
            cpu.r[T5 as usize] = 2;
            cpu.r[T2 as usize] = row * 16; // sll t2,a1,0x4 at 0x801EFB04
            cpu.r[A1 as usize] = 0x2B; // what the copy loop leaves in a1: the finisher
            cpu.pc = W_VA;
            cpu.run_until(&[PERFORMED_RET_VA]);
            assert_eq!(byte(&cpu), want, "after performing row {row}");
            assert_eq!(
                cpu.r[V1 as usize], 0x801F_0000,
                "v1 as retail's lui left it"
            );
            assert_eq!(cpu.r[V0 as usize], 1, "v0 as retail's li left it");
        }
        // Another character's byte is untouched.
        assert_eq!(cpu.rd8(CHAR_RECORD_BASE + u32::from(PERFORMED_OFF)), 0);
    }

    // --- Placement -----------------------------------------------------------

    #[test]
    fn every_routine_and_table_fits_its_region() {
        let count = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], 0).len() * 4;
        let id = assemble_id([HOOK_ID_W0, HOOK_ID_W1], 0, 0, 0, 0).len() * 4;
        let fill = assemble_fill(0, 0, 0, 0).len() * 4;
        let n = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
        let gap = id + SCRATCH_BYTES + GLYPH_BUF_BYTES;
        assert!(
            gap <= (SCUS_GAP_END_VA - SCUS_GAP_VA) as usize,
            "{gap} B in the gap"
        );
        let a1 = fill + n * ARROWS_STRIDE as usize;
        assert!(
            a1 <= (ARENA1_END_VA - ARENA1_VA) as usize,
            "{a1} B in arena 1"
        );
        assert!(
            count <= (ARENA2_END_VA - ARENA2_VA) as usize,
            "{count} B in arena 2"
        );
        assert!(n * SUP_STRIDE as usize <= (SLOT6_END_VA - SLOT6_VA) as usize);
        eprintln!(
            "count {count} + id {id} + fill {fill} = {} B of code, {} B of tables",
            count + id + fill,
            n * (SUP_STRIDE + ARROWS_STRIDE) as usize + SCRATCH_BYTES + GLYPH_BUF_BYTES
        );
    }

    #[test]
    fn scratch_cursor_lines_up_with_the_fields_retail_reads() {
        // Retail's hit arm reads rec[+0xC] as `lw a0,0x4(s5)`, rec[+2] as
        // `lbu s0,-0x6(s5)` and rec[+8] as `lw v1,0x0(s5)` - all off s5 = rec+8.
        let s5 = SCRATCH_ARROWS_OFF as i32;
        assert_eq!(s5 + 4, SCRATCH_NAME_OFF as i32, "name at s5+4");
        assert_eq!(s5 - 6, SCRATCH_AP_OFF as i32, "AP at s5-6");
        assert!(SCRATCH_NAME_OFF as usize + 4 <= SCRATCH_BYTES);
    }
}
