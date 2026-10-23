//! **Conkram mirror freeze fix**: make the move VM's keyframe-pose seat (op
//! `0x3C`) survive a failed heap allocation instead of copying its pose
//! through the NULL pointer over the CPU's exception vector.
//!
//! ## The freeze
//!
//! In present-day Conkram's Room of Mirrors (`concnow` `P2[18]`) the heap
//! runs with about 16.6 KB free; on an emulated CPU faster than a console's,
//! the mirror effect keeps more parts alive while the music poller
//! `FUN_800243F0` pre-stages `PLAYER1..3`, and an allocation fails. The
//! failing caller is op `0x3C` of the move VM `FUN_80023070` (jump-table
//! entry `0x80010778 + 0x3C * 4` = `0x80023CA4`):
//!
//! ```text
//! 80023ca4  lh    s3,0x2(s0)        ; count = op[1]
//! 80023ca8  lw    v0,0x44(s2)
//! 80023cac  nop
//! 80023cb0  sw    s3,0x0(v0)        ; model list count word
//! 80023cb4  lw    v1,0x4c(s2)       ; pose block, NULL until the first seat
//! 80023cb8  li    v0,0x6
//! 80023cbc  sh    v0,0x5a(s2)       ; render mode 6
//! 80023cc0  sh    zero,0x22(s2)
//! 80023cc4  sh    zero,0x68(s2)
//! 80023cc8  bne   v1,zero,0x80023ce4
//! 80023ccc  _sh   zero,0x5c(s2)
//! 80023cd0  clear a0
//! 80023cd4  sll   a1,s3,0x5
//! 80023cd8  jal   0x80017888        ; malloc(0, count << 5 | 8)
//! 80023cdc  _ori  a1,a1,0x8
//! 80023ce0  sw    v0,0x4c(s2)       ; stored with no NULL test
//! 80023ce4  li    a2,0x2            ; ... then the per-part copy through it
//! ```
//!
//! The copy (`sh` at `0x80023D30` / `0x80023D3C`) writes from
//! `block + 8 + count * 8`, so a NULL block puts a four-part pose over
//! `0x80000028..0x8000008C` - across the general exception vector at
//! `0x80000080`. The next interrupt executes pose data and the CPU loops on a
//! reserved-instruction exception at the vector.
//!
//! ## The fix: on NULL, halt the part through retail's own HALT arm
//!
//! The window above has one wasted word, the load-delay `nop` at
//! `0x80023CAC`. Moving the independent `sh zero,0x22(s2)` into it shifts the
//! following five instructions up by one word, which frees the word after the
//! `jal` for a NULL test:
//!
//! ```text
//! 80023cac  sh    zero,0x22(s2)     ; fills the lw v0 load delay
//! 80023cc0  sh    zero,0x68(s2)
//! 80023cc4  bne   v1,zero,0x80023ce4
//! 80023cc8  _sh   zero,0x5c(s2)
//! 80023ccc  clear a0
//! 80023cd0  sll   a1,s3,0x5
//! 80023cd4  jal   0x80017888
//! 80023cd8  _ori  a1,a1,0x8
//! 80023cdc  beq   v0,zero,0x800232b8   ; NULL: op 0x08's HALT arm
//! 80023ce0  _sw   v0,0x4c(s2)          ; (unchanged word, now a delay slot)
//! ```
//!
//! The HALT arm (`0x800232B8`, op `0x08`) is `a2 = 0; s6 = 0; +0x10 |= 8;
//! j 0x80024150`: the VM loop adds a zero PC delta and returns. The part tick
//! tests `+0x10 & 8` straight after the VM call (`0x80022BB4`) and skips the
//! mode-6 pack; the list walk `FUN_8002519C` marks the halted part and
//! releases it on the next pass, freeing its render node (flag `0x800`) and
//! skipping `+0x4C`, which stays NULL (`beqz` at `0x80025264`). The part is
//! simply gone - one effect particle fewer - and its node goes back to the
//! heap the effect is starving.
//!
//! The delay-slot `sw` writes the NULL into `+0x4C`, which already held 0
//! (the allocation only runs when it was empty), so the failure path stores
//! nothing new. The success path executes the same instructions as retail in
//! the same order apart from the `+0x22` store, which now precedes the
//! count-word store through `+0x44` - two stores to unrelated memory (the
//! node is its own heap block). No branch in any image targets
//! `0x80023CA8..0x80023CE4` (five-form address scan), and the jump table
//! enters the handler at `0x80023CA4`, which is unchanged.
//!
//! Why halting and not skipping the install: a part whose pose block is NULL
//! would still meet op `0x3D`, which writes through `+0x4C` with no test
//! either (`0x80023D90`, `0x80023EC0`); retrying next frame keeps a part
//! (and its node) alive on a heap that may not recover; a static fallback
//! block would be handed to `free` by the list walk. Halting is the only
//! in-place choice that leaves no later writer behind.
//!
//! One same-size edit of nine words in `SCUS_942.54`, verified against the
//! stock handler words and the HALT arm before writing; an unrecognized build
//! is refused and an already-fixed image is a no-op. The stock words below
//! are cited from the project's own disassembly reference; no game bytes are
//! embedded.

