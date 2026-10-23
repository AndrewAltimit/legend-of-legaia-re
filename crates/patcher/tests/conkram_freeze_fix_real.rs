//! Disc-gated tests for the **Conkram mirror freeze fix** (see
//! `legaia_code_hooks::conkram_freeze_fix`): the move VM's keyframe-pose seat
//! (op `0x3C`) halts the part on a failed pose-block allocation, the
//! render-node allocation in `FUN_80020F88` installs a shared stand-in node
//! instead of leaving NULL at `actor+0x44`, and both allocate through a quiet
//! entry that leaves the `PRG ERR` counter alone on a failure.
//!
//! These apply the fix to an in-memory copy of the real disc and assert, off
//! the patched image, that:
//!   * every site, the HALT arm and the jump-table entries hold the
//!     documented retail words (non-vacuous: the recognized US build);
//!   * exactly the planned words change in `SCUS_942.54`, no other file moves,
//!     and every touched sector stays EDC/ECC-valid;
//!   * the patched image still parses, the edit is byte-deterministic, and a
//!     second application is a no-op;
//!   * executed from the disc's own bytes on the crate's R3000 model, with
//!     the retail malloc wrapper and a stub allocator, both allocation sites
//!     leave retail's state on success; on failure retail writes below
//!     `0x80000100` and bumps `PRG ERR`, and the fix does neither.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset. In-game
//! verification is an emulator run on a freezing setting (see
//! `docs/tooling/randomizer-internals.md`).

use legaia_code_hooks::conkram_freeze_fix::{
    ALLOC_VA, GP_VALUE, HALT_ARM, HALT_ARM_VA, MALLOC_VA, NODE_CALL_VA, NODE_EPILOGUE_VA,
    NODE_OK_VA, PRG_ERR_VA, QUIET_MALLOC_VA, STANDIN_NODE_VA, STOCK_NODE_CALL, STOCK_NODE_FAIL,
    STOCK_THUNK, STOCK_WINDOW, VM_TAIL_VA, WINDOW_VA, plan, site_spans,
};
use legaia_code_hooks::mips::{RA, V0, jr, lui, nop, ori};
use legaia_code_hooks::mips_sim::Cpu;
use legaia_disc_patch::disc::DiscPatcher;
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_patcher::apply;

const SCUS: &str = "SCUS_942.54";
const SECTOR_SIZE: usize = 2352;
/// End of the op-`0x3C` handler (its `j 0x80024150` delay slot is `+4`).
const HANDLER_END_VA: u32 = 0x8002_3D64;
/// The BIOS-printf wrapper the malloc wrapper's failure arm calls.
const PRINTF_VA: u32 = 0x8005_67A8;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_off(va: u32) -> usize {
    0x800 + (va - 0x8001_0000) as usize
}

fn word(buf: &[u8], va: u32) -> u32 {
    let o = scus_off(va);
    u32::from_le_bytes(buf[o..o + 4].try_into().unwrap())
}

fn low_ram_writes(c: &Cpu) -> usize {
    c.mem
        .keys()
        .filter(|&&a| a < 0x100 || (0x8000_0000..0x8000_0100).contains(&a))
        .count()
}

#[test]
fn baseline_holds_the_retail_code() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let scus = patcher.read_named_file(SCUS).expect("read SCUS");
    for (i, want) in STOCK_WINDOW.iter().enumerate() {
        assert_eq!(
            word(&scus, WINDOW_VA + i as u32 * 4),
            *want,
            "window word {i}"
        );
    }
    for (i, want) in HALT_ARM.iter().enumerate() {
        assert_eq!(
            word(&scus, HALT_ARM_VA + i as u32 * 4),
            *want,
            "HALT word {i}"
        );
    }
    assert_eq!(word(&scus, NODE_CALL_VA), STOCK_NODE_CALL);
    for (i, want) in STOCK_NODE_FAIL.iter().enumerate() {
        assert_eq!(
            word(&scus, NODE_CALL_VA + 0x10 + i as u32 * 4),
            *want,
            "node block {i}"
        );
    }
    for (i, want) in STOCK_THUNK.iter().enumerate() {
        assert_eq!(
            word(&scus, QUIET_MALLOC_VA + i as u32 * 4),
            *want,
            "thunk word {i}"
        );
    }
    assert_eq!(word(&scus, 0x8001_0778 + 0x3C * 4), WINDOW_VA);
    assert_eq!(word(&scus, 0x8001_0778 + 0x08 * 4), HALT_ARM_VA);
    let fix = plan(&scus)
        .expect("plan")
        .expect("pristine build plans the fix");
    assert_eq!(fix.edits.len(), 4);
}

