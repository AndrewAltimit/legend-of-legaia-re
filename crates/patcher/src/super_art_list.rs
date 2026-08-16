//! **Show Super Arts on the in-battle move list**: add a character's Super Arts
//! to the Tactical-Arts list the Triangle button opens in battle - which retail
//! never draws at all - at the **head** of the list, each row carrying its
//! **name** and its **AP cost**, and only once the chain that triggers it is
//! learned.
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
//! 80034460  addiu s5,a1,0x8      ; (E)  a1 = table base, s5 = record + 8
//! 80034464  lbu   v1,0x0(a1)     ;   require rec[+0] == character   <- scan head
//! 80034480  bne   v0,s2,...      ;   require rec[+1] == id
//! 80034488  clear a1             ;   HIT: draw the row                <- scan hit
//! 80034498  lw    a0,0x4(s5)     ;     rec[+0xC] -> name  -> FUN_80036888
//! 800344d8  lbu   s0,-0x6(s5)    ;     rec[+2]   -> AP    -> digit sprites
//! 800345e8  lw    v1,0x0(s5)     ;     rec[+8]   -> arrow glyph string
//! 8003474c  addiu s7,s7,0x1      ;   MISS: the row is silently consumed
//! ```
//!
//! `v0` at (A) is `0x80084140 + character*0x414`; the character index itself is
//! `gp+0x874` (`0` Vahn / `1` Noa / `2` Gala / `3` Terra), the same 0-based space
//! the arts-name table's `rec[+0]` byte uses. Rows per page is the drawable
//! height over the row pitch, `0x90 / 0x1C` = **5**.
//!
//! ## The shape of the injection
//!
//! A Super Art has no row in `DAT_80075EC4` (45 records, fifteen regular arts
//! per character) and no learned-art id. So the feature **synthesises a record**:
//! a 16-byte scratch record in dead SCUS space whose `+2` AP byte and `+0xC`
//! name pointer are filled per row, and whose `+8` arrow pointer is a fixed
//! pointer to a zero byte. Hook (E) then jumps the routine **past the scan**
//! straight to the hit path with `s5` pointing at that scratch record, so every
//! field is drawn by retail's own code - no re-implemented renderer, and the
//! synthetic row cannot be confused with a real one.
//!
//! | Site | VA | Stock word | Role |
//! |---|---|---|---|
//! | A count | `0x800343C4` | `0x9042074D` (`lbu v0,0x74d(v0)`) | `count + unlocked` |
//! | B id | `0x80034450` | `0x9052074E` (`lbu s2,0x74e(v0)`) | head rows get a synthetic id; learned rows shift down |
//! | E record | `0x80034460` | `0x24B50008` (`addiu s5,a1,0x8`) | point `s5` at the scratch record and skip the scan |
//! | D pager | `0x801D3748` (PROT 0898) | `0x27BDFFE8` | page while another page exists |
//!
//! A shared leaf routine [`assemble_sub`] answers the one question all four ask:
//! **which** Super Arts this character may see, as a five-bit mask plus its
//! population count. (A) and (D) call it; (B) reads the two bytes it caches.
//!
//! ## What "unlocked" means here
//!
//! Retail has nowhere to record that a Super Art was *performed*: the id list at
//! `+0x74E..+0x75D` holds regular-art ids only, and the Super applier
//! `FUN_801EF9E4` contains no call at all - it is a find/replace over the
//! finished action queue and marks nothing. A true "performed at least once"
//! flag would need a new persistent per-character bit plus a writer inside the
//! battle overlay.
//!
//! So the gate is the **availability** one instead: a Super Art is listed once
//! **every art in its trigger chain is in the character's learned-art list**.
//! That is exact for whether you can perform it - the trigger is a find/replace
//! over a queue you can only build out of arts you know - so a row appears on
//! the same battle the move itself becomes possible, and the list is empty until
//! then. It is *not* a "you have done this" flag, and the CLI, the page and the
//! docs all say so.
//!
//! Each Super Art's chain is [`legaia_art::SuperArt::art_sequence`] - its `find`
//! pattern with the `0x19` starters and connector directions stripped. Chain
//! entries are **action constants**; the learned list stores **display ids**, and
//! the two differ by a constant: display row `n` is action constant
//! `0x1B + n` ([`ART_CONSTANT_BIAS`], the same relation
//! [`crate::super_art_power::art_block_base`] already solves the art block with).
//! [`super_art_rows`] converts every chain entry back and **refuses** if any one
//! of them fails to land on a real row of the disc's own arts-name table.
//!
//! ## Where the row's AP and name come from
//!
//! - **AP** is the chain's: the sum of the chain arts' `rec[+2]` AP costs, read
//!   off the disc's arts-name table at patch time into a 15-byte table. A Super
//!   Art has no AP of its own - the chain arts pay it - so the chain's total is
//!   the truthful number, and it is what the row shows. Retail's own halving for
//!   the `+0x6C0 & 0x800` flag still applies on top, because the value is drawn
//!   by retail's own digit loop.
//! - **Name** is chased in RAM. Retail resolves an art record at
//!   `*(*(DAT_801C9360[character]) + 0x58) + 4 + (constant - 0x10) * 0xD0`, and
//!   the display name is that record's `+0x10` field. All three constants are
//!   measured from retail's own indexing in `FUN_8004AD80`
//!   (`0x8004B6FC..0x8004B718` builds the base; `0x8004BBE8..0x8004BC10` reads
//!   `... - 0xCF0` = the `+0x10` **name**, `0x8004BC60..0x8004BC80` reads
//!   `... - 0xCDC` = the `+0x24` **power**), and the same offsets are what
//!   [`crate::super_art_power`] edits through. Only the finisher constant needs
//!   carrying (15 bytes), so no name blob is embedded at all.
//!
//! ## What a row does not show
//!
//! **No arrows.** A regular row draws `rec[+8]`, a `[count][2-byte glyph]*`
//! string, and a Super Art's truthful string is its chain arts' glyph strings
//! concatenated with the per-combo connector directions between them. Carrying
//! the fifteen concatenations costs roughly 330 bytes and building them at
//! runtime costs a copy loop plus an ordered chain table, and the whole feature
//! has 652 bytes of verified-dead SCUS to live in. The scratch record's `+8`
//! therefore points at a zero byte: the glyph count is `0`, retail's own
//! `beq v0,zero,0x8003474c` at `0x80034650` takes the row-consumed exit, and the
//! row draws its name and AP with a blank command line.
//!
//! ## Battle-only by construction
//!
//! `FUN_80034358` is reached only through the shared window-content dispatcher
//! `FUN_80031D00`, which also runs from the field and menu overlays. The name
//! chase dereferences `DAT_801C9360`, which is meaningful only while the battle
//! overlay is resident, so [`assemble_sub`] gates the whole feature on the
//! master game-mode selector `_DAT_8007B83C == 0x15` ([`BATTLE_MODE`], the same
//! word `FUN_80031D00` itself compares against `0x15` at its entry). Outside
//! battle the unlocked count is zero, (B) never synthesises an id and (E) never
//! fires.
//!
//! ## (D) is a replacement, not a detour
//!
//! `FUN_801D3748` is an 81-instruction leaf with exactly one caller
//! (`jal` at `0x801D21BC`) and no reference to its interior from anywhere -
//! every branch into its body comes from inside itself. So the whole body is
//! rewritten in place in the overlay, which costs no dead space at all. Retail
//! steps the page offset `0 -> 5 -> 10` and then closes the list; the
//! replacement steps while `scroll + 5 < effective count` (capped at
//! [`MAX_SCROLL`]). It also drops retail's "no arts at all -> do nothing" early
//! exit, so a character who has learned nothing but has an unlocked Super Art
//! can still open the list.
//!
//! ## Placement
//!
//! Four verified-dead SCUS regions, all of them shared with `--shiny-seru`,
//! `--arts-ap-grant` / `--arts-ap-cost` and `--delilas-challenge`, so this
//! toggle stays in that **mutually exclusive** set:
//!
//! | Region | Holds |
//! |---|---|
//! | [`SCUS_GAP_VA`] (256 B) | routine (B), the cache, the AP + finisher tables, the scratch record |
//! | [`ARENA1_VA`] (256 B) | the shared leaf and routine (A) |
//! | [`ARENA2_VA`] (72 B) | routine (E) |
//! | [`SLOT6_VA`] (68 B) | the fifteen chain bitmasks |
//!
//! The pager replacement lands in the overlay itself and takes no dead space.
//!
//! ## Known cosmetic gap
//!
//! `FUN_801D3444`'s caption thresholds (`< 6`, `< 11`) stay retail, so the
//! Triangle prompt can read "View Hyper Arts list" on a page where it should
//! read "View Next page". The list contents are correct; only that one caption
//! string is stale.
//!
//! No Sony bytes are embedded: the routines are the patcher's own code, and
//! every table entry is derived from the user's own disc (AP costs and chain
//! membership out of `SCUS_942.54`'s arts-name table) or from
//! [`legaia_art::SUPER_ARTS`], the repo's own capture-validated trigger table.

