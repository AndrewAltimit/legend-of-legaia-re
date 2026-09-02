//! Disc-gated oracle for the **Super Arts Pack, by ZetaPhoenix**
//! (see `legaia_patcher::super_arts_pack`).
//!
//! The injection has two halves: ZetaPhoenix's 3764-byte block parked in the
//! `DMY.DAT` annex, and fourteen same-size edited words across ten sites
//! (seven sites in PROT 0898, three in `SCUS_942.54`) - ZetaPhoenix's own hook
//! set plus the battle-load hook - that make retail code load it and jump into
//! it. These tests
//! apply it to a scratch copy of the real disc and assert, off the patched
//! image, that:
//!   * the block landed in the annex **byte-identical** to the embedded file -
//!     the load the injected stub performs reads exactly ZetaPhoenix's bytes;
//!   * the loader stub reads that LBA into `0x801FD000` and returns to battle
//!     init, and every hook word became the planned jump;
//!   * the applier now addresses the pack's tables at the pack's strides;
//!   * each edit is surgical (nothing outside the planned words moved) and the
//!     patched sectors stay readable / EDC-valid;
//!   * the patch is byte-deterministic; and
//!   * the planner refuses an unrecognized build or a claimed arena instead of
//!     corrupting either.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips and passes when unset. The clean-room
//! engine cannot execute injected MIPS, so the runtime half of the proof lives
//! in the crate's own disc-gated unit tests, which run the patched retail
//! applier over the block in the in-crate interpreter.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::super_arts_pack::{
    self as pack, BLOCK, BLOCK_SECTORS, BLOCK_VA, FIND_TABLE_VA, HOOK_APPLIER_VA, HOOK_BANNER_VA,
    HOOK_BOUND_VA, HOOK_EXIT_SEED_VA, HOOK_FIND_HI_VA, HOOK_KEEP_NAME_VA, HOOK_QUEUE_DELAY_VA,
    HOOK_QUEUE_VA, HOOK_REPL_HI_VA, HOOK_STRIDE_VA, LOAD_HOOK_VA, OVERLAY_BASE_VA,
    OVERLAY_PROT_INDEX, REPLACE_TABLE_VA, ROUTINE_APPLIER_VA, ROUTINE_BANNER_VA,
    ROUTINE_KEEP_NAME_VA, STUB_VA, SuperArtsPackInjection, T5_SOURCE_VA,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_word(scus: &[u8], va: u32) -> u32 {
    let off = file_offset_for_va(scus, va).expect("resolve SCUS va");
    u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
}

fn scus_words(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    (0..n).map(|i| scus_word(scus, va + i as u32 * 4)).collect()
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// `j target` as the R3000 encodes it.
fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}

#[test]
fn baseline_hook_sites_match_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let overlay = patcher.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");

    // The plan is the guard: it fingerprints every hook word, checks the arena
    // is dead space, and compares the pack's retail rows against this disc's own
    // Super Art trigger table.
    SuperArtsPackInjection::plan(&scus, &overlay, 1).expect("plans on the retail US build");

    // Spelled out, so a build change names the site that moved.
    assert_eq!(
        overlay_word(&overlay, HOOK_STRIDE_VA),
        0,
        "load-delay nop the stride edit burns"
    );
    assert_eq!(
        overlay_word(&overlay, T5_SOURCE_VA),
        0x00A0_6821,
        "move t5,a1 - t5 must still be the raw character at the stride site"
    );
    assert_eq!(overlay_word(&overlay, HOOK_APPLIER_VA), 0, "load-delay nop");
    assert_eq!(
        overlay_word(&overlay, HOOK_QUEUE_VA),
        0x0080_C821,
        "move t9,a0"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_QUEUE_DELAY_VA),
        0xAFB3_0054,
        "sw s3,0x54(sp) - the store routine B replays"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_BOUND_VA),
        0x28A2_0005,
        "slti v0,a1,5"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_EXIT_SEED_VA),
        0x2405_0005,
        "li a1,5 - the match arm's exit seed"
    );
    assert_eq!(
        scus_word(&scus, HOOK_BANNER_VA),
        0xAC82_074C,
        "sw v0,0x74c(a0)"
    );
    assert_eq!(
        scus_word(&scus, HOOK_KEEP_NAME_VA),
        0xAE02_074C,
        "sw v0,0x74c(s0)"
    );
    assert_eq!(scus_word(&scus, LOAD_HOOK_VA), 0x3C06_1F80, "lui a2,0x1f80");
}

