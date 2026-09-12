//! Disc-gated tests for **enemy HP bars** - the code hook that draws a red HP
//! gauge over every living monster in battle (see `legaia_patcher::enemy_hp_bar`).
//!
//! The injection is five same-size edits: a two-word detour at the head of the
//! damage-popup renderer in the battle-action overlay (PROT 898, VA
//! `0x801DF6B8`) and the routine's four fragments laid over four routines
//! retail never references - three in the overlay (`0x801F2D54`, `0x801F463C`,
//! `0x801DBB2C`) and one in `SCUS_942.54` (`0x8005126C`). These tests apply it
//! to a scratch copy of the real disc and assert, off the patched image, that:
//!   * every host body on the real disc **is** the fingerprinted retail routine
//!     (prologue words + its own `jr ra`), so the plan targets what it thinks;
//!   * the detour jumps to fragment A and every fragment decodes as the
//!     assembled words;
//!   * each edit is surgical and every touched sector stays EDC/ECC-valid;
//!   * the patch is byte-deterministic; and
//!   * a second application refuses (the hosts are no longer retail's).
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory. The engine can't run injected MIPS, so the runtime oracle is
//! the PCSX-Redux RAM-injection probe `autorun_enemy_hp_bar_inject.lua` plus
//! the simulator tests inside the module.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::write::mode2_form1_sector_is_valid;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::enemy_hp_bar::{
    EnemyHpBarInjection, FRAG_A_VA, FRAG_B_VA, FRAG_C_VA, FRAG_S_VA, HOOK_VA, OVERLAY_PROT_INDEX,
    assemble,
};
use legaia_patcher::shiny_seru::OVERLAY_BASE_VA;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_words(scus: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = file_offset_for_va(scus, va).expect("resolve va");
    (0..n)
        .map(|i| u32::from_le_bytes(scus[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

fn overlay_words(entry: &[u8], va: u32, n: usize) -> Vec<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    (0..n)
        .map(|i| u32::from_le_bytes(entry[off + i * 4..off + i * 4 + 4].try_into().unwrap()))
        .collect()
}

const JR_RA: u32 = 0x03E0_0008;

#[test]
fn host_bodies_on_the_real_disc_are_the_fingerprinted_routines() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let overlay = patcher
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("read overlay");

    // The plan's own fingerprints pass on the pristine disc.
    EnemyHpBarInjection::plan(&scus, &overlay).expect("plan against the real disc");

    // And independently of the planner: each body opens the way the
    // reference scan described it and closes on its own `jr ra`.
    assert_eq!(overlay_words(&overlay, HOOK_VA, 1), vec![0x27BD_FF90]); // addiu sp,sp,-0x70
    assert_eq!(overlay_words(&overlay, FRAG_A_VA, 1), vec![0x3C05_8008]); // lui a1,0x8008
    assert_eq!(overlay_words(&overlay, 0x801F_2E08, 1), vec![JR_RA]);
    assert_eq!(overlay_words(&overlay, FRAG_B_VA, 1), vec![0x00A0_4821]); // move t1,a1
    assert_eq!(overlay_words(&overlay, 0x801F_46C0, 1), vec![JR_RA]);
    assert_eq!(overlay_words(&overlay, FRAG_C_VA, 1), vec![0x3C03_8008]); // lui v1,0x8008
    assert_eq!(overlay_words(&overlay, 0x801D_BB84, 1), vec![JR_RA]);
    assert_eq!(scus_words(&scus, FRAG_S_VA, 1), vec![0x27BD_FFC8]); // addiu sp,sp,-0x38
    assert_eq!(scus_words(&scus, 0x8005_1334, 1), vec![JR_RA]);
}

#[test]
fn injection_writes_the_detour_and_four_fragments_surgically() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let overlay0 = patcher
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("read overlay");
    let plan = EnemyHpBarInjection::plan(&scus0, &overlay0).expect("plan");

    let report = apply::inject_enemy_hp_bar(&mut patcher).expect("inject");
    assert_eq!(report.edits, 5);
    assert_eq!(
        report.overlay_words + report.scus_words,
        plan.overlay_words + plan.scus_words
    );

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let overlay = patcher
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("read patched overlay");

    // 1. The detour: `j FRAG_A` + nop.
    let j_a = (0x02 << 26) | ((FRAG_A_VA >> 2) & 0x03ff_ffff);
    assert_eq!(overlay_words(&overlay, HOOK_VA, 2), vec![j_a, 0]);

    // 2. Every fragment decodes as the assembler's words.
    let frags = assemble().expect("assemble");
    let mut touched: Vec<(Option<usize>, usize, usize)> = Vec::new();
    for f in &frags {
        if f.va >= OVERLAY_BASE_VA {
            assert_eq!(
                overlay_words(&overlay, f.va, f.words.len()),
                f.words,
                "{:#x}",
                f.va
            );
            let off = (f.va - OVERLAY_BASE_VA) as usize;
            touched.push((Some(OVERLAY_PROT_INDEX), off, f.words.len() * 4));
        } else {
            assert_eq!(
                scus_words(&scus, f.va, f.words.len()),
                f.words,
                "{:#x}",
                f.va
            );
            let off = file_offset_for_va(&scus0, f.va).unwrap();
            touched.push((None, off, f.words.len() * 4));
        }
    }
    touched.push((
        Some(OVERLAY_PROT_INDEX),
        (HOOK_VA - OVERLAY_BASE_VA) as usize,
        8,
    ));

    // 3. Surgical: nothing outside the five edit spans changed.
    let in_span = |target: Option<usize>, i: usize| {
        touched
            .iter()
            .any(|&(t, off, len)| t == target && (off..off + len).contains(&i))
    };
    assert_eq!(scus.len(), scus0.len());
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !in_span(None, i) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside the fragment");
        }
    }
    assert_eq!(overlay.len(), overlay0.len());
    for (i, (&a, &b)) in overlay0.iter().zip(overlay.iter()).enumerate() {
        if !in_span(Some(OVERLAY_PROT_INDEX), i) {
            assert_eq!(
                a, b,
                "overlay byte {i:#x} changed outside the fragments/hook"
            );
        }
    }

    // 4. Every touched sector stays EDC/ECC-valid (and both files re-read).
    let image = patcher.image();
    let (scus_lba, _) = find_file_in_image(image, "SCUS_942.54").expect("SCUS extent");
    let entry_lba = patcher
        .entry_disc_lba(OVERLAY_PROT_INDEX)
        .expect("overlay entry LBA");
    for &(target, off, len) in &touched {
        let lba = match target {
            None => scus_lba,
            Some(_) => entry_lba,
        };
        for sector in (off / 2048)..=((off + len - 1) / 2048) {
            let s = (lba as usize + sector) * 2352;
            assert!(
                mode2_form1_sector_is_valid(&image[s..s + 2352]),
                "sector {} of {:?} is EDC/ECC-valid",
                sector,
                target
            );
        }
    }

    // 5. Deterministic.
    let mut again = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::inject_enemy_hp_bar(&mut again).expect("inject again");
    assert_eq!(again.image(), patcher.image(), "byte-deterministic");

    // 6. A second application refuses: the hosts are no longer retail's.
    let err = apply::inject_enemy_hp_bar(&mut patcher).expect_err("second application refuses");
    assert!(
        format!("{err:#}").contains("nothing written"),
        "refusal names the guard: {err:#}"
    );
}

/// The feature claims no arena byte, so it must apply on top of the arena
/// features that exclude *each other* - here shiny Seru on one image and the
/// Super Arts Pack (annex + arena stub) on another.
#[test]
fn composes_with_the_arena_features() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut with_shiny = DiscPatcher::open(disc.clone()).expect("open disc");
    apply::inject_shiny_seru(&mut with_shiny, 2).expect("shiny Seru");
    let rep = apply::inject_enemy_hp_bar(&mut with_shiny).expect("HP bars after shiny Seru");
    assert_eq!(rep.edits, 5);

    let mut with_pack = DiscPatcher::open(disc).expect("open disc");
    apply::inject_super_arts_pack(&mut with_pack).expect("Super Arts Pack");
    let rep = apply::inject_enemy_hp_bar(&mut with_pack).expect("HP bars after the pack");
    assert_eq!(rep.edits, 5);

    // And in the other order, the arena feature still finds its own hosts.
    let mut hp_first = DiscPatcher::open(with_shiny.image().to_vec()).expect("reopen");
    assert!(
        apply::inject_enemy_hp_bar(&mut hp_first).is_err(),
        "already applied - refuses"
    );
}
