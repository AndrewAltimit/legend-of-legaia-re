//! **Conkram mirror freeze fix**: make the move-VM parts' two heap
//! allocations - the keyframe-pose block (op `0x3C`) and the `0x9C`-byte
//! render node (`FUN_80020F88`) - survive a failure without writing through
//! NULL, and without leaving the `PRG ERR` readout on screen.
//!
//! ## The freeze
//!
//! In present-day Conkram's Room of Mirrors (`concnow` `P2[18]`) the heap
//! runs with about 16.6 KB free; on an emulated CPU faster than a console's,
//! the mirror effect keeps more parts alive while the music poller
//! `FUN_800243F0` pre-stages `PLAYER1..3`, and allocations fail. Two callers
//! then write through NULL:
//!
//! - op `0x3C` of the move VM `FUN_80023070` (jump-table entry `0x80023CA4`)
//!   stores its pose block with no test and copies a four-part pose over
//!   `0x80000028..0x8000008C`, across the exception vector at `0x80000080` -
//!   the freeze;
//! - `FUN_80020F88` tests its render-node allocation but still stores the
//!   NULL at `actor+0x44` (`sw` in the `bnez` delay slot at `0x80021190`),
//!   and the node's users write through it: the spawner's count store
//!   (`0x80021DC4`), op `0x3C`'s count store (`0x80023CB0`) and the draw
//!   path's slot pointers (`0x8001B098`), which reach `4 + 4 * count`.
//!
//! Every failure also adds `0x20000` to the `PRG ERR` counter `0x8007B828`
//! (`gp+0x510`) in the malloc wrapper's failure arm (`0x800178B8`), and the
//! render-node path ORs `0x4000` into the same word; the per-frame reporter
//! prints the counter whenever it is non-zero and nothing ever clears it.
//!
//! ## The fix: four same-size edits in `SCUS_942.54`
//!
//! 1. **A quiet malloc entry.** The unreferenced bare thunk `0x80025B10`
//!    becomes the wrapper without its failure arm: `gp+0x488 += 1` (the
//!    wrapper's outstanding-allocation count, which `free` decrements), then a
//!    tail jump into the allocator `FUN_8002B468`, which returns to the
//!    caller. On success it leaves exactly the wrapper's state; on failure it
//!    neither bumps `PRG ERR` nor logs.
//! 2. **Op `0x3C`** (`0x80023CA4..0x80023CE0`). The load-delay `nop` takes the
//!    independent `sh zero,0x22(s2)`, the five instructions after it move up
//!    one word, the `jal` goes to the quiet entry, and the freed word after it
//!    becomes `beq v0,zero,0x800232B8` - op `0x08`'s HALT arm (`a2 = 0`,
//!    `s6 = 0`, `+0x10 |= 8`), so a failed pose block ends the part. The list
//!    walk `FUN_8002519C` releases it a pass later, freeing its node and
//!    skipping the NULL `+0x4C`.
//! 3. **The render-node call** (`0x80021184`): `jal` to the quiet entry.
//! 4. **The render-node failure block** (`0x80021194..0x800211B0`). Instead of
//!    leaving NULL at `+0x44` and ORing `0x4000` into `PRG ERR`, it stores a
//!    shared stand-in node at [`STANDIN_NODE_VA`] (with `+0x9A = -1`, as the
//!    success path seeds a fresh node), then keeps retail's `+0x56 = 0` and
//!    `return -1`. The actor flag `0x800` stays clear, and every `free` of
//!    `+0x44` on the disc (the list walk at `0x80025200` and its copies in
//!    the field, dance and fishing overlays) is gated on that flag, so the
//!    stand-in is never freed; a later call of `FUN_80020F88` on the actor
//!    retries the allocation, as retail does.
//!
//! The stand-in lives at the bottom of the free RAM above the largest slot-B
//! image: `0x801FA9D8..0x801FE000` reads all-zero in every library state, no
//! instruction in any image forms an address there, and the deepest stack use
//! stops at `0x801FE420` (`docs/reference/memory-map.md`). The Super Arts Pack
//! block at `0x801FD000` is the only other tenant of that band.
//!
//! Every allocation that succeeds runs retail's instructions in retail's
//! order (apart from the one reordered store in op `0x3C`, to unrelated
//! memory) and leaves the same state; only the wrapper's stack frame, dead
//! memory below `sp`, is no longer written. No branch, jump or address word
//! in any image targets the rewritten ranges (five-form scan), and
//! `0x80025B10` has no reference of any form.
//!
//! Each site is verified against its stock words before writing; an
//! unrecognized or partly patched build is refused and a fully fixed image is
//! a no-op. The stock words below are cited from the project's own
//! disassembly reference; no game bytes are embedded.