#[test]
fn block_lands_in_the_annex_byte_identical() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let report = apply::inject_super_arts_pack(&mut patcher).expect("inject");

    assert_eq!(report.block_sectors, BLOCK_SECTORS);
    assert_eq!(
        report.names.len(),
        15,
        "five added Super Arts per character"
    );
    assert_eq!(report.names[0], "Ultra Elbow");

    // What the injected stub will stream to 0x801FD000 must be ZetaPhoenix's
    // block, unmodified, with the tail of the last sector zero-padded.
    let parked = patcher
        .read_disc_sectors(report.block_lba, report.block_sectors)
        .expect("read the annexed block back");
    assert_eq!(parked.len(), BLOCK_SECTORS as usize * 2048);
    assert_eq!(&parked[..BLOCK.len()], BLOCK, "block installed unmodified");
    assert!(
        parked[BLOCK.len()..].iter().all(|&b| b == 0),
        "the sector tail is zero padding"
    );
}

#[test]
fn hooks_reach_the_block_and_the_tables_are_retargeted() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let report = apply::inject_super_arts_pack(&mut patcher).expect("inject");
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let overlay = patcher.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");

    // 1. The four jumps into ZetaPhoenix's routines.
    assert_eq!(
        overlay_word(&overlay, HOOK_APPLIER_VA),
        j(ROUTINE_APPLIER_VA)
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_QUEUE_VA),
        j(pack::ROUTINE_QUEUE_VA)
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_QUEUE_DELAY_VA),
        0x0080_C821,
        "the displaced move t9,a0 rides the jump's delay slot"
    );
    assert_eq!(scus_word(&scus, HOOK_BANNER_VA), j(ROUTINE_BANNER_VA));
    assert_eq!(scus_word(&scus, HOOK_KEEP_NAME_VA), j(ROUTINE_KEEP_NAME_VA));
    assert_eq!(
        scus_word(&scus, HOOK_KEEP_NAME_VA + 4),
        0,
        "second store nopped"
    );

    // 2. The applier now addresses the pack's tables, at the pack's strides.
    //    `lui`+`addiu` resolve to the table VAs; `t5` is doubled so the
    //    character stride the applier computes (`t5*65` / `t5*80`) becomes
    //    10 rows of 13 / 16 bytes.
    let find_pair = (0..2)
        .map(|i| overlay_word(&overlay, HOOK_FIND_HI_VA + i * 4))
        .collect::<Vec<_>>();
    let repl_pair = (0..2)
        .map(|i| overlay_word(&overlay, HOOK_REPL_HI_VA + i * 4))
        .collect::<Vec<_>>();
    let resolve = |pair: &[u32]| -> u32 {
        let hi = (pair[0] & 0xffff) << 16;
        let lo = (pair[1] & 0xffff) as i16 as i32;
        (hi as i32 + lo) as u32
    };
    assert_eq!(resolve(&find_pair), FIND_TABLE_VA);
    assert_eq!(resolve(&repl_pair), REPLACE_TABLE_VA);
    assert_eq!(
        overlay_word(&overlay, HOOK_STRIDE_VA),
        0x000D_6840,
        "sll t5,t5,1"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_BOUND_VA),
        0x28A2_000A,
        "ten rows"
    );
    assert_eq!(
        overlay_word(&overlay, HOOK_EXIT_SEED_VA),
        0x2405_000A,
        "li a1,10 - a match still exits the widened row loop"
    );

    // 3. The battle-load stub reads the annexed block to 0x801FD000 and returns
    //    to battle init. Decode the pieces the stub's correctness rests on.
    assert_eq!(scus_word(&scus, LOAD_HOOK_VA), j(STUB_VA));
    let stub = scus_words(&scus, STUB_VA, pack::STUB_WORDS as usize);
    assert_eq!(stub[0] & 0xffff, BLOCK_SECTORS, "a0 = sector count");
    let lba = ((stub[1] & 0xffff) << 16) | (stub[2] & 0xffff);
    assert_eq!(lba, report.block_lba, "a1 = the annexed block's LBA");
    let dest = ((stub[3] & 0xffff) << 16) | (stub[5] & 0xffff);
    assert_eq!(dest, BLOCK_VA, "a2 = 0x801FD000");
    assert_eq!(
        stub[4],
        (0x03 << 26) | ((0x8005_E4D4 >> 2) & 0x03ff_ffff),
        "jal FUN_8005E4D4"
    );
    assert_eq!(stub[9], 0x3C06_1F80, "replays lui a2,0x1f80");
    assert_eq!(stub[10], j(pack::LOAD_RET_VA), "returns to battle init");
    assert_eq!(
        stub[11], 0x34C6_0314,
        "replays ori a2,a2,0x314 in the delay slot"
    );
}