use anyhow::{Result, bail};

use legaia_art::queue::Character;

use crate::mips::*;
use crate::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, Edit, OVERLAY_TABLE_RANGES,
    SCUS_GAP_END_VA, SCUS_GAP_VA, SCUS_TABLE_RANGES, SLOT6_END_VA, SLOT6_VA,
};
use crate::super_art_power::{GRID_BIAS, super_arts_for};

/// PROT entry of the battle-action overlay that hosts the list pager.
pub const OVERLAY_PROT_INDEX: usize = legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base of that overlay: a VA maps to raw-entry file offset `va - base`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// Super Arts per character, and therefore the most rows the feature can add.
pub const SUPER_ARTS_PER_CHAR: usize = 5;
/// Characters with an arts list (`0` Vahn / `1` Noa / `2` Gala). Terra (`3`) has
/// no arts-name-table rows and is left at retail.
pub const ARTS_CHARACTERS: usize = 3;
/// Synthetic art id of the first Super Art row. Retail's table uses `0x00..=0x10`
/// only, so `0x40..=0x44` cannot collide; [`SuperArtListInjection::plan`]
/// re-checks that against the disc's own table.
pub const SYN_ID_BASE: u8 = 0x40;
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
/// Widest display id the chain bitmask can represent (one `u32` per Super Art).
pub const MAX_ART_ID: u8 = 31;

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

/// (E) `addiu s5,a1,0x8` - where the record cursor is set up, one instruction
/// ahead of the arts-name-table scan. **One word only**: the next word
/// [`SCAN_HEAD_VA`] is the scan's loop head and is branched to from
/// `0x80034744`, so it must stay put. The hook's own delay slot re-executes it
/// harmlessly and the routine returns to it.
pub const HOOK_REC_VA: u32 = 0x8003_4460;
pub(crate) const HOOK_REC_W0: u32 = 0x24B5_0008;
/// `lbu v1,0x0(a1)` - the scan loop head, fingerprinted, never written. A row
/// that is not a Super Art returns here, exactly as retail falls through.
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
/// Original body length in words - the replacement must fit inside it.
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

// Globals the routines touch, all read straight out of the retail code above.
/// `gp+0x874` - the acting character index (0-based).
const GP_CHARACTER: u16 = 0x874;
/// Base of the four live character records; `+ character*0x414` is one record.
const CHAR_RECORD_BASE: u32 = 0x8008_4140;
/// `record + 0x74D` - the learned-art count.
const LEARNED_COUNT_OFF: u16 = 0x74D;
/// `record + 0x74E` - the first of sixteen learned-art id slots.
const LEARNED_LIST_OFF: u16 = 0x74E;
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
/// Per-art-record stride inside that array. Routine (B) spells the multiply out
/// as retail does (`sll 1, addu, sll 2, addu, sll 4`); the test
/// `id_routine_chases_the_name_the_way_retail_does` pins that chain to this.
pub const ART_RECORD_STRIDE: u32 = 0xD0;
/// `record + 0x10` - the record's display-name field.
const ART_NAME_FIELD_OFF: u32 = 0x10;

// Scratch-record field offsets, in the arts-name-table record's own layout.
/// `+2` - the AP cost the digit loop draws.
const SCRATCH_AP_OFF: u16 = 0x2;
/// `+8` - the command-glyph string pointer (aimed at a zero byte: no arrows).
const SCRATCH_ARROWS_OFF: usize = 0x8;
/// `+0xC` - the display-name pointer.
const SCRATCH_NAME_OFF: u16 = 0xC;
/// Bytes of scratch record reserved (the record's own stride is `0x14`, but only
/// `+0..+0x10` is ever read through `s5 = record + 8`).
pub const SCRATCH_BYTES: usize = 0x10;

// --- Routine assembly --------------------------------------------------------

