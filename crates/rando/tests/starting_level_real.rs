//! Disc-gated end-to-end test for the starting-level randomizer: rewrite the
//! new-game seed routine's level / current-XP / next-threshold instructions and
//! slot 0's stat template in `SCUS_942.54` on a scratch copy of the disc, then
//! re-decode straight off the patched image and confirm the edit is faithful — the
//! level literal decodes back to the requested level, the seed instructions carry
//! the planned XP values with the right register/opcode shape, the stats are the
//! level's growth-curve values (and strictly above the vanilla level-1 template),
//! the lead character's name and the `sw $ra` after the threshold literal are
//! untouched, the image is the same size, every touched SCUS sector stays
//! EDC/ECC-valid, and the patch is byte-deterministic. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::new_game::{
    CURRENT_XP_PRELOAD_VA, CURRENT_XP_STORE_VA, StartingParty, party_template_file_offset,
    scus_file_offset, starting_xp_seed_file_offset,
};
use legaia_iso::iso9660::{find_file_in_image, read_file_in_image};
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::starting_level::{
    DEFAULT_STARTING_LEVEL, MAX_STARTING_LEVEL, MIN_STARTING_LEVEL, plan,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn slot0_stats(scus: &[u8]) -> [u16; 8] {
    let p = StartingParty::from_scus(scus).expect("template");
    let v = p.member(0).expect("slot 0");
    [
        v.hp_max, v.mp_max, v.agl, v.atk, v.udf, v.ldf, v.spd, v.intel,
    ]
}

#[test]
fn starting_level_round_trips_on_disc() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // Vanilla baseline: retail begins at level 1.
    let base = DiscPatcher::open(original.clone()).expect("open");
    assert_eq!(
        apply::current_starting_level(&base).expect("read level"),
        1,
        "retail new game starts at level 1"
    );
    let scus0 = read_file_in_image(&original, "SCUS_942.54").expect("SCUS present");
    let vanilla_stats = slot0_stats(&scus0);
    let xp_off = starting_xp_seed_file_offset(&scus0).expect("xp seed offset");
    let tmpl_off = party_template_file_offset(&scus0).expect("template offset");
    let cur_store_off = scus_file_offset(&scus0, CURRENT_XP_STORE_VA).expect("current-xp store");
    let cur_pre_off = scus_file_offset(&scus0, CURRENT_XP_PRELOAD_VA).expect("current-xp preload");
    // Bytes that must survive: the `sw $ra` right after the threshold literal, and
    // the lead character's 10-byte name right after slot 0's 16 stat bytes.
    let after_xp = scus0[xp_off + 4..xp_off + 8].to_vec();
    let slot0_name = scus0[tmpl_off + 16..tmpl_off + 26].to_vec();

    let level = DEFAULT_STARTING_LEVEL;
    let want = plan(&scus0, level).expect("plan");

    let mut patcher = DiscPatcher::open(original.clone()).expect("open");
    let report = apply::apply_starting_level(&mut patcher, level).expect("apply");
    assert_eq!(report.level, level);
    assert_eq!(report.stats, want.stats);
    assert_eq!(report.current_xp, want.current_xp);
    assert_eq!(report.next_threshold, want.next_threshold);

    // Re-decode off the patched image: the seeded experience must read back as the
    // requested level, and the template stats as the planned level-N stats.
    assert_eq!(
        apply::current_starting_level(&patcher).expect("read patched level"),
        level,
        "patched experience decodes back to the requested level"
    );
    let patched_scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS");
    assert_eq!(
        slot0_stats(&patched_scus),
        want.stats,
        "template carries the level-N stats"
    );

    // The seed instructions carry the planned values + register/opcode shape.
    let word = |o: usize| u32::from_le_bytes(patched_scus[o..o + 4].try_into().unwrap());
    assert_eq!(
        word(xp_off) & 0xffff,
        want.next_threshold as u32,
        "+0x4 next-level threshold literal"
    );
    // The current-experience value rides in `addiu $t0, $zero, imm` and the store is
    // `sw $t0, 0x5c8($s0)` (record +0x0).
    assert_eq!(
        word(cur_pre_off) & 0xffff,
        want.current_xp as u32,
        "current-experience preload immediate"
    );
    assert_eq!(word(cur_pre_off) >> 16, 0x2408, "preload targets $t0");
    assert_eq!(
        word(cur_store_off),
        0xAE08_05C8,
        "sw $t0, 0x5c8($s0) -> record +0x0"
    );
    // The current-experience value is strictly inside level N's XP band (above
    // reach(N), below the next threshold) so the derived level is unambiguous.
    assert!(
        (want.current_xp as u32) < (want.next_threshold as u32),
        "experience {} must sit below the next threshold {}",
        want.current_xp,
        want.next_threshold
    );

    // The level-N stats are strictly above the vanilla level-1 template (a real
    // head start), and HP in particular grew a lot.
    for (i, (&n, &v)) in want.stats.iter().zip(&vanilla_stats).enumerate() {
        assert!(n >= v, "stat {i} must not shrink ({n} < {v})");
    }
    assert!(
        want.stats[0] > vanilla_stats[0],
        "level {level} HP {} must exceed vanilla {}",
        want.stats[0],
        vanilla_stats[0]
    );

    // The lead character's name and the instruction after the XP literal are
    // untouched (the edits are confined to the stat bytes + the one literal).
    assert_eq!(
        &patched_scus[tmpl_off + 16..tmpl_off + 26],
        &slot0_name[..],
        "slot 0 name preserved"
    );
    assert_eq!(
        &patched_scus[xp_off + 4..xp_off + 8],
        &after_xp[..],
        "the sw $ra after the XP literal is preserved"
    );

    // Same image size; both touched SCUS sectors stay EDC/ECC-valid.
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "image size unchanged"
    );
    let (scus_lba, _) = find_file_in_image(patcher.image(), "SCUS_942.54").unwrap();
    for off in [xp_off, cur_pre_off, cur_store_off, tmpl_off] {
        let sb = (scus_lba as usize + off / USER_DATA_SIZE) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&patcher.image()[sb..sb + SECTOR_SIZE]),
            "patched sector must be EDC/ECC-valid"
        );
    }

    // Determinism.
    let mut patcher2 = DiscPatcher::open(original).expect("open");
    apply::apply_starting_level(&mut patcher2, level).expect("apply");
    assert!(patcher2.image() == patcher.image(), "deterministic");

    eprintln!(
        "starting-level {level}: current_xp={} next_threshold={} stats={:?}",
        report.current_xp, report.next_threshold, report.stats
    );
}

/// Every level in range round-trips: patch it, read the XP literal back, and the
/// derived level must equal what was requested (the XP seed lands inside the
/// level's band, never off-by-one into a neighbour).
#[test]
fn every_level_in_range_decodes_back_exactly() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for level in MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL {
        let mut patcher = DiscPatcher::open(original.clone()).expect("open");
        apply::apply_starting_level(&mut patcher, level).expect("apply");
        let got = apply::current_starting_level(&patcher).expect("read level");
        assert_eq!(got, level, "level {level} must decode back to itself");
    }
}