#[test]
fn every_other_byte_is_untouched_and_the_disc_still_parses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let clean = DiscPatcher::open(disc.clone()).expect("open clean");
    let scus0 = clean.read_named_file("SCUS_942.54").expect("SCUS");
    let ov0 = clean.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");

    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::inject_super_arts_pack(&mut patcher).expect("inject");
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let overlay = patcher.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");

    // Overlay: only the ten planned words moved.
    let mut allowed_ov: Vec<std::ops::Range<usize>> = Vec::new();
    for (va, words) in [
        (HOOK_STRIDE_VA, 1usize),
        (HOOK_FIND_HI_VA, 2),
        (HOOK_REPL_HI_VA, 2),
        (HOOK_BOUND_VA, 1),
        (HOOK_EXIT_SEED_VA, 1),
        (HOOK_APPLIER_VA, 1),
        (HOOK_QUEUE_VA, 2),
    ] {
        let off = (va - OVERLAY_BASE_VA) as usize;
        allowed_ov.push(off..off + words * 4);
    }
    assert_eq!(overlay.len(), ov0.len(), "overlay entry size unchanged");
    for (i, (&a, &b)) in ov0.iter().zip(overlay.iter()).enumerate() {
        if !allowed_ov.iter().any(|r| r.contains(&i)) {
            assert_eq!(a, b, "PROT 0898 byte {i:#x} changed outside a planned edit");
        }
    }

    // SCUS: the three hook sites plus the arena claim.
    let off_of = |va: u32| file_offset_for_va(&scus0, va).expect("resolve");
    let allowed_sc = [
        off_of(HOOK_BANNER_VA)..off_of(HOOK_BANNER_VA) + 4,
        off_of(HOOK_KEEP_NAME_VA)..off_of(HOOK_KEEP_NAME_VA) + 8,
        off_of(LOAD_HOOK_VA)..off_of(LOAD_HOOK_VA) + 4,
        off_of(STUB_VA)..off_of(STUB_VA) + (pack::ARENA_USED_END_VA - STUB_VA) as usize,
    ];
    assert_eq!(scus.len(), scus0.len(), "SCUS size unchanged");
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !allowed_sc.iter().any(|r| r.contains(&i)) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside a planned edit");
        }
    }

    // The disc still parses off the patched image, and the sectors stay valid.
    assert!(
        !apply::current_drops(&patcher)
            .expect("drops decode")
            .is_empty(),
        "monster archive still readable"
    );
    assert!(
        apply::current_move_powers(&patcher)
            .expect("move powers decode")
            .is_some(),
        "patched battle-action overlay still parses"
    );
    read_file_in_image(patcher.image(), "SCUS_942.54")
        .expect("patched image re-reads (sectors stay valid)");
}

#[test]
fn injection_is_byte_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::inject_super_arts_pack(&mut a).unwrap();
    apply::inject_super_arts_pack(&mut b).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "the patch is byte-identical run to run"
    );
}

#[test]
fn planner_refuses_an_unrecognized_build_or_a_claimed_arena() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let mut overlay = patcher.read_entry(OVERLAY_PROT_INDEX).expect("PROT 0898");
    assert!(SuperArtsPackInjection::plan(&scus, &overlay, 1).is_ok());

    // A moved hook site is refused, not patched around.
    let off = (HOOK_APPLIER_VA - OVERLAY_BASE_VA) as usize;
    overlay[off] ^= 0xFF;
    assert!(
        SuperArtsPackInjection::plan(&scus, &overlay, 1).is_err(),
        "a changed hook word must refuse"
    );
    overlay[off] ^= 0xFF;

    // A different Super Art trigger table (another build, or another pack) is
    // refused too - the pack carries the retail rows and they must match.
    let mut ov_rows = overlay.clone();
    let row = (pack::RETAIL_FIND_VA - OVERLAY_BASE_VA) as usize;
    ov_rows[row + 1] ^= 0xFF;
    assert!(
        SuperArtsPackInjection::plan(&scus, &ov_rows, 1).is_err(),
        "a disc whose own trigger rows differ must refuse"
    );

    // An arena already claimed by another injection is refused.
    let mut scus_dirty = scus.clone();
    let stub_off = file_offset_for_va(&scus, STUB_VA).expect("resolve stub");
    scus_dirty[stub_off + 4] = 0x01;
    assert!(
        SuperArtsPackInjection::plan(&scus_dirty, &overlay, 1).is_err(),
        "a non-zero arena must refuse"
    );

    // And an unplaced block (no annex LBA) is refused.
    assert!(SuperArtsPackInjection::plan(&scus, &overlay, 0).is_err());
}
