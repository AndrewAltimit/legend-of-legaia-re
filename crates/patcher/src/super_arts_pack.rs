//! **Super Arts Pack, by ZetaPhoenix** - fifteen extra Super Arts, one per
//! character-slot beyond the retail five, with their own names, hit counts and
//! per-art animation edits.
//!
//! The pack is ZetaPhoenix's work. He authored it as a GameShark-style RAM
//! patch: a 3764-byte block forced into main RAM at `0x801FD000` during battle
//! plus a handful of word edits that make retail code reach it. This module is
//! the disc-patch carrier for exactly those bytes -
//! [`BLOCK`](crate::super_arts_pack::BLOCK) is installed **unmodified**; nothing
//! here re-assembles, relocates or rewrites the pack.
//!
//! ## What the block holds
//!
//! Every address below is a VA inside the loaded block, and each is confirmed by
//! the block's own code referencing it.
//!
//! | VA | What |
//! |---|---|
//! | `0x801FD000` | **Find** table: 13-byte rows (`[len][bytes][pad]`), **10 rows per character** - rows 0..4 are the retail five verbatim, rows 5..9 are ZetaPhoenix's |
//! | `0x801FD186` | **Replace** table: 16-byte rows, same order |
//! | `0x801FD366` | 15 hit-count seeds (`1` or `3`), one per added Super Art |
//! | `0x801FD376` | `u16` runtime cell - the per-queue "these hits are an added Super" bit train |
//! | `0x801FD378` | `u8` runtime cell - which added Super Art (0..14) is playing |
//! | `0x801FD380` | routine **A**: the applier's post-match arm |
//! | `0x801FD3DC` | routine **B**: reset the two cells when a new queue is built |
//! | `0x801FD400` | fifteen 16-byte names ("Ultra Elbow", "Somersault Duo", ...) |
//! | `0x801FD4F0` | three per-character action-constant groups (6 / 5 / 6 = 17) |
//! | `0x801FD510` | routine **C**: keep an installed banner name from being overwritten |
//! | `0x801FD538` | routine **D**: install the name + apply the animation edit-list |
//! | `0x801FD64C` | routine A's tail (walks the queue, builds the bit train) |
//! | `0x801FD71C` / `0x801FD760` | two 17-entry pointer tables (added-Super clips / retail clips) |
//! | `0x801FD7A4` | the 34 animation edit-lists: `[i16 offset][u16 values..][0xFFFF]` groups |
//!
//! ## How the bytes get there
//!
//! `0x801FD000..0x801FE000` is free RAM in every state of this project's save
//! library (60 states, battles included): all-zero throughout, with the deepest
//! observed stack use at `0x801FE420` - 1388 bytes above the block's end. It is
//! not covered by any overlay: slot A ends at `0x801F7018` (PROT 0898's
//! `0x28800` bytes from `0x801CE818`).
//!
//! So the block is **parked in the `DMY.DAT` annex** ([`crate::disc::DiscPatcher::annex_blob`])
//! and streamed to `0x801FD000` at battle load by a 12-instruction stub in the
//! verified-dead SCUS arena. The stub is entered from battle init
//! (`FUN_80055B6C`) at [`LOAD_HOOK_VA`] - **after** the `FUN_8003DE7C(0)` that
//! waits for the battle overlay's own CD read, so the drive is idle and the read
//! is the same between-frames context every other load runs in. It calls the
//! game's own synchronous reader `FUN_8005E4D4(sectors, lba, dest)`, then the
//! BIOS `FlushCache` (the block is code, and the I-cache is not DMA-coherent).
//!
//! Growing PROT 0898 to cover `0x801FD000` was the alternative and is worse: it
//! needs 14 sectors taken from a neighbour and would zero `0x801F7018..0x801FD000`
//! at every battle load - the persistent `0x801F****` effect/world state the
//! field overlay leaves there.
//!
//! ## The hooks
//!
//! Ten same-size word edits, every one of them derived from the block's own code
//! (each routine replays the exact instruction it displaces and returns to the
//! instruction after it, which pins the site):
//!
//! | Site | Retail word | Becomes |
//! |---|---|---|
//! | `0x801EF9E8` | `move t5,a1` | `sll t5,a1,1` - the applier's per-character stride doubles (5 rows -> 10) |
//! | `0x801EFA38` / `0x801EFA3C` | `lui v0,0x801f` / `addiu t6,v0,0x6524` | the find table becomes `0x801FD000` |
//! | `0x801EFA58` / `0x801EFA5C` | `lui v0,0x801f` / `addiu t8,v0,0x65e8` | the replace table becomes `0x801FD186` |
//! | `0x801EFBE0` | `slti v0,a1,5` | `slti v0,a1,10` - the applier tries all ten rows |
//! | `0x801EFB94` | `nop` (a load-delay slot) | `j 0x801FD380` - routine A |
//! | `0x801EED20` | `move t9,a0` | `j` the arena trampoline -> routine B |
//! | `0x8004BC10` | `sw v0,0x74c(a0)` | `j 0x801FD538` - routine D |
//! | `0x8004C718` / `0x8004C71C` | `sw v0,0x74c(s0)` / `sw v0,0x734(s0)` | `j 0x801FD510` + `nop` - routine C |
//! | `0x80055DBC` | `lui a2,0x1f80` | `j` the arena loader stub |
//!
//! The `t5` edit is what makes the applier's own arithmetic land on the wider
//! table: it computes the character's find base as `t5*65` and its replace base
//! as `t5*80`, so a doubled `t5` gives `130 = 10*13` and `160 = 10*16` exactly.
//! It is also what makes routine A's `(t5>>1) + t5 + t5` read as `5*character` -
//! the flat 0..14 index it stores at `0x801FD378` and uses against the hit-count
//! and name tables. Nothing else in the applier reads `t5`.
//!
//! **What is ours, not ZetaPhoenix's.** He supplied the block and described the
//! hooks as "a few more lines that replace lines from the OG code (mostly jumps
//! to my new code)", but not the lines themselves. The nine jump/retarget words
//! above are this project's reconstruction from his block; the tenth (the loader
//! stub + its hook) is ours by construction, because his RAM patch had no need
//! to load anything. Two deliberate departures, both to keep his bytes exact:
//!
//! - The `0x801EED20` hook goes through a 2-word arena trampoline instead of
//!   detouring `0x801EED24` directly. A `j` at `0x801EED24` would run
//!   `move s3,zero` in its delay slot, so routine B's replayed `sw s3,0x54(sp)`
//!   would spill 0 instead of the caller's `s3`. Hooking one word earlier keeps
//!   the spill honest and replays `move t9,a0` in the trampoline's delay slot.
//! - `0x8004C71C` is nopped rather than left as `sw v0,0x734(s0)`, because
//!   routine C re-stores **both** words on its non-skip path and stores neither
//!   on its skip path.
//!
//! Retail's own five per character keep their triggers and their queue results
//! exactly: rows 0..4 of each character in the pack are the disc's own trigger
//! rows (checked byte-for-byte at patch time), and routine A returns
//! immediately for them (`slti t0,0x50`). Their **animations** do pass through
//! the pack, though - routine D fires for any action in the seventeen listed
//! constants, and three of Noa's and Gala's retail finishers are in that list.
//! Each clip is an `(offset, values)` edit-list pair over the art record: the
//! "A" list adds the Super variant's extra element and bumps the record's
//! `+0x14` / `+0x8C` / `+0x9A` fields, the "B" list writes the plain values
//! back. So a shared constant plays with the pack's B values, which is
//! ZetaPhoenix's own restore path, not a side effect of this carrier.