use anyhow::{Result, anyhow, bail};
use legaia_game_tables::item_names;

use crate::mips::{
    A0, A1, A2, S0, S2, S3, V0, V1, ZERO, addiu, addu, beq, bne, jal, lh, lw, ori, sh, sll, sw,
};

/// First word of the checked window: the op-`0x3C` handler entry.
pub const WINDOW_VA: u32 = 0x8002_3CA4;
/// The malloc wrapper the handler calls (`FUN_80017888`).
pub const MALLOC_VA: u32 = 0x8001_7888;
/// Op `0x08`'s HALT arm in the move VM.
pub const HALT_ARM_VA: u32 = 0x8002_32B8;
/// The move VM's common PC-advance tail.
pub const VM_TAIL_VA: u32 = 0x8002_4150;
/// The instruction after the window: the copy's `li a2,0x2`.
pub const COPY_VA: u32 = 0x8002_3CE4;
/// The move VM's opcode jump table.
pub const JUMP_TABLE_VA: u32 = 0x8001_0778;

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

/// The HALT arm (`0x800232B8..0x800232D0`) the fix branches to.
pub const HALT_ARM: [u32; 6] = [
    0x0000_3021, // move a2,zero
    0x8E42_0010, // lw v0,0x10(s2)
    0x00C0_B021, // move s6,a2
    0x3442_0008, // ori v0,v0,0x8
    0x0800_9054, // j 0x80024150
    0xAE42_0010, // _sw v0,0x10(s2)
];

/// Signed word offset of a branch at `from` to `to`.
fn branch_off(from: u32, to: u32) -> i16 {
    let delta = (to as i64 - (from as i64 + 4)) / 4;
    i16::try_from(delta).expect("branch target in range")
}

/// Assemble the replacement window (sixteen words, same size).
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
        jal(MALLOC_VA),                                 // 12
        ori(A1, A1, 0x8),                               // 13 (delay)
        beq(V0, ZERO, branch_off(va(14), HALT_ARM_VA)), // 14 NULL -> HALT arm
        sw(V0, S2, 0x4C),                               // 15 (delay, both paths)
    ];
    debug_assert_eq!(words.len(), STOCK_WINDOW.len());
    words
}

/// A planned fix: one same-size window rewrite in `SCUS_942.54`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkramFreezeFix {
    /// File offset of [`WINDOW_VA`] in `SCUS_942.54`.
    pub window_off: usize,
    /// Replacement bytes (sixteen little-endian words).
    pub bytes: Vec<u8>,
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