use anyhow::{Result, anyhow, bail};
use legaia_game_tables::item_names;

use crate::arenas::Edit;
use crate::mips::{
    A0, A1, A2, AT, GP, S0, S2, S3, V0, V1, ZERO, addiu, addu, beq, bne, j, jal, lh, lui, lw, nop,
    ori, sh, sll, sw,
};

/// First word of the op-`0x3C` window: the handler entry.
pub const WINDOW_VA: u32 = 0x8002_3CA4;
/// The malloc wrapper retail calls (`FUN_80017888`).
pub const MALLOC_VA: u32 = 0x8001_7888;
/// The allocator the wrapper calls (`FUN_8002B468`).
pub const ALLOC_VA: u32 = 0x8002_B468;
/// The quiet malloc entry, over the unreferenced thunk at `0x80025B10`.
pub const QUIET_MALLOC_VA: u32 = 0x8002_5B10;
/// Op `0x08`'s HALT arm in the move VM.
pub const HALT_ARM_VA: u32 = 0x8002_32B8;
/// The move VM's common PC-advance tail.
pub const VM_TAIL_VA: u32 = 0x8002_4150;
/// The instruction after the window: the copy's `li a2,0x2`.
pub const COPY_VA: u32 = 0x8002_3CE4;
/// The move VM's opcode jump table.
pub const JUMP_TABLE_VA: u32 = 0x8001_0778;
/// `FUN_80020F88`'s render-node `jal`.
pub const NODE_CALL_VA: u32 = 0x8002_1184;
/// `FUN_80020F88`'s render-node failure block.
pub const NODE_FAIL_VA: u32 = 0x8002_1194;
/// `FUN_80020F88`'s success continuation (the `bnez` target).
pub const NODE_OK_VA: u32 = 0x8002_11B4;
/// `FUN_80020F88`'s epilogue.
pub const NODE_EPILOGUE_VA: u32 = 0x8002_1238;
/// The shared stand-in render node (free RAM above slot B).
pub const STANDIN_NODE_VA: u32 = 0x801F_AA00;
/// Size of a render node (`FUN_80017888(0, 0x9C)`).
pub const NODE_SIZE: u32 = 0x9C;
/// `gp` in `SCUS_942.54`.
pub const GP_VALUE: u32 = 0x8007_B318;
/// The `PRG ERR` counter (`gp+0x510`).
pub const PRG_ERR_VA: u32 = 0x8007_B828;

// The stand-in sits in the free band above slot B, below the Super Arts Pack
// block at 0x801FD000, word-aligned.
const _: () = assert!(STANDIN_NODE_VA >= 0x801F_A9D8);
const _: () = assert!(STANDIN_NODE_VA + NODE_SIZE <= 0x801F_D000);
const _: () = assert!(STANDIN_NODE_VA.is_multiple_of(4));

/// The sixteen stock words of `0x80023CA4..0x80023CE4`.
pub const STOCK_WINDOW: [u32; 16] = [
    0x8613_0002, // lh s3,0x2(s0)
    0x8E42_0044, // lw v0,0x44(s2)
    0x0000_0000, // nop
    0xAC53_0000, // sw s3,0x0(v0)
    0x8E43_004C, // lw v1,0x4c(s2)
    0x2402_0006, // li v0,0x6
    0xA642_005A, // sh v0,0x5a(s2)
    0xA640_0022, // sh zero,0x22(s2)
    0xA640_0068, // sh zero,0x68(s2)
    0x1460_0006, // bne v1,zero,0x80023ce4
    0xA640_005C, // _sh zero,0x5c(s2)
    0x0000_2021, // clear a0
    0x0013_2940, // sll a1,s3,0x5
    0x0C00_5E22, // jal 0x80017888
    0x34A5_0008, // _ori a1,a1,0x8
    0xAE42_004C, // sw v0,0x4c(s2)
];