/// The shared leaf: work out which of the acting character's Super Arts are
/// unlocked. Returns the five-bit mask in `t9` and its population count in `t8`,
/// and caches both as two bytes at `cache_va` (`+0` mask, `+1` count) for the
/// per-row hook to read. Clobbers `t0`-`t9` and nothing else - in particular it
/// uses no `mult`/`div`, because `hi`/`lo` are live across hook (A).
///
/// The answer is zero outside battle (`_DAT_8007B83C != 0x15`) and zero for
/// Terra, which is what keeps hooks (B) and (E) inert in both cases.
pub(crate) fn assemble_sub(base_va: u32, masktab_va: u32, cache_va: u32) -> Vec<u32> {
    const L1: i32 = 20;
    const D1: i32 = 28;
    const L2: i32 = 35;
    const SKIP: i32 = 42;
    const STORE: i32 = 44;
    let cache_lo = lo(cache_va);
    let body = vec![
        or(T8, ZERO, ZERO),                            // 0  n = 0
        or(T9, ZERO, ZERO),                            // 1  mask = 0
        lui(T3, hi(GAME_MODE_VA)),                     // 2
        lh(T3, T3, lo(GAME_MODE_VA)),                  // 3  t3 = game mode
        lw(T0, GP, GP_CHARACTER),                      // 4  t0 = character (t3 delay)
        addiu(T3, T3, (-(BATTLE_MODE as i32)) as u16), // 5  mode - 0x15
        bne(T3, ZERO, (STORE - 7) as i16),             // 6  not in battle -> nothing
        sltiu(T1, T0, ARTS_CHARACTERS as u16),         // 7  delay: character < 3 ?
        beq(T1, ZERO, (STORE - 9) as i16),             // 8  Terra -> nothing
        sll(T1, T0, 6),                                // 9  delay: character * 64
        addu(T1, T1, T0),                              // 10 * 65
        sll(T1, T1, 2),                                // 11 * 260
        addu(T1, T1, T0),                              // 12 * 261
        sll(T1, T1, 2),                                // 13 * 0x414
        lui(T2, hi(CHAR_RECORD_BASE)),                 // 14
        addiu(T2, T2, lo(CHAR_RECORD_BASE)),           // 15
        addu(T1, T1, T2),                              // 16 t1 = the character record
        lbu(T4, T1, LEARNED_COUNT_OFF),                // 17 t4 = learned-art count
        addiu(T1, T1, LEARNED_LIST_OFF),               // 18 t1 = the id list
        or(T2, ZERO, ZERO),                            // 19 t2 = learned bitmask
        // L1: fold every learned id into a bitmask.
        beq(T4, ZERO, (D1 - 21) as i16), // 20
        ori(T3, ZERO, 1),                // 21 delay
        lbu(T5, T1, 0),                  // 22 t5 = one learned id
        addiu(T1, T1, 1),                // 23
        addiu(T4, T4, 0xFFFF),           // 24 count -= 1
        sllv(T3, T3, T5),                // 25 1 << id
        j(base_va + (L1 as u32) * 4),    // 26
        or(T2, T2, T3),                  // 27 delay
        // D1: a Super Art is unlocked when its whole chain is in that mask.
        sll(T6, T0, 2),                // 28
        addu(T6, T6, T0),              // 29 character * 5
        sll(T6, T6, 2),                // 30 * 4 (u32 entries)
        lui(T5, hi(masktab_va)),       // 31
        addiu(T5, T5, lo(masktab_va)), // 32
        addu(T6, T6, T5),              // 33 t6 = &chain_mask[char*5]
        ori(T7, ZERO, 1),              // 34 t7 = the row's bit
        // L2:
        lw(T5, T6, 0),                                 // 35 t5 = the chain mask
        addiu(T6, T6, 4),                              // 36
        and(T3, T5, T2),                               // 37
        bne(T3, T5, (SKIP - 39) as i16),               // 38 a chain art is missing
        sltiu(T4, T7, 1 << (SUPER_ARTS_PER_CHAR - 1)), // 39 delay: more rows?
        or(T9, T9, T7),                                // 40 unlocked
        addiu(T8, T8, 1),                              // 41
        // SKIP:
        bne(T4, ZERO, (L2 - 43) as i16), // 42
        sll(T7, T7, 1),                  // 43 delay: next row's bit
        // STORE:
        lui(T3, hi(cache_va)),    // 44
        sb(T9, T3, cache_lo),     // 45 cache[+0] = mask
        sb(T8, T3, cache_lo + 1), // 46 cache[+1] = count
        jr(RA),                   // 47
        nop(),                    // 48
    ];
    debug_assert_eq!(body.len(), STORE as usize + 5);
    body
}

/// (A) `count -> count + unlocked`. `ra` is dead here (`FUN_80034358` spilled it
/// at `0x54(sp)` in its prologue), so the routine simply calls the shared leaf.
/// `disp = [lbu v0,0x74d(v0), sra a2,a2,0x1f]`.
pub(crate) fn assemble_count(disp: [u32; 2], sub_va: u32, ret: u32) -> Vec<u32> {
    vec![
        disp[0],          // 0 v0 = learned count
        jal(sub_va),      // 1 t8 = unlocked Super Arts
        nop(),            // 2 delay (also v0's load delay)
        addu(V0, V0, T8), // 3 the walk runs that many more rows
        j(ret),           // 4
        disp[1],          // 5 delay: sra a2,a2,0x1f (replay)
    ]
}

/// (B) resolve the row's art id, and stage a Super Art row's record fields.
///
/// The Super Arts sit at the **head** of the list: row `entry < unlocked` is the
/// `entry`-th set bit of the cached mask and gets synthetic id `SYN_ID_BASE + j`,
/// while every later row reads the learned id at `entry - unlocked` - so the
/// learned half is shifted *down*, and unlike retail-plus-tail the read never
/// runs past the sixteen id slots. The stock `lbu` is replayed verbatim on that
/// path with the cursor moved back; the Super Art path skips it entirely, so the
/// character record is never even read out of range, let alone written.
///
/// On the Super Art path the routine also writes the two per-row fields of the
/// scratch record: `+2` = the chain's summed AP, `+0xC` = the runtime name
/// pointer chased through `DAT_801C9360[character] -> +0x58 -> +4 ->
/// (finisher - 0x10) * 0xD0 -> +0x10`.
///
/// `disp = [lbu s2,0x74e(v0), sltiu v1,v1,0x63]`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_id(
    disp: [u32; 2],
    base_va: u32,
    cache_va: u32,
    aptab_va: u32,
    fintab_va: u32,
    scratch_va: u32,
    ret: u32,
) -> Vec<u32> {
    const LOOP: i32 = 8;
    const NEXT: i32 = 13;
    const FOUND: i32 = 15;
    const PLAIN: i32 = 46;
    let cache_lo = lo(cache_va);
    let body = vec![
        lui(T3, hi(cache_va)),             // 0
        lbu(T0, T3, cache_lo),             // 1  t0 = unlocked mask
        lbu(T1, T3, cache_lo + 1),         // 2  t1 = unlocked count
        nop(),                             // 3  load delay
        sltu(T2, A2, T1),                  // 4  entry < unlocked ?
        beq(T2, ZERO, (PLAIN - 6) as i16), // 5  no -> a learned art
        or(T4, ZERO, ZERO),                // 6  delay: j = 0
        or(T5, A2, ZERO),                  // 7  c = entry
        // LOOP: walk to the entry-th set bit of the mask.
        andi(T6, T0, 1),                    // 8
        beq(T6, ZERO, (NEXT - 10) as i16),  // 9  bit clear -> next j
        srl(T0, T0, 1),                     // 10 delay: consume the bit
        beq(T5, ZERO, (FOUND - 12) as i16), // 11 this is the row
        addiu(T5, T5, 0xFFFF),              // 12 delay: c -= 1
        // NEXT:
        j(base_va + (LOOP as u32) * 4), // 13
        addiu(T4, T4, 1),               // 14 delay: j += 1
        // FOUND: t4 = the Super Art's index within the character's five.
        lw(T1, GP, GP_CHARACTER),                                    // 15
        addiu(S2, T4, u16::from(SYN_ID_BASE)),                       // 16 s2 = the synthetic id
        sll(T2, T1, 2),                                              // 17
        addu(T2, T2, T1),                                            // 18 character * 5
        addu(T4, T4, T2),                                            // 19 t4 = the table index
        lui(T3, hi(aptab_va)),                                       // 20
        addu(T3, T3, T4),                                            // 21
        lbu(T6, T3, lo(aptab_va)),       // 22 t6 = the chain's summed AP
        lui(T5, hi(fintab_va)),          // 23
        addu(T5, T5, T4),                // 24
        lbu(T7, T5, lo(fintab_va)),      // 25 t7 = the finisher constant
        sll(T2, T1, 2),                  // 26 character * 4
        lui(T8, hi(CMD_TABLE_VA)),       // 27
        addiu(T8, T8, lo(CMD_TABLE_VA)), // 28
        addu(T8, T8, T2),                // 29
        lw(T8, T8, 0),                   // 30 the command-data block
        addiu(T7, T7, (-(GRID_BIAS as i32)) as u16), // 31 delay filler: grid index
        lw(T8, T8, ART_BLOCK_PTR_OFF),   // 32 the art-record array
        sll(T2, T7, 1),                  // 33 delay filler
        addu(T2, T2, T7),                // 34 * 3
        sll(T2, T2, 2),                  // 35 * 12
        addu(T2, T2, T7),                // 36 * 13
        sll(T2, T2, 4),                  // 37 * 0xD0
        addu(T8, T8, T2),                // 38
        addiu(T8, T8, (ART_BLOCK_BIAS + ART_NAME_FIELD_OFF) as u16), // 39
        lui(T3, hi(scratch_va)),         // 40
        addiu(T3, T3, lo(scratch_va)),   // 41
        sb(T6, T3, SCRATCH_AP_OFF),      // 42 scratch[+2]   = AP
        sw(T8, T3, SCRATCH_NAME_OFF),    // 43 scratch[+0xC] = name
        j(ret),                          // 44
        disp[1],                         // 45 delay: sltiu v1,v1,0x63
        // PLAIN: a learned art, shifted down past the Super Art rows.
        subu(V0, V0, T1),              // 46
        lbu(S2, V0, LEARNED_LIST_OFF), // 47 == disp[0], re-based
        j(ret),                        // 48
        disp[1],                       // 49 delay: sltiu v1,v1,0x63
    ];
    debug_assert_eq!(body[47], disp[0], "the learned path replays the stock load");
    debug_assert_eq!(body.len(), PLAIN as usize + 4);
    body
}