use anyhow::{Context, Result, bail};

use crate::mips::{
    A0, A1, A2, T1, T2, T5, T6, T8, V0, ZERO, addiu, j, jal, jalr, lui, nop, ori, read_word, sll,
    slti,
};
use crate::shiny_seru::{ARENA1_END_VA, ARENA1_VA, Edit, OVERLAY_TABLE_RANGES, SCUS_TABLE_RANGES};

/// ZetaPhoenix's block, verbatim. See `crates/patcher/data/README.md` for its
/// provenance and the licence note.
pub const BLOCK: &[u8] = include_bytes!("../data/zetaphoenix-super-arts-pack.bin");

/// Where the block runs.
pub const BLOCK_VA: u32 = 0x801F_D000;
/// Whole 2048-byte sectors the block occupies in the annex (and the sector count
/// the loader stub asks the CD reader for).
pub const BLOCK_SECTORS: u32 = 2;

/// PROT entry index of the battle-action overlay hosting the applier + the
/// party arts queue builder.
pub const OVERLAY_PROT_INDEX: usize = 898;
/// Load base VA of the slot-A overlays.
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;

// --- Block-internal addresses (each confirmed by the block's own code) -------

/// Find table (13-byte rows, 10 rows per character).
pub const FIND_TABLE_VA: u32 = 0x801F_D000;
/// Replace table (16-byte rows, 10 rows per character).
pub const REPLACE_TABLE_VA: u32 = 0x801F_D186;
/// Row stride of the find table.
pub const FIND_STRIDE: usize = 13;
/// Row stride of the replace table.
pub const REPLACE_STRIDE: usize = 16;
/// Rows per character in both tables.
pub const ROWS_PER_CHAR: usize = 10;
/// Rows per character that are the retail Super Arts (the first five).
pub const RETAIL_ROWS: usize = 5;
/// The fifteen 16-byte Super Art names.
pub const NAME_TABLE_VA: u32 = 0x801F_D400;
/// Stride of the name table.
pub const NAME_STRIDE: usize = 16;
/// Routine A - entered from the applier's post-match arm.
pub const ROUTINE_APPLIER_VA: u32 = 0x801F_D380;
/// Routine B - entered from the queue builder's prologue.
pub const ROUTINE_QUEUE_VA: u32 = 0x801F_D3DC;
/// Routine C - entered from `FUN_8004C650`'s banner-name store.
pub const ROUTINE_KEEP_NAME_VA: u32 = 0x801F_D510;
/// Routine D - entered from `FUN_8004AD80`'s banner-name store.
pub const ROUTINE_BANNER_VA: u32 = 0x801F_D538;

// --- Retail trigger tables the pack supersedes -------------------------------

/// Retail find table in PROT 0898 (5 rows per character).
pub const RETAIL_FIND_VA: u32 = 0x801F_6524;
/// Retail replace table in PROT 0898.
pub const RETAIL_REPLACE_VA: u32 = 0x801F_65E8;

// --- Hook sites (overlay 0898) ----------------------------------------------

/// `FUN_801EF9E4` entry, fingerprinted so a re-based overlay is refused.
pub const APPLIER_VA: u32 = 0x801E_F9E4;
const APPLIER_W: u32 = 0x27BD_FFF8; // addiu sp,sp,-8

/// The applier's `move t5,a1` (its per-character stride source).
pub const HOOK_STRIDE_VA: u32 = 0x801E_F9E8;
const HOOK_STRIDE_W: u32 = 0x00A0_6821;

/// The applier's find-table `lui` / `addiu` pair.
pub const HOOK_FIND_HI_VA: u32 = 0x801E_FA38;
const HOOK_FIND_HI_W: u32 = 0x3C02_801F;
pub const HOOK_FIND_LO_VA: u32 = 0x801E_FA3C;
const HOOK_FIND_LO_W: u32 = 0x244E_6524;