/// The HALT arm (`0x800232B8..0x800232D0`) op `0x3C` branches to.
pub const HALT_ARM: [u32; 6] = [
    0x0000_3021, // move a2,zero
    0x8E42_0010, // lw v0,0x10(s2)
    0x00C0_B021, // move s6,a2
    0x3442_0008, // ori v0,v0,0x8
    0x0800_9054, // j 0x80024150
    0xAE42_0010, // _sw v0,0x10(s2)
];

/// The stock render-node call (`0x80021184`).
pub const STOCK_NODE_CALL: u32 = 0x0C00_5E22; // jal 0x80017888

/// The stock render-node failure block (`0x80021194..0x800211B4`).
pub const STOCK_NODE_FAIL: [u32; 8] = [
    0x3C03_8008, // lui v1,0x8008
    0x8C63_B828, // lw v1,-0x47d8(v1)     PRG ERR
    0xA600_0056, // sh zero,0x56(s0)
    0x3463_4000, // ori v1,v1,0x4000
    0x3C01_8008, // lui at,0x8008
    0xAC23_B828, // sw v1,-0x47d8(at)     PRG ERR |= 0x4000
    0x0800_848E, // j 0x80021238
    0x2402_FFFF, // _li v0,-1
];

/// The stock words around the failure block that pin it in place.
const NODE_CONTEXT: [(u32, u32); 3] = [
    (0x8002_1188, 0x2405_009C), // _li a1,0x9c
    (0x8002_118C, 0x1440_0009), // bnez v0,0x800211b4
    (0x8002_1190, 0xAE02_0044), // _sw v0,0x44(s0)
];

/// The stock unreferenced thunk at `0x80025B10` (frame, `jal 0x8002B688`,
/// epilogue).
pub const STOCK_THUNK: [u32; 8] = [
    0x27BD_FFE8, // addiu sp,sp,-0x18
    0xAFBF_0010, // sw ra,0x10(sp)
    0x0C00_ADA2, // jal 0x8002b688
    0x0000_0000, // nop
    0x8FBF_0010, // lw ra,0x10(sp)
    0x0000_0000, // nop
    0x03E0_0008, // jr ra
    0x27BD_0018, // _addiu sp,sp,0x18
];

/// Signed word offset of a branch at `from` to `to`.
fn branch_off(from: u32, to: u32) -> i16 {
    let delta = (to as i64 - (from as i64 + 4)) / 4;
    i16::try_from(delta).expect("branch target in range")
}

/// Assemble the replacement op-`0x3C` window (sixteen words, same size).
pub fn assemble_window() -> Vec<u32> {
    let va = |i: u32| WINDOW_VA + i * 4;
    let words = vec![
        lh(S3, S0, 0x2),                                // 0  count = op[1]
        lw(V0, S2, 0x44),                               // 1  model list
        sh(ZERO, S2, 0x22),                             // 2  (fills the v0 load delay)
        sw(S3, V0, 0x0),                                // 3  count word
        lw(V1, S2, 0x4C),                               // 4  pose block
        addiu(V0, ZERO, 0x6),                           // 5  li v0,6
        sh(V0, S2, 0x5A),                               // 6  render mode 6
        sh(ZERO, S2, 0x68),                             // 7
        bne(V1, ZERO, branch_off(va(8), COPY_VA)),      // 8  block exists -> copy
        sh(ZERO, S2, 0x5C),                             // 9  (delay)
        addu(A0, ZERO, ZERO),                           // 10 clear a0
        sll(A1, S3, 5),                                 // 11
        jal(QUIET_MALLOC_VA),                           // 12
        ori(A1, A1, 0x8),                               // 13 (delay)
        beq(V0, ZERO, branch_off(va(14), HALT_ARM_VA)), // 14 NULL -> HALT arm
        sw(V0, S2, 0x4C),                               // 15 (delay, both paths)
    ];
    debug_assert_eq!(words.len(), STOCK_WINDOW.len());
    words
}