/// (E) point the record cursor at the scratch record for a Super Art row.
///
/// Entered one instruction before the arts-name-table scan with `a1` = the table
/// base and `s2` = the row's id. A Super Art row jumps past the whole scan to
/// its **hit** arm ([`SCAN_HIT_VA`]) with `s5` = `scratch + 8`, which is exactly
/// the cursor retail's hit arm expects; every other row gets retail's own
/// `s5 = a1 + 8` and returns to the scan head.
pub(crate) fn assemble_rec(scratch_va: u32) -> Vec<u32> {
    const PLAIN: i32 = 7;
    let cursor = scratch_va + SCRATCH_ARROWS_OFF as u32;
    let body = vec![
        addiu(T0, S2, (-(SYN_ID_BASE as i32)) as u16), // 0 k = id - SYN_ID_BASE
        sltiu(T1, T0, SUPER_ARTS_PER_CHAR as u16),     // 1 k < 5 ? (k < 0 wraps)
        lui(T3, hi(cursor)),                           // 2
        beq(T1, ZERO, (PLAIN - 4) as i16),             // 3 not a Super Art row
        addiu(T3, T3, lo(cursor)),                     // 4 delay
        j(SCAN_HIT_VA),                                // 5 skip the scan entirely
        or(S5, T3, ZERO),                              // 6 delay: the scratch cursor
        // PLAIN:
        j(SCAN_HEAD_VA),  // 7
        addiu(S5, A1, 8), // 8 delay: retail's own s5
    ];
    debug_assert_eq!(
        body[8], HOOK_REC_W0,
        "the plain path replays the stock word"
    );
    debug_assert_eq!(body.len(), PLAIN as usize + 2);
    body
}

/// (D) the whole replacement pager, assembled at [`PAGER_VA`]. Same prologue,
/// pad gate, character-record walk and open/close tail as retail; the page step
/// becomes "advance while another page exists", and the row count it compares
/// against is the same `learned + unlocked` the renderer sees - taken from the
/// same shared leaf, not from the cache, so it is right even on the frame the
/// list first opens.
pub(crate) fn assemble_pager(base_va: u32, sub_va: u32) -> Vec<u32> {
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
        lbu(A0, V0, LEARNED_COUNT_OFF),      // 17 a0 = learned count
        jal(sub_va),                         // 18 t8 = unlocked Super Arts
        nop(),                               // 19 delay (also a0's load delay)
        addu(A0, A0, T8),                    // 20 a0 = rows the list will draw
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
    /// Trigger-chain arts as **display ids**, in trigger order (duplicates kept).
    pub chain_ids: Vec<u8>,
    /// The chain arts' names as the disc's own arts-name table spells them.
    pub chain_names: Vec<String>,
    /// Sum of the chain arts' AP costs - what the row displays.
    pub ap: u8,
    /// `1 << id` over the chain, deduplicated: the "is it unlocked" test.
    pub chain_mask: u32,
}

/// Derive all fifteen rows from `scus`. Every chain entry has to land on a real
/// arts-name-table row of the same character or the whole thing is refused: that
/// is the check that keeps the `constant - 0x1B` conversion honest against the
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
        for s in supers {
            let chain = s.art_sequence();
            if chain.is_empty() {
                bail!(
                    "show-super-arts: Super Art {} has an empty trigger chain - refusing",
                    s.name
                );
            }
            let mut ap: u32 = 0;
            let mut chain_mask = 0u32;
            let mut chain_ids = Vec::with_capacity(chain.len());
            let mut chain_names = Vec::with_capacity(chain.len());
            for c in chain {
                let id = c.checked_sub(ART_CONSTANT_BIAS).ok_or_else(|| {
                    anyhow::anyhow!(
                        "show-super-arts: {}'s chain entry {c:#x} is below the art-constant \
                         base {ART_CONSTANT_BIAS:#x}",
                        s.name
                    )
                })?;
                if id > MAX_ART_ID {
                    bail!(
                        "show-super-arts: {}'s chain art id {id} exceeds the {MAX_ART_ID}-bit \
                         mask the unlock test uses - refusing",
                        s.name
                    );
                }
                let rec = table
                    .iter()
                    .find(|r| r.character == ch && r.index == id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "show-super-arts: {}'s chain art (constant {c:#x} -> id {id}) has no \
                             row in this disc's arts-name table - refusing",
                            s.name
                        )
                    })?;
                ap += u32::from(rec.ap);
                chain_mask |= 1u32 << id;
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
            out.push(SuperArtRow {
                character: ch,
                name: s.name,
                finisher: s.finisher,
                chain_ids,
                chain_names,
                ap,
                chain_mask,
            });
        }
    }
    Ok(out)
}

// --- Planning ----------------------------------------------------------------