#[test]
fn fix_changes_exactly_the_planned_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let before = patcher.read_named_file(SCUS).expect("SCUS before");
    let fix = plan(&before).unwrap().unwrap();

    let report = apply::apply_conkram_freeze_fix(&mut patcher).expect("apply");
    assert!(report.changed);

    let after = patcher.read_named_file(SCUS).expect("SCUS after");
    assert_eq!(before.len(), after.len());
    // The planned image: the stock SCUS with every edit laid over it.
    let mut planned = before.clone();
    for e in &fix.edits {
        planned[e.file_off..e.file_off + e.bytes.len()].copy_from_slice(&e.bytes);
    }
    assert_eq!(after, planned, "SCUS is exactly stock + the planned edits");
    // Every changed byte lies inside a declared site.
    let spans = site_spans();
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        if a != b {
            let va = 0x8001_0000 + (i - 0x800) as u32;
            assert!(
                spans.iter().any(|&(s, n)| va >= s && va < s + n as u32 * 4),
                "byte at {va:#x} changed outside the sites"
            );
        }
    }

    // Every raw sector that changed belongs to SCUS and is a valid Mode 2
    // Form 1 sector.
    let img = patcher.image();
    let mut sectors: Vec<usize> = disc
        .iter()
        .zip(img.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i / SECTOR_SIZE)
        .collect();
    sectors.dedup();
    assert!(!sectors.is_empty() && sectors.len() <= 8, "{sectors:?}");
    let (lba, size) = find_file_in_image(&disc, SCUS).expect("SCUS on disc");
    let scus_sectors = lba as usize..lba as usize + (size as usize).div_ceil(2048);
    for s in sectors {
        assert!(scus_sectors.contains(&s), "sector {s} outside SCUS");
        let sb = s * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "sector {s} stays EDC/ECC-valid"
        );
    }
    read_file_in_image(patcher.image(), SCUS).expect("patched image re-reads");
}

#[test]
fn fix_is_byte_deterministic_and_idempotent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::apply_conkram_freeze_fix(&mut a).unwrap();
    apply::apply_conkram_freeze_fix(&mut b).unwrap();
    assert_eq!(a.image(), b.image(), "byte-identical patched images");
    let again = apply::apply_conkram_freeze_fix(&mut a).expect("re-apply");
    assert!(!again.changed);
    let mut c = DiscPatcher::open(a.image().to_vec()).expect("open patched");
    assert!(!apply::apply_conkram_freeze_fix(&mut c).unwrap().changed);
}

/// A machine holding the disc's own malloc wrapper, quiet entry (patched
/// image only, else the stock thunk) and a stub allocator returning `ret`;
/// the printf the wrapper's failure arm calls is a bare return.
fn alloc_machine(scus: &[u8], ret: u32) -> Cpu {
    let mut c = Cpu::new();
    c.r[28] = GP_VALUE;
    c.r[29] = 0x801F_FF00; // sp
    c.load(
        MALLOC_VA,
        &scus[scus_off(MALLOC_VA)..scus_off(MALLOC_VA + 0x68)],
    );
    c.load(
        QUIET_MALLOC_VA,
        &scus[scus_off(QUIET_MALLOC_VA)..scus_off(QUIET_MALLOC_VA + 0x20)],
    );
    c.load_words(
        ALLOC_VA,
        &[lui(V0, (ret >> 16) as u16), jr(RA), ori(V0, V0, ret as u16)],
    );
    c.load_words(PRINTF_VA, &[jr(RA), nop()]);
    c
}

/// Run the disc's op-`0x3C` handler for a four-part pose.
fn run_handler(scus: &[u8], ret: u32) -> Cpu {
    let mut c = alloc_machine(scus, ret);
    c.load(
        WINDOW_VA,
        &scus[scus_off(WINDOW_VA)..scus_off(HANDLER_END_VA + 4)],
    );
    c.load(
        HALT_ARM_VA,
        &scus[scus_off(HALT_ARM_VA)..scus_off(HALT_ARM_VA + 24)],
    );
    let actor = 0x8010_0000u32;
    let ops = 0x8010_2000u32;
    c.wr32(actor + 0x44, 0x8010_1000);
    c.wr16(actor + 0x70, 0x40);
    let mut stream = vec![0x3Cu16, 4];
    stream.extend((0..24).map(|k| 0x100 + k as u16));
    for (i, h) in stream.iter().enumerate() {
        c.wr16(ops + i as u32 * 2, *h);
    }
    c.r[18] = actor; // s2
    c.r[17] = actor + 0x80; // s1
    c.r[16] = ops; // s0
    c.r[22] = 1; // s6
    c.pc = WINDOW_VA;
    c.run_until(&[VM_TAIL_VA]);
    c
}