/// Assemble the quiet malloc entry (eight words over the thunk: five of
/// code, three of padding).
pub fn assemble_quiet_malloc() -> Vec<u32> {
    vec![
        lw(V0, GP, 0x488), // outstanding-allocation count
        nop(),             // (load delay)
        addiu(V0, V0, 1),  //
        j(ALLOC_VA),       // tail call: the allocator returns to our caller
        sw(V0, GP, 0x488), // (delay)
        nop(),
        nop(),
        nop(),
    ]
}

/// Assemble the replacement render-node failure block (eight words).
pub fn assemble_node_fail() -> Vec<u32> {
    let lo = STANDIN_NODE_VA as u16;
    let hi = ((STANDIN_NODE_VA >> 16) + u32::from(lo >= 0x8000)) as u16;
    vec![
        lui(AT, hi),             // \
        addiu(AT, AT, lo),       // / at = stand-in node
        addiu(V1, ZERO, 0xFFFF), // v1 = -1
        sh(V1, AT, 0x9A),        // node+0x9A = -1, as a fresh node
        sw(AT, S0, 0x44),        // +0x44 = stand-in (flag 0x800 stays clear)
        sh(ZERO, S0, 0x56),      // retail: draw kind 0
        j(NODE_EPILOGUE_VA),     // retail: return
        addiu(V0, ZERO, 0xFFFF), // (delay) retail: -1
    ]
}

fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow!("conkram-freeze-fix: can't resolve SCUS VA {va:#x}"))
}