/// The applier's replace-table `lui` / `addiu` pair.
pub const HOOK_REPL_HI_VA: u32 = 0x801E_FA58;
const HOOK_REPL_HI_W: u32 = 0x3C02_801F;
pub const HOOK_REPL_LO_VA: u32 = 0x801E_FA5C;
const HOOK_REPL_LO_W: u32 = 0x2458_65E8;

/// The applier's row-loop bound (`slti v0,a1,5`).
pub const HOOK_BOUND_VA: u32 = 0x801E_FBE0;
const HOOK_BOUND_W: u32 = 0x28A2_0005;

/// The applier's post-match arm - a load-delay `nop`, so routine A displaces
/// nothing and the following `subu v0,t4,v0` runs as the jump's delay slot.
pub const HOOK_APPLIER_VA: u32 = 0x801E_FB94;
const HOOK_APPLIER_W: u32 = 0x0000_0000;
/// The instruction routine A returns to.
pub const APPLIER_RET_VA: u32 = 0x801E_FB9C;

/// `FUN_801EED1C`'s `move t9,a0` (its second instruction). Routine B is reached
/// through the arena trampoline so the `sw s3,0x54(sp)` in the delay slot still
/// spills the caller's `s3`.
pub const HOOK_QUEUE_VA: u32 = 0x801E_ED20;
const HOOK_QUEUE_W: u32 = 0x0080_C821;
/// The instruction routine B returns to.
pub const QUEUE_RET_VA: u32 = 0x801E_ED28;

// --- Hook sites (SCUS_942.54) -----------------------------------------------

/// `FUN_8004AD80`'s banner-name store; routine D replays it.
pub const HOOK_BANNER_VA: u32 = 0x8004_BC10;
const HOOK_BANNER_W: u32 = 0xAC82_074C;
/// The instruction routine D returns to.
pub const BANNER_RET_VA: u32 = 0x8004_BC18;

/// `FUN_8004C650`'s banner-name store pair; routine C replays both.
pub const HOOK_KEEP_NAME_VA: u32 = 0x8004_C718;
const HOOK_KEEP_NAME_W0: u32 = 0xAE02_074C;
const HOOK_KEEP_NAME_W1: u32 = 0xAE02_0734;
/// The instruction routine C returns to.
pub const KEEP_NAME_RET_VA: u32 = 0x8004_C720;

/// Battle init (`FUN_80055B6C`), one instruction past the `FUN_8003DE7C(0)` that
/// waits for the battle overlay's CD read. The loader stub replays this word and
/// the `ori` in its delay slot.
pub const LOAD_HOOK_VA: u32 = 0x8005_5DBC;
const LOAD_HOOK_W0: u32 = 0x3C06_1F80; // lui a2,0x1f80
const LOAD_HOOK_W1: u32 = 0x34C6_0314; // ori a2,a2,0x314
/// Where the loader stub returns.
pub const LOAD_RET_VA: u32 = 0x8005_5DC4;

/// The game's synchronous sector reader `FUN_8005E4D4(count, lba, dest)`.
const LOADER_FN: u32 = 0x8005_E4D4;
/// BIOS A-table dispatcher entry and the `FlushCache()` function number.
const BIOS_DISPATCH_A: u16 = 0x00A0;
const FLUSH_CACHE_FN: u16 = 0x0044;

/// Loader stub VA - the head of the verified-dead SCUS arena 1.
pub const STUB_VA: u32 = ARENA1_VA;
/// Words the loader stub occupies.
pub const STUB_WORDS: u32 = 12;
/// Queue-hook trampoline VA, right behind the loader stub.
pub const TRAMPOLINE_VA: u32 = STUB_VA + STUB_WORDS * 4;
/// Words the trampoline occupies.
pub const TRAMPOLINE_WORDS: u32 = 2;
/// One past the last arena byte this feature claims.
pub const ARENA_USED_END_VA: u32 = TRAMPOLINE_VA + TRAMPOLINE_WORDS * 4;

/// Assemble the battle-load stub: read [`BLOCK_SECTORS`] sectors from disc
/// `lba` to [`BLOCK_VA`], `FlushCache()`, replay `displaced`, return to
/// [`LOAD_RET_VA`]. 12 instructions.
///
/// `displaced[1]` rides the return jump's delay slot, so the pair is replayed in
/// program order. Both are plain ALU words (`lui` / `ori`), never a branch.
pub fn assemble_loader_stub(lba: u32, displaced: [u32; 2]) -> Vec<u32> {
    let words = vec![
        addiu(A0, ZERO, BLOCK_SECTORS as u16), // 0:  a0 = sector count
        lui(A1, (lba >> 16) as u16),           // 1:  \ a1 = absolute disc LBA
        ori(A1, A1, (lba & 0xffff) as u16),    // 2:  /
        lui(A2, (BLOCK_VA >> 16) as u16),      // 3:  a2 = dest hi
        jal(LOADER_FN),                        // 4:  FUN_8005E4D4(count, lba, dest)
        ori(A2, A2, (BLOCK_VA & 0xffff) as u16), // 5:  (delay) a2 = 0x801FD000
        addiu(T2, ZERO, BIOS_DISPATCH_A),      // 6:  t2 = 0xA0 (A-table dispatcher)
        jalr(T2),                              // 7:  FlushCache() - the block is code
        addiu(T1, ZERO, FLUSH_CACHE_FN),       // 8:  (delay) t1 = 0x44
        displaced[0],                          // 9:  replay `lui a2,0x1f80`
        j(LOAD_RET_VA),                        // 10: back to battle init
        displaced[1],                          // 11: (delay) replay `ori a2,a2,0x314`
    ];
    debug_assert_eq!(words.len(), STUB_WORDS as usize);
    words
}