/// A planned "show Super Arts" injection: every same-size write, plus the exact
/// landing addresses so an oracle can pin them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListInjection {
    pub edits: Vec<Edit>,
    /// The fifteen derived rows, in `character * 5 + k` order.
    pub rows: Vec<SuperArtRow>,
    pub sub_va: u32,
    pub count_va: u32,
    pub id_va: u32,
    pub rec_va: u32,
    pub masktab_va: u32,
    pub aptab_va: u32,
    pub fintab_va: u32,
    pub cache_va: u32,
    pub scratch_va: u32,
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
    /// table, dead-space hosts) and the raw PROT 0898 entry (the pager).
    /// Refuses, without touching anything, if the build isn't the recognized US
    /// layout, a hosted region isn't dead space, a Super Art's chain doesn't
    /// resolve against this disc's arts-name table, the synthetic id space
    /// collides with a real art row, or a routine overruns its region.
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
        expect_scus(scus, HOOK_REC_VA, HOOK_REC_W0)?;
        // Never written, but the hooks' correctness depends on both staying put:
        // a plain row returns to the scan head and a Super Art row enters the
        // scan's hit arm.
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
        assert_not_in_tables(
            PAGER_VA,
            (PAGER_WORDS * 4) as u32,
            OVERLAY_TABLE_RANGES,
            "pager",
        )?;

        // The synthetic ids must be unreachable by any real arts-table row, or a
        // Super Art row could collide with a regular art's id.
        let rows_raw = legaia_art::arts_table::raw_records_from_scus(scus)
            .ok_or_else(|| anyhow::anyhow!("show-super-arts: parse the SCUS arts-name table"))?;
        let syn = SYN_ID_BASE..SYN_ID_BASE.saturating_add(SUPER_ARTS_PER_CHAR as u8);
        if let Some(bad) = rows_raw.iter().find(|r| syn.contains(&r.index)) {
            bail!(
                "show-super-arts: arts-name table row (character {:?}, id {:#x}) lands in the \
                 synthetic id range {:#x}..{:#x} - refusing",
                bad.character,
                bad.index,
                syn.start,
                syn.end
            );
        }

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
        // Gap 1 hosts routine (B) plus every small table; arena 1 the shared
        // leaf and (A); arena 2 the record hook; slot 6 the chain bitmasks.
        let mut gap = Region::new(SCUS_GAP_VA, SCUS_GAP_END_VA, "the SCUS rodata gap");
        let mut arena1 = Region::new(ARENA1_VA, ARENA1_END_VA, "arena 1");
        let mut arena2 = Region::new(ARENA2_VA, ARENA2_END_VA, "arena 2");
        let mut slot6 = Region::new(SLOT6_VA, SLOT6_END_VA, "slot 6");

        let id_len = (assemble_id(id_disp, 0, 0, 0, 0, 0, 0).len() * 4) as u32;
        let id_va = gap.take(id_len, 4, "the id routine")?;
        let cache_va = gap.take(2, 1, "the unlocked cache")?;
        let aptab_va = gap.take(want as u32, 1, "the AP table")?;
        let fintab_va = gap.take(want as u32, 1, "the finisher table")?;
        let scratch_va = gap.take(SCRATCH_BYTES as u32, 4, "the scratch record")?;

        let sub_len = (assemble_sub(0, 0, 0).len() * 4) as u32;
        let sub_va = arena1.take(sub_len, 4, "the shared unlock routine")?;
        let count_len = (assemble_count(count_disp, 0, 0).len() * 4) as u32;
        let count_va = arena1.take(count_len, 4, "the count routine")?;

        let rec_len = (assemble_rec(0).len() * 4) as u32;
        let rec_va = arena2.take(rec_len, 4, "the record routine")?;

        let masktab_va = slot6.take((want * 4) as u32, 4, "the chain bitmasks")?;

        // `sb rt,lo+1(base)` shares one `lui`, so the cache must not straddle a
        // low-half sign boundary.
        if hi(cache_va) != hi(cache_va + 1) {
            bail!("show-super-arts: the cache at {cache_va:#x} straddles a lui boundary");
        }

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
        let sub = assemble_sub(sub_va, masktab_va, cache_va);
        let count = assemble_count(count_disp, sub_va, COUNT_RET_VA);
        let id = assemble_id(
            id_disp, id_va, cache_va, aptab_va, fintab_va, scratch_va, ID_RET_VA,
        );
        let rec = assemble_rec(scratch_va);
        debug_assert_eq!((sub.len() * 4) as u32, sub_len);
        debug_assert_eq!((id.len() * 4) as u32, id_len);

        let masktab: Vec<u8> = rows
            .iter()
            .flat_map(|r| r.chain_mask.to_le_bytes())
            .collect();
        let aptab: Vec<u8> = rows.iter().map(|r| r.ap).collect();
        let fintab: Vec<u8> = rows.iter().map(|r| r.finisher).collect();
        // The scratch record: only `+8` is fixed, and it points at the record's
        // own `+0` byte, which nothing ever writes - so the glyph count reads 0
        // and no arrows are drawn.
        let mut scratch = vec![0u8; SCRATCH_BYTES];
        scratch[SCRATCH_ARROWS_OFF..SCRATCH_ARROWS_OFF + 4]
            .copy_from_slice(&scratch_va.to_le_bytes());

        // The pager replacement, nop-padded out to the original body length so
        // no stale retail instruction survives behind it.
        let mut pager = assemble_pager(PAGER_VA, sub_va);
        if pager.len() > PAGER_WORDS {
            bail!(
                "show-super-arts: the replacement pager ({} words) does not fit \
                 FUN_801D3748 ({PAGER_WORDS} words)",
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
        let edits = vec![
            // Detours over the renderer's three sites. (A) and (B) take two
            // words; (E) takes exactly one - see HOOK_REC_VA.
            scus_edit(scus_off(scus, HOOK_COUNT_VA)?, detour(count_va)),
            scus_edit(scus_off(scus, HOOK_ID_VA)?, detour(id_va)),
            scus_edit(scus_off(scus, HOOK_REC_VA)?, words_to_bytes(&[j(rec_va)])),
            // Routines + tables into the verified-dead SCUS regions.
            scus_edit(scus_off(scus, sub_va)?, words_to_bytes(&sub)),
            scus_edit(scus_off(scus, count_va)?, words_to_bytes(&count)),
            scus_edit(scus_off(scus, id_va)?, words_to_bytes(&id)),
            scus_edit(scus_off(scus, rec_va)?, words_to_bytes(&rec)),
            scus_edit(scus_off(scus, masktab_va)?, masktab),
            scus_edit(scus_off(scus, aptab_va)?, aptab),
            scus_edit(scus_off(scus, fintab_va)?, fintab),
            scus_edit(scus_off(scus, scratch_va)?, scratch),
            // The pager, replaced whole inside the overlay.
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: (PAGER_VA - OVERLAY_BASE_VA) as usize,
                bytes: words_to_bytes(&pager),
            },
        ];

        Ok(Self {
            edits,
            rows,
            sub_va,
            count_va,
            id_va,
            rec_va,
            masktab_va,
            aptab_va,
            fintab_va,
            cache_va,
            scratch_va,
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

    /// Every store opcode, so a test can assert a routine writes no memory.
    fn is_store(w: u32) -> bool {
        (0x28..=0x2b).contains(&(w >> 26))
    }

    const SUB_VA: u32 = ARENA1_VA;
    const MASK_VA: u32 = SLOT6_VA;
    const CACHE_VA: u32 = SCUS_GAP_VA + 0xC8;
    const AP_VA: u32 = CACHE_VA + 2;
    const FIN_VA: u32 = AP_VA + 15;
    const SCRATCH_VA: u32 = SCUS_GAP_VA + 0xE8;

    // --- (SUB) the shared unlock routine ------------------------------------

    #[test]
    fn sub_gates_on_battle_mode_and_on_having_an_arts_list() {
        let r = assemble_sub(SUB_VA, MASK_VA, CACHE_VA);
        assert_eq!(r[0], or(T8, ZERO, ZERO), "count starts at zero");
        assert_eq!(r[1], or(T9, ZERO, ZERO), "mask starts at zero");
        assert_eq!(r[3], lh(T3, T3, lo(GAME_MODE_VA)), "reads the game mode");
        assert_eq!(r[4], lw(T0, GP, GP_CHARACTER), "covers the mode load delay");
        assert_eq!(r[5], addiu(T3, T3, 0xFFEB), "mode - 0x15");
        assert_eq!(r[7], sltiu(T1, T0, 3), "character < 3");
        // Both guards leave t8/t9 at zero and fall through to the cache store.
        let store = r.len() - 5;
        for i in [6usize, 8] {
            let target = i as i32 + 1 + br_off(r[i]);
            assert_eq!(target as usize, store, "guard at {i} skips to the store");
        }
        assert_eq!(r[store], lui(T3, hi(CACHE_VA)));
        assert_eq!(r[store + 3], jr(RA), "the leaf returns");
    }

    #[test]
    fn sub_uses_no_multiplier_because_hi_lo_are_live_at_the_count_hook() {
        // `mult v1,v0` at 0x800343A0 is read by `mfhi t0` at 0x800343D4, which
        // is AFTER hook (A) - so anything (A) calls must leave hi/lo alone.
        for w in assemble_sub(SUB_VA, MASK_VA, CACHE_VA) {
            let special = (w >> 26) == 0 && (w & 0x3f) != 0;
            let funct = w & 0x3f;
            assert!(
                !(special && (0x18..=0x1b).contains(&funct)),
                "no mult/div in the shared leaf"
            );
        }
    }

    /// The register an instruction writes, for the ones these routines use.
    fn dest_reg(w: u32) -> Option<u32> {
        let op = w >> 26;
        match op {
            // SPECIAL: `rd`, except jr/jalr/branch-likes, which write nothing
            // interesting here (the leaf only uses jr).
            0 => match w & 0x3f {
                0x08 => None, // jr
                _ if w == 0 => None,
                _ => Some((w >> 11) & 0x1f),
            },
            // j / branches / stores write no register.
            0x02 | 0x04 | 0x05 | 0x06 | 0x07 | 0x28 | 0x29 | 0x2b => None,
            0x03 => Some(RA), // jal
            _ => Some((w >> 16) & 0x1f),
        }
    }

    #[test]
    fn sub_clobbers_only_the_temporaries_hook_a_can_spare() {
        // Hook (A) calls this leaf with v0 (the learned count), v1, a0 and a2
        // all live across the call, and `hi`/`lo` mid-`mult`. Everything the
        // leaf writes must therefore be a caller-saved temporary.
        for (i, w) in assemble_sub(SUB_VA, MASK_VA, CACHE_VA)
            .into_iter()
            .enumerate()
        {
            let Some(d) = dest_reg(w) else { continue };
            assert!(
                (T0..=T7).contains(&d) || d == T8 || d == T9,
                "instruction {i} writes r{d}, which hook (A) still needs"
            );
        }
    }

    #[test]
    fn sub_only_writes_the_two_cache_bytes() {
        let r = assemble_sub(SUB_VA, MASK_VA, CACHE_VA);
        let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
        assert_eq!(stores.len(), 2, "mask + count, and nothing else");
        assert_eq!(stores[0], sb(T9, T3, lo(CACHE_VA)));
        assert_eq!(stores[1], sb(T8, T3, lo(CACHE_VA) + 1));
    }

    #[test]
    fn sub_loops_close_on_themselves() {
        let r = assemble_sub(SUB_VA, MASK_VA, CACHE_VA);
        // The learned-id fold runs `count` times and jumps back to its head.
        assert_eq!(r[26], j(SUB_VA + 20 * 4), "learned-id loop head");
        assert_eq!(r[25], sllv(T3, T3, T5), "1 << id");
        let exit = 20 + 1 + br_off(r[20]);
        assert_eq!(exit, 28, "empty learned list falls straight through");
        // The per-Super test runs exactly five times: the row bit walks
        // 1,2,4,8,0x10 and the continue test is `bit < 0x10`.
        assert_eq!(r[39], sltiu(T4, T7, 0x10));
        assert_eq!(r[43], sll(T7, T7, 1));
        let back = 42 + 1 + br_off(r[42]);
        assert_eq!(back, 35, "the per-Super loop closes on its head");
        let skip = 38 + 1 + br_off(r[38]);
        assert_eq!(skip, 42, "a missing chain art skips the unlock");
    }

    #[test]
    fn sub_load_delay_slots_are_covered() {
        let r = assemble_sub(SUB_VA, MASK_VA, CACHE_VA);
        // Every load's delay slot must not read the register being loaded:
        // lh t3 (3), lbu t4 (17), lbu t5 (22), lw t5 (35).
        for load in [3usize, 17, 22, 35] {
            let dest = (r[load] >> 16) & 0x1f;
            let filler = r[load + 1];
            let (rs, rt) = ((filler >> 21) & 0x1f, (filler >> 16) & 0x1f);
            assert_ne!(rs, dest, "delay slot after the load at {load} reads it");
            assert_ne!(rt, dest, "delay slot after the load at {load} reads it");
        }
        // ...and the first real use is at least two instructions later.
        for (load, first_use) in [(17usize, 20usize), (22, 25), (35, 37)] {
            assert!(first_use > load + 1, "load at {load} used inside its delay");
        }
    }

    #[test]
    fn unlock_model_matches_what_the_routine_encodes() {
        // Model of the mask fold + per-Super test the routine performs.
        let unlocked = |learned: &[u8], chains: &[u32]| -> (u32, u32) {
            let mut lm = 0u32;
            for &id in learned {
                lm |= 1u32 << (id & 31);
            }
            let (mut mask, mut n) = (0u32, 0u32);
            for (j, &c) in chains.iter().enumerate() {
                if c & lm == c {
                    mask |= 1 << j;
                    n += 1;
                }
            }
            (mask, n)
        };
        let chains = [0b0011, 0b0101, 0b1001, 0b0110, 0b1100];
        // Nothing learned: nothing unlocked, so the list is empty.
        assert_eq!(unlocked(&[], &chains), (0, 0));
        // Knowing one art of a two-art chain is not enough.
        assert_eq!(unlocked(&[0], &chains), (0, 0));
        assert_eq!(unlocked(&[0, 1], &chains), (0b00001, 1));
        assert_eq!(unlocked(&[0, 1, 2], &chains), (0b01011, 3));
        assert_eq!(unlocked(&[0, 1, 2, 3], &chains), (0b11111, 5));
    }

    // --- (A) the count hook --------------------------------------------------

    #[test]
    fn count_routine_adds_the_unlocked_rows() {
        let disp = [HOOK_COUNT_W0, HOOK_COUNT_W1];
        let r = assemble_count(disp, SUB_VA, COUNT_RET_VA);
        assert_eq!(r[0], HOOK_COUNT_W0, "replays the displaced count load");
        assert_eq!(r[1], jal(SUB_VA), "ra is dead here, so a call is free");
        assert_eq!(r[2], nop(), "delay slot, and v0's load delay");
        assert_eq!(r[3], addu(V0, V0, T8), "count += unlocked Super Arts");
        assert_eq!(r[4], j(COUNT_RET_VA));
        assert_eq!(r[5], HOOK_COUNT_W1, "replays sra a2,a2,0x1f");
        assert_eq!(r.len(), 6);
        assert!(
            !r.iter().copied().any(is_store),
            "no store in the count hook"
        );
    }

    // --- (B) the id hook -----------------------------------------------------

    fn id_routine() -> Vec<u32> {
        assemble_id(
            [HOOK_ID_W0, HOOK_ID_W1],
            SCUS_GAP_VA,
            CACHE_VA,
            AP_VA,
            FIN_VA,
            SCRATCH_VA,
            ID_RET_VA,
        )
    }

    #[test]
    fn id_routine_puts_the_super_arts_at_the_head() {
        let r = id_routine();
        assert_eq!(r[1], lbu(T0, T3, lo(CACHE_VA)), "reads the cached mask");
        assert_eq!(r[2], lbu(T1, T3, lo(CACHE_VA) + 1), "and the cached count");
        assert_eq!(r[3], nop(), "load-delay slot before the compare");
        assert_eq!(
            r[4],
            sltu(T2, A2, T1),
            "entry < unlocked -> a Super Art row"
        );
        assert_eq!(
            r[16],
            addiu(S2, T4, u16::from(SYN_ID_BASE)),
            "the head rows carry synthetic ids"
        );
        // The learned half is shifted DOWN by the number of head rows, so the
        // read stays inside the sixteen id slots instead of running past them.
        let plain = r.len() - 4;
        assert_eq!(r[plain], subu(V0, V0, T1), "cursor moves back by unlocked");
        assert_eq!(r[plain + 1], HOOK_ID_W0, "then replays the stock load");
        assert_eq!(r[plain + 2], j(ID_RET_VA));
        assert_eq!(r[plain + 3], HOOK_ID_W1);
        let target = 5 + 1 + br_off(r[5]);
        assert_eq!(target as usize, plain, "the miss branch lands on the shift");
    }

    #[test]
    fn id_routine_selects_the_nth_unlocked_super_art() {
        // Model of the "walk to the entry-th set bit" loop.
        let nth = |mask: u32, entry: u32| -> u32 {
            let (mut m, mut c, mut j) = (mask, entry, 0u32);
            loop {
                let bit = m & 1;
                m >>= 1;
                if bit != 0 {
                    if c == 0 {
                        return j;
                    }
                    c -= 1;
                }
                j += 1;
            }
        };
        assert_eq!(nth(0b11111, 0), 0);
        assert_eq!(nth(0b11111, 4), 4);
        // Holes: only Super Arts 1 and 3 are unlocked, so rows 0 and 1 are them.
        assert_eq!(nth(0b01010, 0), 1);
        assert_eq!(nth(0b01010, 1), 3);
        assert_eq!(nth(0b10000, 0), 4);
    }

    #[test]
    fn id_routine_branches_land_where_the_comments_say() {
        let r = id_routine();
        assert_eq!(
            r[13],
            j(SCUS_GAP_VA + 8 * 4),
            "the bit walk closes on itself"
        );
        let next = 9 + 1 + br_off(r[9]);
        assert_eq!(next, 13, "a clear bit steps to the next Super Art");
        let found = 11 + 1 + br_off(r[11]);
        assert_eq!(found, 15, "the entry-th set bit falls into the fill");
    }

    #[test]
    fn id_routine_chases_the_name_the_way_retail_does() {
        let r = id_routine();
        // DAT_801C9360[character] -> +0x58 -> +4 -> (fin - 0x10) * 0xD0 -> +0x10.
        assert_eq!(r[27], lui(T8, hi(CMD_TABLE_VA)));
        assert_eq!(r[28], addiu(T8, T8, lo(CMD_TABLE_VA)));
        assert_eq!(r[30], lw(T8, T8, 0), "the command-data block");
        assert_eq!(r[31], addiu(T7, T7, 0xFFF0), "grid index = finisher - 0x10");
        assert_eq!(r[32], lw(T8, T8, ART_BLOCK_PTR_OFF), "the art-record array");
        // *0xD0 the same way retail spells it at 0x8004BBF0..0x8004BC00.
        assert_eq!(r[33], sll(T2, T7, 1));
        assert_eq!(r[34], addu(T2, T2, T7));
        assert_eq!(r[35], sll(T2, T2, 2));
        assert_eq!(r[36], addu(T2, T2, T7));
        assert_eq!(r[37], sll(T2, T2, 4));
        // That shift chain is x -> 2x+x -> *4 -> +x -> *16 = 208x. Run it on a
        // real value so the constants, not a folded literal, are what is pinned.
        let mul_by_stride = |x: u32| (((x << 1) + x) << 2).wrapping_add(x) << 4;
        assert_eq!(mul_by_stride(1), ART_RECORD_STRIDE);
        assert_eq!(mul_by_stride(7), 7 * ART_RECORD_STRIDE);
        assert_eq!(r[39], addiu(T8, T8, 0x14), "+4 art block, +0x10 name field");
        // Both pointer loads have their delay slot filled with real work, and
        // neither filler reads the register being loaded.
        for (load, filler) in [(30usize, 31usize), (32, 33)] {
            let dest = (r[load] >> 16) & 0x1f;
            assert_ne!(r[filler], nop(), "delay slot after the load at {load}");
            let (rs, rt) = ((r[filler] >> 21) & 0x1f, (r[filler] >> 16) & 0x1f);
            assert_ne!(rs, dest, "filler at {filler} reads the loading register");
            assert_ne!(rt, dest, "filler at {filler} reads the loading register");
        }
    }

    #[test]
    fn id_routine_writes_only_the_scratch_record() {
        let r = id_routine();
        let stores: Vec<u32> = r.iter().copied().filter(|&w| is_store(w)).collect();
        assert_eq!(stores.len(), 2, "AP + name pointer, and nothing else");
        assert_eq!(stores[0], sb(T6, T3, SCRATCH_AP_OFF));
        assert_eq!(stores[1], sw(T8, T3, SCRATCH_NAME_OFF));
        // Structural restatement of the rule the hook must not break: the only
        // memory op touching the character record is the stock `lbu`, a load.
        assert_eq!(HOOK_ID_W0 >> 26, 0x24, "stock word is lbu (a load)");
    }

    #[test]
    fn id_routine_never_moves_the_stack_pointer() {
        for w in id_routine() {
            let is_addiu_sp = (w >> 26) == 0x09 && ((w >> 21) & 0x1f) == SP;
            assert!(!is_addiu_sp, "the id hook borrows retail's frame");
        }
    }

    // --- (E) the record hook -------------------------------------------------

    #[test]
    fn rec_routine_skips_the_scan_only_for_a_super_art_row() {
        let r = assemble_rec(SCRATCH_VA);
        assert_eq!(r[0], addiu(T0, S2, 0xFFC0), "k = id - SYN_ID_BASE");
        assert_eq!(r[1], sltiu(T1, T0, 5), "unsigned, so a negative k fails");
        assert_eq!(r[5], j(SCAN_HIT_VA), "a Super Art row enters the hit arm");
        assert_eq!(
            r[6],
            or(S5, T3, ZERO),
            "with s5 already at scratch + 8, the cursor the hit arm expects"
        );
        assert_eq!(r[7], j(SCAN_HEAD_VA), "every other row returns to the scan");
        assert_eq!(r[8], HOOK_REC_W0, "replaying the displaced addiu s5,a1,0x8");
        let plain = 3 + 1 + br_off(r[3]);
        assert_eq!(plain, 7, "the guard lands on the plain return");
        assert_eq!(r.len(), 9);
        assert!(
            !r.iter().copied().any(is_store),
            "no store in the record hook"
        );
        assert_eq!(SCAN_HEAD_VA, HOOK_REC_VA + 4, "one-word detour only");
    }

    #[test]
    fn scratch_cursor_lines_up_with_the_fields_retail_reads() {
        // Retail's hit arm reads rec[+0xC] as `lw a0,0x4(s5)`, rec[+2] as
        // `lbu s0,-0x6(s5)` and rec[+8] as `lw v1,0x0(s5)` - all off s5 = rec+8.
        let s5 = SCRATCH_ARROWS_OFF as i32;
        assert_eq!(s5 + 4, SCRATCH_NAME_OFF as i32, "name at s5+4");
        assert_eq!(s5 - 6, SCRATCH_AP_OFF as i32, "AP at s5-6");
        assert_eq!(s5, SCRATCH_ARROWS_OFF as i32, "glyph string at s5+0");
    }

    // --- (D) the pager -------------------------------------------------------

    #[test]
    fn pager_fits_the_original_body_and_its_branches_land() {
        let r = assemble_pager(PAGER_VA, SUB_VA);
        assert!(r.len() <= PAGER_WORDS, "{} words", r.len());
        assert_eq!(r[0], addiu(SP, SP, 0xFFE8), "same frame as retail");
        assert_eq!(r[r.len() - 2], jr(RA));
        assert_eq!(r[r.len() - 1], addiu(SP, SP, 0x18));
        assert_eq!(r[18], jal(SUB_VA), "the pager asks the same leaf");
        assert_eq!(r[20], addu(A0, A0, T8), "rows = learned + unlocked");
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
        // The comparison the routine encodes: advance iff offset < MAX_SCROLL
        // and offset + PAGE_ROWS < rows.
        let step = |offset: u16, rows: u32| -> Option<u16> {
            if offset < MAX_SCROLL && u32::from(offset) + PAGE_ROWS < rows {
                Some(offset + PAGE_ROWS as u16)
            } else {
                None
            }
        };
        // Sixteen learned arts + five unlocked Super Arts = five pages.
        assert_eq!(step(0, 21), Some(5));
        assert_eq!(step(15, 21), Some(20));
        assert_eq!(step(20, 21), None, "closes after the last page");
        // Nothing learned, two Super Arts unlocked: one page.
        assert_eq!(step(0, 2), None);
        // One learned art plus five Super Arts: two pages.
        assert_eq!(step(0, 6), Some(5));
        assert_eq!(step(5, 6), None);
        // Terra, and any character before the first Super Art unlocks with an
        // empty learned list: retail's empty list.
        assert_eq!(step(0, 0), None);
    }

    #[test]
    fn pager_delay_slots_are_harmless_when_the_branch_is_taken() {
        let r = assemble_pager(PAGER_VA, SUB_VA);
        // Each conditional branch's delay slot writes a register the taken arm
        // re-initialises before use (v0 at TOGGLE, v1 at TOGGLE, a1 unused).
        assert_eq!(r[8], lui(V0, 0x8008));
        assert_eq!(r[22], lui(V0, 0x801F));
        assert_eq!(r[27], lui(A1, 0x8008));
        assert_eq!(r[32], addiu(V0, V1, 5));
        assert_eq!(r[35], addiu(V1, V1, 5));
        assert_eq!(
            r[45],
            sb(A0, V0, 0x4E09),
            "the store is unconditional in retail too"
        );
    }

    // --- Table derivation ----------------------------------------------------

    #[test]
    fn chain_conversion_is_the_documented_constant_offset() {
        // Display row n is action constant 0x1B + n - the same relation
        // `super_art_power::art_block_base` already solves the art block with.
        for s in legaia_art::SUPER_ARTS {
            for c in s.art_sequence() {
                assert!(
                    c >= ART_CONSTANT_BIAS,
                    "{}: chain constant {c:#x} is below the base",
                    s.name
                );
                assert!(
                    c - ART_CONSTANT_BIAS <= MAX_ART_ID,
                    "{}: chain id {} needs more than 32 mask bits",
                    s.name,
                    c - ART_CONSTANT_BIAS
                );
            }
        }
    }

    #[test]
    fn every_super_art_has_a_multi_art_chain_so_the_gate_can_never_be_vacuous() {
        for s in legaia_art::SUPER_ARTS {
            assert!(
                s.art_sequence().len() >= 2,
                "{} would unlock on a single art",
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

    // --- Placement -----------------------------------------------------------

    #[test]
    fn every_routine_and_table_fits_its_region() {
        let sub = assemble_sub(0, 0, 0).len() * 4;
        let count = assemble_count([HOOK_COUNT_W0, HOOK_COUNT_W1], 0, 0).len() * 4;
        let id = assemble_id([HOOK_ID_W0, HOOK_ID_W1], 0, 0, 0, 0, 0, 0).len() * 4;
        let rec = assemble_rec(0).len() * 4;
        let n = ARTS_CHARACTERS * SUPER_ARTS_PER_CHAR;
        // Gap 1: routine (B), the cache, two byte tables and the scratch record.
        let gap = id + 2 + n + n + SCRATCH_BYTES + 4; // + worst-case alignment
        assert!(
            gap <= (SCUS_GAP_END_VA - SCUS_GAP_VA) as usize,
            "{gap} B in a {} B gap",
            SCUS_GAP_END_VA - SCUS_GAP_VA
        );
        assert!(
            sub + count <= (ARENA1_END_VA - ARENA1_VA) as usize,
            "{} B in a {} B arena",
            sub + count,
            ARENA1_END_VA - ARENA1_VA
        );
        assert!(rec <= (ARENA2_END_VA - ARENA2_VA) as usize);
        assert!(n * 4 <= (SLOT6_END_VA - SLOT6_VA) as usize);
        // The whole feature still leaves nothing over for a second arena
        // feature, which is why the toggle is mutually exclusive with them.
        eprintln!(
            "sub {sub} + count {count} + id {id} + rec {rec} = {} B of code, {} B of tables",
            sub + count + id + rec,
            2 + n + n + n * 4 + SCRATCH_BYTES
        );
    }

    #[test]
    fn the_scratch_glyph_pointer_aims_at_a_byte_that_is_never_written() {
        // rec[+8] points at the record's own +0, and the only fields the id
        // routine writes are +2 and +0xC - so byte 0 stays the zero glyph count.
        let written: Vec<u16> = id_routine()
            .iter()
            .filter(|&&w| is_store(w))
            .map(|w| (w & 0xffff) as u16)
            .collect();
        assert_eq!(written, vec![SCRATCH_AP_OFF, SCRATCH_NAME_OFF]);
        assert!(!written.contains(&0), "nothing writes the glyph-count byte");
        const { assert!(SCRATCH_ARROWS_OFF + 4 <= SCRATCH_BYTES) };
    }
}