fn word_at(scus: &[u8], va: u32) -> Result<u32> {
    let off = scus_off(scus, va)?;
    let b = scus
        .get(off..off + 4)
        .ok_or_else(|| anyhow!("conkram-freeze-fix: SCUS_942.54 too short at {va:#x}"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

fn words_at(scus: &[u8], va: u32, n: usize) -> Result<Vec<u32>> {
    (0..n).map(|i| word_at(scus, va + i as u32 * 4)).collect()
}

/// One patched site: its VA, stock words and fixed words.
struct Site {
    va: u32,
    what: &'static str,
    stock: Vec<u32>,
    fixed: Vec<u32>,
}

fn sites() -> Vec<Site> {
    vec![
        Site {
            va: QUIET_MALLOC_VA,
            what: "quiet malloc entry (unreferenced thunk)",
            stock: STOCK_THUNK.to_vec(),
            fixed: assemble_quiet_malloc(),
        },
        Site {
            va: WINDOW_VA,
            what: "op 0x3C handler",
            stock: STOCK_WINDOW.to_vec(),
            fixed: assemble_window(),
        },
        Site {
            va: NODE_CALL_VA,
            what: "render-node malloc call",
            stock: vec![STOCK_NODE_CALL],
            fixed: vec![jal(QUIET_MALLOC_VA)],
        },
        Site {
            va: NODE_FAIL_VA,
            what: "render-node failure block",
            stock: STOCK_NODE_FAIL.to_vec(),
            fixed: assemble_node_fail(),
        },
    ]
}

/// The planned fix: same-size edits in `SCUS_942.54`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkramFreezeFix {
    /// One edit per site, in the order [`sites`] lists them.
    pub edits: Vec<Edit>,
}

/// The VAs and word counts of the patched sites.
pub fn site_spans() -> Vec<(u32, usize)> {
    sites().iter().map(|s| (s.va, s.fixed.len())).collect()
}

/// Plan the fix against the raw `SCUS_942.54` bytes.
///
/// Returns `Ok(None)` when every site already holds the fix. Fails, writing
/// nothing, when any site is neither stock nor fixed, when the sites are a
/// mix of both, or when the jump table, the HALT arm, the copy entry or the
/// words around the render-node block do not read as the documented
/// disassembly.
pub fn plan(scus: &[u8]) -> Result<Option<ConkramFreezeFix>> {
    let mut context = vec![
        (
            JUMP_TABLE_VA + 0x3C * 4,
            WINDOW_VA,
            "op 0x3C jump-table entry",
        ),
        (
            JUMP_TABLE_VA + 0x08 * 4,
            HALT_ARM_VA,
            "op 0x08 jump-table entry",
        ),
        (COPY_VA, addiu(A2, ZERO, 0x2), "copy entry li a2,2"),
    ];
    for (i, w) in HALT_ARM.iter().enumerate() {
        context.push((HALT_ARM_VA + i as u32 * 4, *w, "HALT arm"));
    }
    for (va, w) in NODE_CONTEXT {
        context.push((va, w, "render-node call context"));
    }
    for (va, want, what) in context {
        let got = word_at(scus, va)?;
        if got != want {
            bail!(
                "conkram-freeze-fix: {what} at {va:#x} = {got:#010x}, expected {want:#010x} \
                 (unrecognized build) - refusing to patch"
            );
        }
    }

    let mut edits = Vec::new();
    let mut fixed_count = 0;
    let all = sites();
    for s in &all {
        let current = words_at(scus, s.va, s.stock.len())?;
        if current == s.fixed {
            fixed_count += 1;
        } else if current == s.stock {
            edits.push(Edit {
                prot_index: None,
                file_off: scus_off(scus, s.va)?,
                bytes: s.fixed.iter().flat_map(|w| w.to_le_bytes()).collect(),
            });
        } else {
            bail!(
                "conkram-freeze-fix: {} at {:#x} is neither the stock code nor the fix \
                 (first divergence at word {}) - unrecognized build, refusing to patch",
                s.what,
                s.va,
                current
                    .iter()
                    .zip(s.stock.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(0),
            );
        }
    }
    if fixed_count == all.len() {
        return Ok(None);
    }
    if fixed_count != 0 {
        bail!(
            "conkram-freeze-fix: {fixed_count} of {} sites already hold the fix - a partly \
             patched image, refusing to patch",
            all.len()
        );
    }
    Ok(Some(ConkramFreezeFix { edits }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips::{RA, S6, jr};
    use crate::mips_sim::Cpu;

    fn in_low_ram(a: u32) -> bool {
        a < 0x100 || (0x8000_0000..0x8000_0100).contains(&a)
    }

    #[test]
    fn stock_constants_match_the_encoders() {
        let s = STOCK_WINDOW;
        assert_eq!(s[0], lh(S3, S0, 0x2));
        assert_eq!(s[1], lw(V0, S2, 0x44));
        assert_eq!(s[2], nop());
        assert_eq!(s[3], sw(S3, V0, 0x0));
        assert_eq!(s[4], lw(V1, S2, 0x4C));
        assert_eq!(s[5], addiu(V0, ZERO, 0x6));
        assert_eq!(s[6], sh(V0, S2, 0x5A));
        assert_eq!(s[7], sh(ZERO, S2, 0x22));
        assert_eq!(s[8], sh(ZERO, S2, 0x68));
        assert_eq!(s[9], bne(V1, ZERO, 6));
        assert_eq!(s[10], sh(ZERO, S2, 0x5C));
        assert_eq!(s[11], addu(A0, ZERO, ZERO));
        assert_eq!(s[12], sll(A1, S3, 5));
        assert_eq!(s[13], jal(MALLOC_VA));
        assert_eq!(s[14], ori(A1, A1, 0x8));
        assert_eq!(s[15], sw(V0, S2, 0x4C));
        let halt = [
            addu(A2, ZERO, ZERO),
            lw(V0, S2, 0x10),
            addu(S6, A2, ZERO),
            ori(V0, V0, 0x8),
            j(VM_TAIL_VA),
            sw(V0, S2, 0x10),
        ];
        assert_eq!(HALT_ARM, halt);
        assert_eq!(STOCK_NODE_CALL, jal(MALLOC_VA));
        let nf = STOCK_NODE_FAIL;
        assert_eq!(nf[0], lui(V1, 0x8008));
        assert_eq!(nf[1], lw(V1, V1, 0xB828));
        assert_eq!(nf[2], sh(ZERO, S0, 0x56));
        assert_eq!(nf[3], ori(V1, V1, 0x4000));
        assert_eq!(nf[4], lui(AT, 0x8008));
        assert_eq!(nf[5], sw(V1, AT, 0xB828));
        assert_eq!(nf[6], j(NODE_EPILOGUE_VA));
        assert_eq!(nf[7], addiu(V0, ZERO, 0xFFFF));
        assert_eq!(STOCK_THUNK[2], jal(0x8002_B688));
        assert_eq!(STOCK_THUNK[6], jr(RA));
        assert_eq!(GP_VALUE + 0x510, PRG_ERR_VA);
    }

    #[test]
    fn window_changes_nine_words_and_keeps_the_rest() {
        let g = assemble_window();
        let changed: Vec<usize> = (0..16).filter(|&i| g[i] != STOCK_WINDOW[i]).collect();
        assert_eq!(changed, vec![2, 7, 8, 9, 10, 11, 12, 13, 14]);
        for i in [0, 1, 3, 4, 5, 6, 15] {
            assert_eq!(g[i], STOCK_WINDOW[i], "word {i}");
        }
    }

    #[test]
    fn branches_land_on_the_copy_and_the_halt_arm() {
        let g = assemble_window();
        let target = |i: usize| {
            let off = (g[i] & 0xFFFF) as i16 as i32;
            (WINDOW_VA as i64 + (i as i64 + 1) * 4 + off as i64 * 4) as u32
        };
        assert_eq!(g[8] >> 26, 0x05, "bne");
        assert_eq!(target(8), COPY_VA);
        assert_eq!(g[14] >> 26, 0x04, "beq");
        assert_eq!(target(14), HALT_ARM_VA);
    }

    #[test]
    fn fix_respects_r3000_load_delays() {
        let g = assemble_window();
        // lw v0 at 1: word 2 does not read v0.
        assert_eq!(g[2], sh(ZERO, S2, 0x22));
        // lw v1 at 4: words 5..7 do not read v1; the bne at 8 does.
        for w in &g[5..8] {
            let rs = (w >> 21) & 31;
            let rt = (w >> 16) & 31;
            assert!(rs != V1 && rt != V1);
        }
        // Quiet malloc: the load of v0 is followed by a nop.
        let q = assemble_quiet_malloc();
        assert_eq!(q[1], nop());
    }

    // ---- simulated machines ---------------------------------------------

    /// Malloc stubs: the wrapper models retail's (count, alloc, and on NULL
    /// `PRG ERR += 0x20000`); the allocator stub returns `ret`.
    fn load_alloc(c: &mut Cpu, ret: u32, quiet: bool) {
        c.r[GP as usize] = GP_VALUE;
        // Allocator stub at ALLOC_VA: v0 = ret; jr ra.
        c.load_words(
            ALLOC_VA,
            &[lui(V0, (ret >> 16) as u16), jr(RA), ori(V0, V0, ret as u16)],
        );
        if quiet {
            c.load_words(QUIET_MALLOC_VA, &assemble_quiet_malloc());
        }
        // Retail wrapper model at MALLOC_VA (frameless: ra kept in t9).
        let t9 = 25;
        c.load_words(
            MALLOC_VA,
            &[
                lw(V0, GP, 0x488),
                nop(),
                addiu(V0, V0, 1),
                sw(V0, GP, 0x488),
                addu(t9, RA, ZERO),
                jal(ALLOC_VA),
                nop(),
                addu(RA, t9, ZERO),
                bne(V0, ZERO, 5),
                nop(),
                lw(V1, GP, 0x510),
                lui(AT, 2),
                addu(V1, V1, AT),
                sw(V1, GP, 0x510),
                jr(RA),
                nop(),
            ],
        );
    }

    /// The op-`0x3C` window, the HALT arm, an actor at `0x80100000` with its
    /// node at `0x80101000`, and an op stream with four parts.
    fn machine(window: &[u32], ret: u32, block: u32, quiet: bool) -> Cpu {
        let mut c = Cpu::new();
        c.load_words(WINDOW_VA, window);
        c.load_words(HALT_ARM_VA, &HALT_ARM);
        load_alloc(&mut c, ret, quiet);
        let actor = 0x8010_0000;
        let ops = 0x8010_2000;
        c.wr32(actor + 0x44, 0x8010_1000);
        c.wr32(actor + 0x4C, block);
        c.wr32(actor + 0x10, 0x0000_0800);
        c.wr16(actor + 0x22, 0x1234);
        c.wr16(actor + 0x5A, 0x0002);
        c.wr16(ops, 0x3C);
        c.wr16(ops + 2, 4);
        c.r[S2 as usize] = actor;
        c.r[S0 as usize] = ops;
        c.r[S6 as usize] = 1;
        c.r[A2 as usize] = 0xDEAD;
        c.pc = WINDOW_VA;
        c
    }

    fn run(window: &[u32], ret: u32, block: u32, quiet: bool) -> Cpu {
        let mut c = machine(window, ret, block, quiet);
        c.run_until(&[COPY_VA, VM_TAIL_VA]);
        c
    }

    fn data(c: &Cpu) -> Vec<(u32, u8)> {
        let mut m: Vec<(u32, u8)> = c
            .mem
            .iter()
            .filter(|(a, _)| (0x8007_0000..0x8008_0000).contains(*a) || **a >= 0x8010_0000)
            .map(|(a, v)| (*a, *v))
            .collect();
        m.sort_unstable();
        m
    }

    #[test]
    fn success_path_matches_retail() {
        let fixed = assemble_window();
        for (ret, block) in [(0x8015_0000, 0), (0, 0x8016_0000)] {
            let a = run(&STOCK_WINDOW, ret, block, false);
            let b = run(&fixed, ret, block, true);
            assert_eq!(a.pc, COPY_VA);
            assert_eq!(b.pc, COPY_VA);
            assert_eq!(data(&a), data(&b), "data memory after the seat");
            for r in [S0, S2, S3, S6, V0, V1, A0, A1] {
                assert_eq!(a.r[r as usize], b.r[r as usize], "reg {r}");
            }
        }
    }

    #[test]
    fn null_pose_block_halts_the_part_quietly() {
        let c = run(&assemble_window(), 0, 0, true);
        let actor = 0x8010_0000;
        assert_eq!(c.pc, VM_TAIL_VA, "left through the HALT arm");
        assert_eq!(c.r[A2 as usize], 0, "zero PC delta");
        assert_eq!(c.r[S6 as usize], 0, "VM loop ends");
        assert_eq!(c.rd32(actor + 0x10), 0x0000_0808, "halt bit set");
        assert_eq!(c.rd32(actor + 0x4C), 0, "pose block stays NULL");
        assert_eq!(c.rd32(PRG_ERR_VA), 0, "PRG ERR untouched");
        assert_eq!(c.rd32(GP_VALUE + 0x488), 1, "allocation counted as retail");
        assert!(!c.mem.keys().any(|&a| in_low_ram(a)));
    }

    #[test]
    fn stock_null_pose_block_reaches_the_copy_and_bumps_prg_err() {
        let c = run(&STOCK_WINDOW, 0, 0, false);
        assert_eq!(c.pc, COPY_VA, "the defect: retail falls into the copy");
        assert_eq!(c.rd32(PRG_ERR_VA), 0x20000);
    }

    /// `FUN_80020F88` from its render-node call to either continuation.
    fn node_machine(fixed: bool, ret: u32) -> Cpu {
        let mut c = Cpu::new();
        load_alloc(&mut c, ret, fixed);
        let call = if fixed {
            jal(QUIET_MALLOC_VA)
        } else {
            STOCK_NODE_CALL
        };
        let block = if fixed {
            assemble_node_fail()
        } else {
            STOCK_NODE_FAIL.to_vec()
        };
        let mut words = vec![addu(A0, ZERO, ZERO), call];
        words.extend(NODE_CONTEXT.iter().map(|(_, w)| *w));
        words.extend(block);
        c.load_words(NODE_CALL_VA - 4, &words);
        let actor = 0x8010_0000;
        c.wr32(actor + 0x10, 0x0000_0002);
        c.wr16(actor + 0x56, 0x0004);
        c.r[S0 as usize] = actor;
        c.pc = NODE_CALL_VA - 4;
        c.run_until(&[NODE_OK_VA, NODE_EPILOGUE_VA]);
        c
    }

    #[test]
    fn node_success_path_matches_retail() {
        let a = node_machine(false, 0x8015_0000);
        let b = node_machine(true, 0x8015_0000);
        assert_eq!(a.pc, NODE_OK_VA);
        assert_eq!(b.pc, NODE_OK_VA);
        assert_eq!(data(&a), data(&b));
        assert_eq!(b.rd32(0x8010_0000 + 0x44), 0x8015_0000);
    }

    #[test]
    fn node_failure_installs_the_standin_quietly() {
        let actor = 0x8010_0000;
        let r = node_machine(false, 0);
        assert_eq!(r.pc, NODE_EPILOGUE_VA);
        assert_eq!(r.rd32(actor + 0x44), 0, "retail leaves NULL");
        assert_eq!(r.rd32(PRG_ERR_VA), 0x24000, "retail: 0x20000 + 0x4000");

        let f = node_machine(true, 0);
        assert_eq!(f.pc, NODE_EPILOGUE_VA);
        assert_eq!(f.rd32(actor + 0x44), STANDIN_NODE_VA);
        assert_eq!(f.rd16(STANDIN_NODE_VA + 0x9A), 0xFFFF);
        assert_eq!(f.rd16(actor + 0x56), 0, "draw kind 0 as retail");
        assert_eq!(f.r[V0 as usize], 0xFFFF_FFFF, "returns -1 as retail");
        assert_eq!(
            f.rd32(actor + 0x10) & 0x800,
            0,
            "never freed: flag 0x800 clear"
        );
        assert_eq!(f.rd32(PRG_ERR_VA), 0, "PRG ERR untouched");
        assert_eq!(f.rd32(GP_VALUE + 0x488), r.rd32(GP_VALUE + 0x488));
        assert!(!f.mem.keys().any(|&a| in_low_ram(a)));
    }

    // ---- plan -------------------------------------------------------------

    fn synth_scus(fixed: bool) -> Vec<u8> {
        let text_len = (QUIET_MALLOC_VA + 0x100 - 0x8001_0000) as usize;
        let mut s = vec![0u8; 0x800 + text_len];
        s[..8].copy_from_slice(b"PS-X EXE");
        s[0x18..0x1C].copy_from_slice(&0x8001_0000u32.to_le_bytes());
        s[0x1C..0x20].copy_from_slice(&(text_len as u32).to_le_bytes());
        let put = |s: &mut Vec<u8>, va: u32, w: u32| {
            let o = 0x800 + (va - 0x8001_0000) as usize;
            s[o..o + 4].copy_from_slice(&w.to_le_bytes());
        };
        put(&mut s, JUMP_TABLE_VA + 0x3C * 4, WINDOW_VA);
        put(&mut s, JUMP_TABLE_VA + 0x08 * 4, HALT_ARM_VA);
        put(&mut s, COPY_VA, addiu(A2, ZERO, 0x2));
        for (i, w) in HALT_ARM.iter().enumerate() {
            put(&mut s, HALT_ARM_VA + i as u32 * 4, *w);
        }
        for (va, w) in NODE_CONTEXT {
            put(&mut s, va, w);
        }
        for site in sites() {
            let words = if fixed { &site.fixed } else { &site.stock };
            for (i, w) in words.iter().enumerate() {
                put(&mut s, site.va + i as u32 * 4, *w);
            }
        }
        s
    }

    #[test]
    fn plans_on_stock_and_is_idempotent() {
        let mut scus = synth_scus(false);
        let fix = plan(&scus).unwrap().unwrap();
        assert_eq!(fix.edits.len(), 4);
        let total: usize = fix.edits.iter().map(|e| e.bytes.len()).sum();
        assert_eq!(total, (8 + 16 + 1 + 8) * 4);
        for e in &fix.edits {
            scus[e.file_off..e.file_off + e.bytes.len()].copy_from_slice(&e.bytes);
        }
        assert_eq!(scus, synth_scus(true));
        assert_eq!(plan(&scus).unwrap(), None);
    }

    #[test]
    fn refuses_unrecognized_or_partly_patched_code() {
        let mut scus = synth_scus(false);
        let o = 0x800 + (WINDOW_VA + 20 - 0x8001_0000) as usize;
        scus[o] ^= 1;
        assert!(plan(&scus).is_err());

        let mut scus = synth_scus(false);
        let o = 0x800 + (HALT_ARM_VA + 12 - 0x8001_0000) as usize;
        scus[o] ^= 0xFF;
        assert!(plan(&scus).is_err());

        // One site fixed, the rest stock.
        let mut scus = synth_scus(false);
        let o = 0x800 + (NODE_CALL_VA - 0x8001_0000) as usize;
        scus[o..o + 4].copy_from_slice(&jal(QUIET_MALLOC_VA).to_le_bytes());
        assert!(plan(&scus).is_err());

        assert!(plan(&[0u8; 0x100]).is_err());
    }
}