/// Run `FUN_80020F88` from its render-node call to either continuation.
fn run_node(scus: &[u8], ret: u32) -> Cpu {
    let mut c = alloc_machine(scus, ret);
    c.load(
        NODE_CALL_VA - 4,
        &scus[scus_off(NODE_CALL_VA - 4)..scus_off(NODE_OK_VA)],
    );
    let actor = 0x8010_0000u32;
    c.wr32(actor + 0x10, 0x0000_0002);
    c.wr16(actor + 0x56, 0x0004);
    c.r[16] = actor; // s0
    c.pc = NODE_CALL_VA - 4;
    c.run_until(&[NODE_OK_VA, NODE_EPILOGUE_VA]);
    c
}

/// Memory the game would keep: everything but the stack band and the code.
fn data(c: &Cpu) -> Vec<(u32, u8)> {
    let mut m: Vec<(u32, u8)> = c
        .mem
        .iter()
        .filter(|(a, _)| !(0x8001_0000..0x8007_0000).contains(*a))
        .filter(|(a, _)| !(0x801F_F000..0x8020_0000).contains(*a))
        .map(|(a, v)| (*a, *v))
        .collect();
    m.sort_unstable();
    m
}

#[test]
fn disc_code_matches_retail_on_success_and_writes_nothing_low_on_failure() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let stock = patcher.read_named_file(SCUS).expect("SCUS");
    apply::apply_conkram_freeze_fix(&mut patcher).expect("apply");
    let fixed = patcher.read_named_file(SCUS).expect("patched SCUS");
    let actor = 0x8010_0000u32;

    // Op 0x3C, success: identical writes and PC delta (2 + 6 * count).
    let a = run_handler(&stock, 0x8015_0000);
    let b = run_handler(&fixed, 0x8015_0000);
    assert_eq!(data(&a), data(&b));
    assert_eq!(a.r[6], 2 + 6 * 4);
    assert_eq!(b.r[6], 2 + 6 * 4);
    assert_eq!(b.rd32(GP_VALUE + 0x488), 1, "allocation counted as retail");

    // Op 0x3C, failure: retail copies over the vector and bumps PRG ERR...
    let r = run_handler(&stock, 0);
    assert!(r.mem.contains_key(&0x80), "retail reaches the vector");
    assert_eq!(r.rd32(PRG_ERR_VA), 0x20000);
    // ...the fix halts the part, writes nothing low, leaves PRG ERR alone.
    let f = run_handler(&fixed, 0);
    assert_eq!(low_ram_writes(&f), 0);
    assert_eq!(f.r[6], 0, "zero PC delta");
    assert_eq!(f.r[22], 0, "VM loop ends");
    assert_eq!(f.rd32(actor + 0x10) & 8, 8, "halt bit");
    assert_eq!(f.rd32(PRG_ERR_VA), 0);

    // Render node, success: identical.
    let a = run_node(&stock, 0x8015_0000);
    let b = run_node(&fixed, 0x8015_0000);
    assert_eq!(a.pc, NODE_OK_VA);
    assert_eq!(b.pc, NODE_OK_VA);
    assert_eq!(data(&a), data(&b));

    // Render node, failure: retail leaves NULL and PRG ERR 0x24000; the fix
    // installs the stand-in and leaves PRG ERR alone.
    let r = run_node(&stock, 0);
    assert_eq!(r.rd32(actor + 0x44), 0);
    assert_eq!(r.rd32(PRG_ERR_VA), 0x24000);
    let f = run_node(&fixed, 0);
    assert_eq!(f.pc, NODE_EPILOGUE_VA);
    assert_eq!(f.rd32(actor + 0x44), STANDIN_NODE_VA);
    assert_eq!(f.rd16(STANDIN_NODE_VA + 0x9A), 0xFFFF);
    assert_eq!(f.rd16(actor + 0x56), 0);
    assert_eq!(f.rd32(actor + 0x10) & 0x800, 0, "stand-in never freed");
    assert_eq!(f.r[2], 0xFFFF_FFFF, "returns -1 as retail");
    assert_eq!(f.rd32(PRG_ERR_VA), 0);
    assert_eq!(low_ram_writes(&f), 0);
}