/// Assemble the queue-hook trampoline: jump to routine B with the displaced
/// `move t9,a0` in the delay slot. 2 instructions.
pub fn assemble_queue_trampoline(displaced: u32) -> Vec<u32> {
    let words = vec![j(ROUTINE_QUEUE_VA), displaced];
    debug_assert_eq!(words.len(), TRAMPOLINE_WORDS as usize);
    words
}

/// A planned Super Arts Pack injection: the block's annex placement plus every
/// same-size word edit. Nothing is written until [`crate::apply::inject_super_arts_pack`]
/// applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtsPackInjection {
    /// Same-size edits (`None` target = `SCUS_942.54`, `Some(idx)` = PROT entry).
    pub edits: Vec<Edit>,
    /// Absolute disc LBA the block was parked at.
    pub block_lba: u32,
    /// Loader-stub VA (arena 1).
    pub stub_va: u32,
    /// Trampoline VA (arena 1).
    pub trampoline_va: u32,
    /// The fifteen added Super Art names, in table order (character-major).
    pub names: Vec<String>,
}

impl SuperArtsPackInjection {
    /// Plan the injection against a real `SCUS_942.54` and PROT 0898 image, with
    /// the block already parked at `block_lba`.
    ///
    /// Refuses - writing nothing - unless every hook site carries its known US
    /// word, the arena region is all-zero dead space outside every live table,
    /// and the pack's own retail rows match the disc's trigger tables.
    pub fn plan(scus: &[u8], overlay: &[u8], block_lba: u32) -> Result<Self> {
        assert_block_shape()?;
        if block_lba == 0 {
            bail!("super-arts-pack: the block has no disc LBA - refusing to patch");
        }

        // 1. Recognized build: the applier, the queue builder and both banner
        //    routines must carry their known words.
        expect_overlay(overlay, APPLIER_VA, APPLIER_W)?;
        let stride_w = expect_overlay(overlay, HOOK_STRIDE_VA, HOOK_STRIDE_W)?;
        expect_overlay(overlay, HOOK_FIND_HI_VA, HOOK_FIND_HI_W)?;
        expect_overlay(overlay, HOOK_FIND_LO_VA, HOOK_FIND_LO_W)?;
        expect_overlay(overlay, HOOK_REPL_HI_VA, HOOK_REPL_HI_W)?;
        expect_overlay(overlay, HOOK_REPL_LO_VA, HOOK_REPL_LO_W)?;
        expect_overlay(overlay, HOOK_BOUND_VA, HOOK_BOUND_W)?;
        expect_overlay(overlay, HOOK_APPLIER_VA, HOOK_APPLIER_W)?;
        let queue_w = expect_overlay(overlay, HOOK_QUEUE_VA, HOOK_QUEUE_W)?;
        let _ = stride_w;
        expect_scus(scus, HOOK_BANNER_VA, HOOK_BANNER_W)?;
        expect_scus(scus, HOOK_KEEP_NAME_VA, HOOK_KEEP_NAME_W0)?;
        expect_scus(scus, HOOK_KEEP_NAME_VA + 4, HOOK_KEEP_NAME_W1)?;
        let load_w0 = expect_scus(scus, LOAD_HOOK_VA, LOAD_HOOK_W0)?;
        let load_w1 = expect_scus(scus, LOAD_HOOK_VA + 4, LOAD_HOOK_W1)?;

        // 2. The pack's rows 0..4 per character must be this disc's own retail
        //    trigger rows - the pack is a superset, so a mismatch means either a
        //    different build or a different pack.
        check_retail_rows(overlay)?;

        // 3. The arena must be dead space, and not inside a live table.
        let stub = assemble_loader_stub(block_lba, [load_w0, load_w1]);
        let tramp = assemble_queue_trampoline(queue_w);
        if ARENA_USED_END_VA > ARENA1_END_VA {
            bail!(
                "super-arts-pack: the loader stub + trampoline overrun arena 1 \
                 ({ARENA_USED_END_VA:#x} > {ARENA1_END_VA:#x})"
            );
        }
        assert_not_in_tables(
            STUB_VA,
            ARENA_USED_END_VA - STUB_VA,
            SCUS_TABLE_RANGES,
            "loader stub",
        )?;
        let stub_off = scus_off(scus, STUB_VA)?;
        assert_zero(
            scus,
            stub_off,
            (ARENA_USED_END_VA - STUB_VA) as usize,
            STUB_VA,
        )?;
        for (va, len, what) in [
            (HOOK_APPLIER_VA, 4, "applier hook"),
            (HOOK_QUEUE_VA, 4, "queue hook"),
        ] {
            assert_not_in_tables(va, len, OVERLAY_TABLE_RANGES, what)?;
        }

        // 4. The edits.
        let mut edits = Vec::new();
        let ov = |va: u32, words: &[u32]| Edit {
            prot_index: Some(OVERLAY_PROT_INDEX),
            file_off: (va - OVERLAY_BASE_VA) as usize,
            bytes: words_to_bytes(words),
        };
        edits.push(ov(HOOK_STRIDE_VA, &[sll(T5, A1, 1)]));
        edits.push(ov(
            HOOK_FIND_HI_VA,
            &[
                lui(V0, hi_of(FIND_TABLE_VA)),
                addiu(T6, V0, lo_of(FIND_TABLE_VA)),
            ],
        ));
        edits.push(ov(
            HOOK_REPL_HI_VA,
            &[
                lui(V0, hi_of(REPLACE_TABLE_VA)),
                addiu(T8, V0, lo_of(REPLACE_TABLE_VA)),
            ],
        ));
        edits.push(ov(HOOK_BOUND_VA, &[slti(V0, A1, ROWS_PER_CHAR as i16)]));
        edits.push(ov(HOOK_APPLIER_VA, &[j(ROUTINE_APPLIER_VA)]));
        edits.push(ov(HOOK_QUEUE_VA, &[j(TRAMPOLINE_VA)]));

        let sc = |va: u32, words: &[u32]| -> Result<Edit> {
            Ok(Edit {
                prot_index: None,
                file_off: scus_off(scus, va)?,
                bytes: words_to_bytes(words),
            })
        };
        edits.push(sc(HOOK_BANNER_VA, &[j(ROUTINE_BANNER_VA)])?);
        edits.push(sc(HOOK_KEEP_NAME_VA, &[j(ROUTINE_KEEP_NAME_VA), nop()])?);
        edits.push(sc(LOAD_HOOK_VA, &[j(STUB_VA)])?);
        edits.push(sc(STUB_VA, &stub)?);
        edits.push(sc(TRAMPOLINE_VA, &tramp)?);

        Ok(Self {
            edits,
            block_lba,
            stub_va: STUB_VA,
            trampoline_va: TRAMPOLINE_VA,
            names: names(),
        })
    }
}

