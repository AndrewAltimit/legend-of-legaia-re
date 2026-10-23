//! Disc-gated tests for the **Conkram mirror freeze fix** (see
//! `legaia_code_hooks::conkram_freeze_fix`): the move VM's keyframe-pose seat
//! (op `0x3C`, `0x80023CA4`) gains a NULL test on its heap allocation and,
//! when the allocation fails, leaves through op `0x08`'s HALT arm instead of
//! copying the pose through NULL over the exception vector.
//!
//! These apply the fix to an in-memory copy of the real disc and assert, off
//! the patched image, that:
//!   * the retail handler, HALT arm and jump-table entries hold the
//!     documented words (non-vacuous: the recognized US build);
//!   * exactly the nine planned words change in `SCUS_942.54`, nothing else in
//!     the file or any other file does, and every touched sector stays
//!     EDC/ECC-valid;
//!   * the patched image still parses, the edit is byte-deterministic, and a
//!     second application is a no-op;
//!   * the whole op-`0x3C` handler, executed from the disc's own bytes on the
//!     crate's R3000 model, copies the same pose as retail when the
//!     allocation succeeds and writes nothing below `0x80000100` when it
//!     fails - where retail's copy lands on the exception vector.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset. In-game
//! verification is an emulator run on a freezing setting (see
//! `docs/tooling/randomizer-internals.md`).

use legaia_code_hooks::conkram_freeze_fix::{
    HALT_ARM, HALT_ARM_VA, MALLOC_VA, STOCK_WINDOW, VM_TAIL_VA, WINDOW_VA, assemble_window, plan,
};
use legaia_code_hooks::mips::{RA, V0, jr, lui, ori};
use legaia_code_hooks::mips_sim::Cpu;
use legaia_disc_patch::disc::DiscPatcher;
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_patcher::apply;

const SCUS: &str = "SCUS_942.54";
const SECTOR_SIZE: usize = 2352;
/// End of the op-`0x3C` handler (its `j 0x80024150` delay slot is `+4`).
const HANDLER_END_VA: u32 = 0x8002_3D64;

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

#[test]
fn baseline_holds_the_retail_handler() {
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
    assert_eq!(word(&scus, 0x8001_0778 + 0x3C * 4), WINDOW_VA);
    assert_eq!(word(&scus, 0x8001_0778 + 0x08 * 4), HALT_ARM_VA);
    let fix = plan(&scus)
        .expect("plan")
        .expect("pristine build plans the rewrite");
    assert_eq!(fix.window_off, scus_off(WINDOW_VA));
    assert_eq!(fix.bytes.len(), 64);
}

#[test]
fn fix_changes_exactly_the_planned_words() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let before = patcher.read_named_file(SCUS).expect("SCUS before");

    let report = apply::apply_conkram_freeze_fix(&mut patcher).expect("apply");
    assert!(report.changed);

    let after = patcher.read_named_file(SCUS).expect("SCUS after");
    assert_eq!(before.len(), after.len());
    let fixed = assemble_window();
    let mut changed_words = Vec::new();
    for va in (0x8001_0000u32..0x8001_0000 + (before.len() as u32 - 0x800)).step_by(4) {
        if word(&before, va) != word(&after, va) {
            changed_words.push(va);
        }
    }
    let planned: Vec<u32> = (0..16)
        .filter(|&i| fixed[i] != STOCK_WINDOW[i])
        .map(|i| WINDOW_VA + i as u32 * 4)
        .collect();
    assert_eq!(planned.len(), 9);
    assert_eq!(changed_words, planned, "only the nine planned words change");
    for (i, want) in fixed.iter().enumerate() {
        assert_eq!(word(&after, WINDOW_VA + i as u32 * 4), *want);
    }
    assert_eq!(&before[..0x800], &after[..0x800], "EXE header untouched");

    // Every raw sector that changed is a valid Mode 2 Form 1 sector.
    let img = patcher.image();
    let mut sectors: Vec<usize> = disc
        .iter()
        .zip(img.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i / SECTOR_SIZE)
        .collect();
    sectors.dedup();
    assert!(!sectors.is_empty() && sectors.len() <= 2, "{sectors:?}");
    // ...and all of them belong to SCUS_942.54: no other file moved.
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

/// Run the disc's op-`0x3C` handler with a malloc stub returning `ret`, for a
/// four-part pose. Returns the machine at the VM tail.
fn run_handler(scus: &[u8], ret: u32) -> Cpu {
    let mut c = Cpu::new();
    let lo = scus_off(WINDOW_VA);
    let hi = scus_off(HANDLER_END_VA + 4);
    c.load(WINDOW_VA, &scus[lo..hi]);
    c.load(
        HALT_ARM_VA,
        &scus[scus_off(HALT_ARM_VA)..scus_off(HALT_ARM_VA + 24)],
    );
    c.load_words(
        MALLOC_VA,
        &[lui(V0, (ret >> 16) as u16), jr(RA), ori(V0, V0, ret as u16)],
    );
    let actor = 0x8010_0000u32;
    let node = 0x8010_1000u32;
    let ops = 0x8010_2000u32;
    c.wr32(actor + 0x44, node);
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

#[test]
fn disc_handler_copies_like_retail_and_never_writes_through_null() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let stock = patcher.read_named_file(SCUS).expect("SCUS");
    apply::apply_conkram_freeze_fix(&mut patcher).expect("apply");
    let fixed = patcher.read_named_file(SCUS).expect("patched SCUS");

    let data = |c: &Cpu| {
        let mut m: Vec<(u32, u8)> = c
            .mem
            .iter()
            .filter(|(a, _)| !(0x8001_0000..0x8003_0000).contains(*a))
            .map(|(a, v)| (*a, *v))
            .collect();
        m.sort_unstable();
        m
    };

    // Success: identical writes, identical PC delta (2 + 6 * count).
    let a = run_handler(&stock, 0x8015_0000);
    let b = run_handler(&fixed, 0x8015_0000);
    assert_eq!(data(&a), data(&b));
    assert_eq!(a.r[6], 2 + 6 * 4);
    assert_eq!(b.r[6], 2 + 6 * 4);
    assert_eq!(b.rd16(0x8015_0000 + 8 + 4 * 8), 0x100, "first part seeded");

    // Failure: retail writes the pose over the exception vector. NULL is
    // KUSEG address 0, the same RAM as `0x80000000`; count both mirrors.
    let low = |c: &Cpu| {
        c.mem
            .keys()
            .filter(|&&a| a < 0x100 || (0x8000_0000..0x8000_0100).contains(&a))
            .count()
    };
    let r = run_handler(&stock, 0);
    assert!(low(&r) > 0, "retail copies through NULL");
    assert!(r.mem.contains_key(&0x80), "retail reaches the vector");
    // ...the fix halts the part with a zero PC delta and writes nothing low.
    let f = run_handler(&fixed, 0);
    assert_eq!(low(&f), 0);
    assert_eq!(f.r[6], 0, "zero PC delta");
    assert_eq!(f.r[22], 0, "VM loop ends");
    assert_eq!(f.rd32(0x8010_0000 + 0x10) & 8, 8, "halt bit");
    assert_eq!(f.rd32(0x8010_0000 + 0x4C), 0);
}