/// Plan the fix against the raw `SCUS_942.54` bytes.
///
/// Returns `Ok(None)` when the window already holds the fix. Fails, writing
/// nothing, when the window is neither stock nor fixed, or when the jump
/// table, the HALT arm or the word after the window do not read as the
/// documented disassembly.
pub fn plan(scus: &[u8]) -> Result<Option<ConkramFreezeFix>> {
    let context = [
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
    for (va, want, what) in context {
        let got = word_at(scus, va)?;
        if got != want {
            bail!(
                "conkram-freeze-fix: {what} at {va:#x} = {got:#010x}, expected {want:#010x} \
                 (unrecognized build) - refusing to patch"
            );
        }
    }
    for (i, want) in HALT_ARM.iter().enumerate() {
        let va = HALT_ARM_VA + i as u32 * 4;
        let got = word_at(scus, va)?;
        if got != *want {
            bail!(
                "conkram-freeze-fix: HALT arm word {va:#x} = {got:#010x}, expected {want:#010x} \
                 (unrecognized build) - refusing to patch"
            );
        }
    }

    let window_off = scus_off(scus, WINDOW_VA)?;
    let mut current = [0u32; 16];
    for (i, w) in current.iter_mut().enumerate() {
        *w = word_at(scus, WINDOW_VA + i as u32 * 4)?;
    }
    let replacement = assemble_window();
    if current[..] == replacement[..] {
        return Ok(None);
    }
    if current != STOCK_WINDOW {
        bail!(
            "conkram-freeze-fix: op 0x3C handler at {WINDOW_VA:#x} is not the stock code \
             (first divergence at word {}) - unrecognized build, refusing to patch",
            current
                .iter()
                .zip(STOCK_WINDOW.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0),
        );
    }
    let bytes = replacement.iter().flat_map(|w| w.to_le_bytes()).collect();
    Ok(Some(ConkramFreezeFix { window_off, bytes }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips::{RA, S6, j, jr, lui, nop};
    use crate::mips_sim::Cpu;

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
    }

    #[test]
    fn fix_changes_nine_words_and_keeps_the_rest() {
        let g = assemble_window();
        let changed: Vec<usize> = (0..16).filter(|&i| g[i] != STOCK_WINDOW[i]).collect();
        assert_eq!(changed, vec![2, 7, 8, 9, 10, 11, 12, 13, 14]);
        // Entry, both loads and the final store are untouched.
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
    }

    /// Simulated machine: the handler window, the HALT arm, a malloc stub
    /// returning `ret`, the actor at `0x80100000` with its node at
    /// `0x80101000`, and an op stream with `count` parts.
    fn machine(window: &[u32], ret: u32, block: u32, count: u16) -> Cpu {
        let mut c = Cpu::new();
        c.load_words(WINDOW_VA, window);
        c.load_words(HALT_ARM_VA, &HALT_ARM);
        // malloc stub: v0 = ret; jr ra
        c.load_words(
            MALLOC_VA,
            &[lui(V0, (ret >> 16) as u16), jr(RA), ori(V0, V0, ret as u16)],
        );
        let actor = 0x8010_0000;
        let node = 0x8010_1000;
        let ops = 0x8010_2000;
        c.wr32(actor + 0x44, node);
        c.wr32(actor + 0x4C, block);
        c.wr32(actor + 0x10, 0x0000_0800);
        c.wr16(actor + 0x22, 0x1234);
        c.wr16(actor + 0x5A, 0x0002);
        c.wr16(ops, 0x3C);
        c.wr16(ops + 2, count);
        c.r[S2 as usize] = actor;
        c.r[S0 as usize] = ops;
        c.r[S6 as usize] = 1;
        c.r[A2 as usize] = 0xDEAD;
        c.pc = WINDOW_VA;
        c
    }

    fn run(window: &[u32], ret: u32, block: u32) -> Cpu {
        let mut c = machine(window, ret, block, 4);
        c.run_until(&[COPY_VA, VM_TAIL_VA]);
        c
    }

    #[test]
    fn success_path_matches_retail() {
        let fixed = assemble_window();
        for (ret, block) in [(0x8015_0000, 0), (0, 0x8016_0000)] {
            let a = run(&STOCK_WINDOW, ret, block);
            let b = run(&fixed, ret, block);
            assert_eq!(a.pc, COPY_VA);
            assert_eq!(b.pc, COPY_VA);
            let data = |c: &Cpu| {
                let mut m: Vec<(u32, u8)> = c
                    .mem
                    .iter()
                    .filter(|(a, _)| (0x8010_0000..0x8020_0000).contains(*a))
                    .map(|(a, v)| (*a, *v))
                    .collect();
                m.sort_unstable();
                m
            };
            assert_eq!(data(&a), data(&b), "data memory after the seat");
            for r in [S0, S2, S3, S6, V0, V1, A0, A1] {
                assert_eq!(a.r[r as usize], b.r[r as usize], "reg {r}");
            }
        }
    }

    #[test]
    fn null_allocation_halts_the_part_instead_of_copying() {
        let c = run(&assemble_window(), 0, 0);
        let actor = 0x8010_0000;
        assert_eq!(c.pc, VM_TAIL_VA, "left through the HALT arm");
        assert_eq!(c.r[A2 as usize], 0, "zero PC delta");
        assert_eq!(c.r[S6 as usize], 0, "VM loop ends");
        assert_eq!(
            c.rd32(actor + 0x10),
            0x0000_0808,
            "halt bit set, node flag kept"
        );
        assert_eq!(c.rd32(actor + 0x4C), 0, "pose block stays NULL");
        // Nothing below 0x80000100 was written.
        assert!(
            c.mem
                .keys()
                .all(|&a| a >= 0x100 && !(0x8000_0000..0x8000_0100).contains(&a))
        );
    }

    #[test]
    fn stock_null_allocation_reaches_the_copy() {
        // The defect: retail falls into the copy with a NULL block.
        let c = run(&STOCK_WINDOW, 0, 0);
        assert_eq!(c.pc, COPY_VA);
        assert_eq!(c.rd32(0x8010_0000 + 0x4C), 0);
    }

    fn synth_scus(window: &[u32]) -> Vec<u8> {
        // A PS-X EXE header with t_addr 0x80010000, then text up to the VM tail.
        let text_len = (VM_TAIL_VA + 0x100 - 0x8001_0000) as usize;
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
        for (i, w) in window.iter().enumerate() {
            put(&mut s, WINDOW_VA + i as u32 * 4, *w);
        }
        s
    }

    #[test]
    fn plans_on_stock_and_is_idempotent() {
        let mut scus = synth_scus(&STOCK_WINDOW);
        let fix = plan(&scus).unwrap().unwrap();
        assert_eq!(fix.window_off, 0x800 + (WINDOW_VA - 0x8001_0000) as usize);
        assert_eq!(fix.bytes.len(), 64);
        scus[fix.window_off..fix.window_off + 64].copy_from_slice(&fix.bytes);
        assert_eq!(plan(&scus).unwrap(), None);
    }

    #[test]
    fn refuses_unrecognized_code() {
        let mut w = STOCK_WINDOW;
        w[5] ^= 1;
        assert!(plan(&synth_scus(&w)).is_err());
        let mut scus = synth_scus(&STOCK_WINDOW);
        let o = 0x800 + (HALT_ARM_VA + 12 - 0x8001_0000) as usize;
        scus[o] ^= 0xFF;
        assert!(plan(&scus).is_err());
        assert!(plan(&[0u8; 0x100]).is_err());
    }
}