/// The fifteen added Super Art names, decoded out of the block.
pub fn names() -> Vec<String> {
    let base = (NAME_TABLE_VA - BLOCK_VA) as usize;
    (0..15)
        .map(|i| {
            let row = &BLOCK[base + i * NAME_STRIDE..base + (i + 1) * NAME_STRIDE];
            let end = row.iter().position(|&b| b == 0).unwrap_or(row.len());
            String::from_utf8_lossy(&row[..end]).into_owned()
        })
        .collect()
}

/// The block padded to whole sectors, ready for the annex.
pub fn block_sectors() -> Vec<u8> {
    let mut out = BLOCK.to_vec();
    out.resize(BLOCK_SECTORS as usize * 2048, 0);
    out
}

/// Refuse a block that is not the pack this module was written against: its
/// size, its four routine entries, and every return address baked into it.
/// The return addresses are what tie the block to the hook sites, so checking
/// them here means a swapped-out blob can never be installed against hooks that
/// no longer match it.
fn assert_block_shape() -> Result<()> {
    if BLOCK.len() != 3764 {
        bail!(
            "super-arts-pack: the embedded block is {} bytes, expected 3764",
            BLOCK.len()
        );
    }
    let at = |va: u32| -> u32 {
        let off = (va - BLOCK_VA) as usize;
        u32::from_le_bytes(BLOCK[off..off + 4].try_into().unwrap())
    };
    let checks: [(u32, u32, &str); 8] = [
        (ROUTINE_APPLIER_VA, 0x27BD_FFF8, "routine A prologue"),
        (0x801F_D3D4, j(APPLIER_RET_VA), "routine A return"),
        (ROUTINE_QUEUE_VA, HOOK_QUEUE_W_DISPLACED, "routine B replay"),
        (0x801F_D3F0, j(QUEUE_RET_VA), "routine B return"),
        (ROUTINE_KEEP_NAME_VA, 0x8E03_074C, "routine C prologue"),
        (0x801F_D524, j(KEEP_NAME_RET_VA), "routine C skip return"),
        (ROUTINE_BANNER_VA, HOOK_BANNER_W, "routine D replay"),
        (0x801F_D644, j(BANNER_RET_VA), "routine D return"),
    ];
    for (va, want, what) in checks {
        let got = at(va);
        if got != want {
            bail!(
                "super-arts-pack: block {what} at {va:#x} = {got:#010x}, expected {want:#010x} \
                 - the embedded block is not the pack these hooks were derived from"
            );
        }
    }
    Ok(())
}

/// `sw s3,0x54(sp)` - the queue builder's `s3` spill, replayed at routine B's
/// head (and the word the trampoline design keeps honest).
const HOOK_QUEUE_W_DISPLACED: u32 = 0xAFB3_0054;

/// The pack's rows 0..4 per character must equal the disc's own retail rows.
fn check_retail_rows(overlay: &[u8]) -> Result<()> {
    for ch in 0..3usize {
        for i in 0..RETAIL_ROWS {
            let packed_find = block_row(FIND_TABLE_VA, ch * ROWS_PER_CHAR + i, FIND_STRIDE);
            let disc_find =
                overlay_row(overlay, RETAIL_FIND_VA, ch * RETAIL_ROWS + i, FIND_STRIDE)?;
            let packed_repl = block_row(REPLACE_TABLE_VA, ch * ROWS_PER_CHAR + i, REPLACE_STRIDE);
            let disc_repl = overlay_row(
                overlay,
                RETAIL_REPLACE_VA,
                ch * RETAIL_ROWS + i,
                REPLACE_STRIDE,
            )?;
            if packed_find != disc_find || packed_repl != disc_repl {
                bail!(
                    "super-arts-pack: character {ch} row {i} differs from this disc's own Super \
                     Art trigger table - unrecognized build, nothing written"
                );
            }
        }
    }
    Ok(())
}

fn block_row(table_va: u32, row: usize, stride: usize) -> &'static [u8] {
    let base = (table_va - BLOCK_VA) as usize + row * stride;
    &BLOCK[base..base + stride]
}

fn overlay_row(overlay: &[u8], table_va: u32, row: usize, stride: usize) -> Result<&[u8]> {
    let base = (table_va - OVERLAY_BASE_VA) as usize + row * stride;
    overlay.get(base..base + stride).ok_or_else(|| {
        anyhow::anyhow!("super-arts-pack: PROT 0898 is too short for its own trigger table")
    })
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// `lui` half of a VA whose low half is used as a **signed** `addiu` immediate.
fn hi_of(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
fn lo_of(va: u32) -> u16 {
    (va & 0xffff) as u16
}

fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    legaia_asset::item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("super-arts-pack: can't resolve SCUS VA {va:#x}"))
}

fn expect_scus(scus: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = scus_off(scus, va)?;
    let got = read_word(scus, off)?;
    if got != expect {
        bail!(
            "super-arts-pack: SCUS {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

fn expect_overlay(overlay: &[u8], va: u32, expect: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let got = read_word(overlay, off)
        .with_context(|| format!("super-arts-pack: PROT 0898 too short at {va:#x}"))?;
    if got != expect {
        bail!(
            "super-arts-pack: PROT 0898 {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build - nothing written)"
        );
    }
    Ok(got)
}

/// Refuse if `[va, va+len)` overlaps a known live data table (zero bytes there
/// are still indexed at runtime - "zero is not dead").
fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "super-arts-pack: {what} region {va:#x}..+{len} overlaps live table \
                 {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm `[off, off+len)` in `scus` is all-zero dead space.
fn assert_zero(scus: &[u8], off: usize, len: usize, va: u32) -> Result<()> {
    let region = scus.get(off..off + len).ok_or_else(|| {
        anyhow::anyhow!("super-arts-pack: region {va:#x}..+{len} past end of SCUS")
    })?;
    if region.iter().any(|&b| b != 0) {
        bail!(
            "super-arts-pack: region {va:#x}..+{len} is not all-zero dead space \
             (another injection holds it, or this is a different build) - refusing"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips_sim::Cpu;

    #[test]
    fn block_is_the_pack_the_hooks_were_derived_from() {
        assert_block_shape().unwrap();
        assert_eq!(BLOCK.len(), 3764);
        assert!(BLOCK.len() as u32 <= BLOCK_SECTORS * 2048);
        assert_eq!(block_sectors().len(), 4096);
    }

    #[test]
    fn names_decode() {
        let n = names();
        assert_eq!(n.len(), 15);
        assert_eq!(n[0], "Ultra Elbow");
        assert_eq!(n[14], "Raging Bull");
        assert!(n.iter().all(|s| !s.is_empty() && s.is_ascii()));
    }

    /// The pack's rows 5..9 (ZetaPhoenix's own) are all populated, and every
    /// added replace row ends in a `0x1A` special starter plus at least one
    /// finisher - the shape the applier writes into the action queue.
    #[test]
    fn added_rows_are_well_formed() {
        for ch in 0..3usize {
            for i in RETAIL_ROWS..ROWS_PER_CHAR {
                let f = block_row(FIND_TABLE_VA, ch * ROWS_PER_CHAR + i, FIND_STRIDE);
                let r = block_row(REPLACE_TABLE_VA, ch * ROWS_PER_CHAR + i, REPLACE_STRIDE);
                let len = f[0] as usize;
                assert!((4..=10).contains(&len), "char {ch} row {i} find len {len}");
                let rlen = r.iter().position(|&b| b == 0).unwrap_or(r.len());
                let star = r[..rlen].iter().position(|&b| b == 0x1A);
                let star = star.unwrap_or_else(|| panic!("char {ch} row {i}: no 0x1A starter"));
                assert!(
                    rlen - star >= 2,
                    "char {ch} row {i}: no finisher after 0x1A"
                );
            }
        }
    }

    /// The hit-count seed table is one bit per finisher byte of the matching
    /// added row - `1` for a single hit, `3` for two. This is what routine A
    /// loads at `0x801FD366 + 5*character + (row - 5)`, so it is also a check
    /// that the flat index really is character-major with a stride of five.
    #[test]
    fn hit_seed_table_matches_the_added_rows() {
        let seeds = &BLOCK[(0x801F_D366 - BLOCK_VA) as usize..][..15];
        for ch in 0..3usize {
            for i in RETAIL_ROWS..ROWS_PER_CHAR {
                let r = block_row(REPLACE_TABLE_VA, ch * ROWS_PER_CHAR + i, REPLACE_STRIDE);
                let rlen = r.iter().position(|&b| b == 0).unwrap_or(r.len());
                let star = r[..rlen].iter().position(|&b| b == 0x1A).unwrap();
                let hits = rlen - star - 1;
                let want = (1u32 << hits) - 1;
                let got = seeds[ch * 5 + (i - RETAIL_ROWS)] as u32;
                assert_eq!(got, want, "char {ch} row {i}: {hits} hits");
            }
        }
    }

    /// The doubled `t5` is the whole retarget: the applier computes a
    /// character's find base as `t5*65` and its replace base as `t5*80`, so with
    /// `t5 = 2*character` those become the pack's `10*13` and `10*16` strides,
    /// and routine A's `(t5>>1) + t5 + t5` becomes `5*character`.
    #[test]
    fn doubled_t5_lands_on_the_pack_strides() {
        for ch in 0u32..3 {
            let t5 = 2 * ch;
            assert_eq!(t5 * 65, ch * (ROWS_PER_CHAR * FIND_STRIDE) as u32);
            assert_eq!(t5 * 80, ch * (ROWS_PER_CHAR * REPLACE_STRIDE) as u32);
            assert_eq!((t5 >> 1) + t5 + t5, ch * 5);
        }
    }

    /// Run the assembled `sll t5,a1,1` through the interpreter, for the same
    /// reason: the edit must double `a1`, not shift it somewhere else.
    #[test]
    fn stride_edit_doubles_the_character_index() {
        for ch in 0u32..3 {
            let mut cpu = Cpu::new();
            cpu.r[A1 as usize] = ch;
            cpu.pc = 0x8000_0000;
            cpu.load_words(0x8000_0000, &[sll(T5, A1, 1)]);
            let w = cpu.rd32(cpu.pc);
            cpu.exec(w);
            assert_eq!(cpu.r[T5 as usize], 2 * ch);
        }
    }

    /// The loader stub reads the block to `0x801FD000` and replays both
    /// displaced words before returning - checked by running it, with the CD
    /// reader and the BIOS dispatcher stubbed out as `jr ra`.
    #[test]
    fn loader_stub_calls_the_reader_and_replays() {
        let lba = 0x0003_1234;
        let stub = assemble_loader_stub(lba, [LOAD_HOOK_W0, LOAD_HOOK_W1]);
        let mut cpu = Cpu::new();
        cpu.load_words(STUB_VA, &stub);
        // `FUN_8005E4D4` and the BIOS A-table entry both return immediately.
        cpu.load_words(LOADER_FN, &[crate::mips::jr(crate::mips::RA), nop()]);
        cpu.load_words(0xA0, &[crate::mips::jr(crate::mips::RA), nop()]);
        cpu.pc = STUB_VA;
        cpu.run_until(&[LOAD_RET_VA]);
        assert_eq!(cpu.pc, LOAD_RET_VA, "stub must return to battle init");
        assert_eq!(cpu.r[A0 as usize], BLOCK_SECTORS, "sector count");
        assert_eq!(cpu.r[A1 as usize], lba, "disc LBA");
        assert_eq!(cpu.r[A2 as usize], 0x1F80_0314, "replayed lui+ori (a2)");
    }

    /// The trampoline jumps to routine B with the displaced word in its delay
    /// slot, so `move t9,a0` still happens.
    #[test]
    fn trampoline_replays_move_t9_a0() {
        let tramp = assemble_queue_trampoline(HOOK_QUEUE_W);
        assert_eq!(tramp[0], j(ROUTINE_QUEUE_VA));
        assert_eq!(tramp[1], HOOK_QUEUE_W);
        let mut cpu = Cpu::new();
        cpu.r[A0 as usize] = 0x8009_9000;
        cpu.load_words(TRAMPOLINE_VA, &tramp);
        cpu.pc = TRAMPOLINE_VA;
        cpu.run_until(&[ROUTINE_QUEUE_VA]);
        assert_eq!(cpu.r[25], 0x8009_9000, "t9 = a0");
    }

    /// Every hook target the edits jump to is inside the block, and every return
    /// address the block jumps back to is the instruction after its hook.
    #[test]
    fn hooks_and_returns_pair_up() {
        for va in [
            ROUTINE_APPLIER_VA,
            ROUTINE_QUEUE_VA,
            ROUTINE_KEEP_NAME_VA,
            ROUTINE_BANNER_VA,
        ] {
            assert!((BLOCK_VA..BLOCK_VA + BLOCK.len() as u32).contains(&va));
        }
        assert_eq!(APPLIER_RET_VA, HOOK_APPLIER_VA + 8, "delay slot kept");
        assert_eq!(QUEUE_RET_VA, HOOK_QUEUE_VA + 8);
        assert_eq!(BANNER_RET_VA, HOOK_BANNER_VA + 8);
        assert_eq!(KEEP_NAME_RET_VA, HOOK_KEEP_NAME_VA + 8);
    }

    /// The arena claim fits arena 1 and stays clear of every live SCUS table.
    #[test]
    fn arena_claim_fits() {
        const { assert!(ARENA_USED_END_VA <= ARENA1_END_VA) };
        assert_eq!(
            ARENA_USED_END_VA - STUB_VA,
            (STUB_WORDS + TRAMPOLINE_WORDS) * 4
        );
        assert!(
            assert_not_in_tables(
                STUB_VA,
                ARENA_USED_END_VA - STUB_VA,
                SCUS_TABLE_RANGES,
                "stub"
            )
            .is_ok()
        );
        // Both routine VAs are 4-byte aligned: a `j` drops the low two bits.
        for va in [
            STUB_VA,
            TRAMPOLINE_VA,
            ROUTINE_APPLIER_VA,
            ROUTINE_BANNER_VA,
        ] {
            assert_eq!(va % 4, 0);
        }
    }

    /// The retargeted `lui`/`addiu` pairs really address the pack's tables.
    #[test]
    fn table_retargets_resolve() {
        for (va, reg) in [(FIND_TABLE_VA, T6), (REPLACE_TABLE_VA, T8)] {
            let mut cpu = Cpu::new();
            cpu.pc = 0x8000_0000;
            cpu.load_words(
                0x8000_0000,
                &[lui(V0, hi_of(va)), addiu(reg, V0, lo_of(va))],
            );
            for _ in 0..2 {
                let w = cpu.rd32(cpu.pc);
                cpu.exec(w);
            }
            assert_eq!(cpu.r[reg as usize], va);
        }
    }

    /// The row-loop bound edit widens the applier from five rows to ten.
    #[test]
    fn bound_edit_is_ten_rows() {
        assert_eq!(slti(V0, A1, ROWS_PER_CHAR as i16), 0x28A2_000A);
        assert_eq!(HOOK_BOUND_W, 0x28A2_0005, "retail bound is five");
    }

    // --- Runtime oracle (disc-gated) -----------------------------------------
    //
    // The static checks above say the right bytes are written. These two run
    // the **patched retail applier** - real `FUN_801EF9E4` instructions off the
    // patched disc, with ZetaPhoenix's block at `0x801FD000` read back out of
    // the annex - over a live action queue, and check what comes out. That is
    // the only check here that answers "does the pack actually fire".

    fn disc_bytes() -> Option<Vec<u8>> {
        let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
        p.is_file().then(|| std::fs::read(&p).ok()).flatten()
    }

    /// Actor pointer table `(&DAT_801C9370)[slot]` the applier indexes.
    const ACTOR_TABLE_VA: u32 = 0x801C_9370;
    /// Action queue offset inside an actor record.
    const QUEUE_OFF: u32 = 0x1DF;
    /// Where the harness parks its fake actor and stack.
    const ACTOR_VA: u32 = 0x8009_0000;
    const STACK_VA: u32 = 0x801F_F000;
    /// `jr ra` lands here, ending the run.
    const RETURN_SENTINEL: u32 = 0x0BAD_0000;

    /// Build a CPU with the patched applier, the annexed block and one actor
    /// whose queue holds `queue`, then run `FUN_801EF9E4(slot=0, character)`.
    fn run_applier(overlay: &[u8], block: &[u8], character: u32, queue: &[u8]) -> Cpu {
        let mut cpu = Cpu::new();
        // The applier's own code, straight off the patched overlay entry.
        let lo = (APPLIER_VA - OVERLAY_BASE_VA) as usize;
        let hi = (0x801E_FC00 - OVERLAY_BASE_VA) as usize;
        cpu.load(APPLIER_VA, &overlay[lo..hi]);
        // ZetaPhoenix's block, as the injected stub will have loaded it.
        cpu.load(BLOCK_VA, block);
        // One actor, reachable through the pointer table.
        cpu.wr32(ACTOR_TABLE_VA, ACTOR_VA);
        for (i, &b) in queue.iter().enumerate() {
            cpu.wr8(ACTOR_VA + QUEUE_OFF + i as u32, b);
        }
        cpu.r[4] = 0; // a0 = actor slot
        cpu.r[5] = character; // a1 = character
        cpu.r[29] = STACK_VA; // sp
        cpu.r[31] = RETURN_SENTINEL; // ra
        cpu.pc = APPLIER_VA;
        cpu.run_until(&[RETURN_SENTINEL]);
        cpu
    }

    fn read_queue(cpu: &Cpu) -> Vec<u8> {
        (0..16).map(|i| cpu.rd8(ACTOR_VA + QUEUE_OFF + i)).collect()
    }

    fn patched_images() -> Option<(Vec<u8>, Vec<u8>)> {
        let disc = disc_bytes()?;
        let mut patcher = crate::disc::DiscPatcher::open(disc).expect("open disc");
        let report = crate::apply::inject_super_arts_pack(&mut patcher).expect("inject");
        let overlay = patcher.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");
        let block = patcher
            .read_disc_sectors(report.block_lba, report.block_sectors)
            .expect("read the annexed block");
        Some((overlay, block))
    }

    /// Every one of ZetaPhoenix's fifteen added chains, run through the patched
    /// applier: the queue tail must become that row's replace string, and the
    /// block's "which added Super" cell must hold the flat index
    /// `5*character + (row - 5)` - the index its name and hit seed are read by.
    #[test]
    fn patched_applier_fires_every_added_super_art() {
        let Some((overlay, block)) = patched_images() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        for ch in 0..3usize {
            for row in RETAIL_ROWS..ROWS_PER_CHAR {
                let f = block_row(FIND_TABLE_VA, ch * ROWS_PER_CHAR + row, FIND_STRIDE);
                let r = block_row(REPLACE_TABLE_VA, ch * ROWS_PER_CHAR + row, REPLACE_STRIDE);
                let find = &f[1..1 + f[0] as usize];
                let rlen = r.iter().position(|&b| b == 0).unwrap_or(r.len());
                let replace = &r[..rlen];

                let cpu = run_applier(&overlay, &block, ch as u32, find);
                let queue = read_queue(&cpu);
                assert_eq!(
                    &queue[..replace.len()],
                    replace,
                    "character {ch} row {row}: the queue must become the replace string"
                );
                let index = cpu.rd8(0x801F_D378);
                assert_eq!(
                    index as usize,
                    ch * 5 + (row - RETAIL_ROWS),
                    "character {ch} row {row}: wrong added-Super index (so wrong name)"
                );
                assert_ne!(
                    cpu.rd16(0x801F_D376),
                    0,
                    "character {ch} row {row}: the hit bit-train must be built"
                );
            }
        }
    }

    /// The retail five per character still fire, unchanged, and routine A
    /// returns early for them - so the name banner keeps whatever retail put
    /// there instead of one of the pack's names.
    #[test]
    fn patched_applier_leaves_the_retail_super_arts_alone() {
        let Some((overlay, block)) = patched_images() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        for ch in 0..3usize {
            for row in 0..RETAIL_ROWS {
                let f = block_row(FIND_TABLE_VA, ch * ROWS_PER_CHAR + row, FIND_STRIDE);
                let r = block_row(REPLACE_TABLE_VA, ch * ROWS_PER_CHAR + row, REPLACE_STRIDE);
                let find = &f[1..1 + f[0] as usize];
                let rlen = r.iter().position(|&b| b == 0).unwrap_or(r.len());

                let mut cpu = run_applier(&overlay, &block, ch as u32, find);
                let queue = read_queue(&cpu);
                assert_eq!(
                    &queue[..rlen],
                    &r[..rlen],
                    "character {ch} retail row {row}: retail Super Art must still fire"
                );
                // Routine A's gate is `row < 5`, so neither runtime cell moves.
                assert_eq!(
                    cpu.rd8(0x801F_D378),
                    0,
                    "retail row {row} set the index cell"
                );
                assert_eq!(
                    cpu.rd16(0x801F_D376),
                    0,
                    "retail row {row} set the bit train"
                );
                cpu.steps = 0;
            }
        }
    }

    /// A queue that matches nothing is left exactly as it was.
    #[test]
    fn patched_applier_ignores_an_unmatched_queue() {
        let Some((overlay, block)) = patched_images() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let queue = [0x19u8, 0x01, 0x19, 0x02];
        let cpu = run_applier(&overlay, &block, 0, &queue);
        assert_eq!(&read_queue(&cpu)[..queue.len()], &queue[..]);
        assert_eq!(cpu.rd8(0x801F_D378), 0);
    }
}
