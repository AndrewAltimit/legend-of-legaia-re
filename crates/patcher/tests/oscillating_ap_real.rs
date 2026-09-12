//! Disc-gated oracle for **oscillating AP costs** (see
//! `legaia_patcher::oscillating_ap`): every battle each Tactical Art is dealt
//! onto the cost side (retail) or the grant side (admitted at any AP, adds the
//! AP it would have cost, deals a fraction of its damage). Four same-size
//! detours into PROT 0898 (the arts AP override's `0x801EF410` / `0x801EF490` /
//! `0x801EF988` plus the strike-damage site `0x801EDA10`), one into the SCUS
//! battle-loader setup site `0x80051A20`, and the routines across the four
//! verified-dead SCUS regions (`ARENA1` guard + debit, `ARENA2` refund, `SLOT6`
//! the roll, `SCUS_GAP` the damage routine + side table + counter).
//!
//! These apply it to a scratch copy of the real disc and assert, off the
//! patched image, that every hosted region was all-zero pre-patch and every
//! fingerprinted retail word is the US build; each detour became a `j routine`
//! plus a `nop`; the routines land exactly where the plan says and the side
//! table + counter stay zero on disc; every byte outside the planned edits is
//! untouched; the disc still parses and stays EDC/ECC-valid; a fixed input is
//! byte-deterministic; re-applying is refused (idempotent); the feature is
//! refused on top of shiny-Seru and the arts AP override (shared regions), and
//! they are refused on top of it; and an unrecognized build is refused. Gates on
//! `LEGAIA_DISC_BIN`; skips and passes when unset.
//!
//! HONESTY GATE: this proves only WHERE the bytes land, never in-game
//! behaviour. A live battle playtest (a grant-side art admits at 0 AP, raises
//! the gauge by its retail cost, deals the configured fraction; a cost-side art
//! is retail; the deal changes between battles) is still required before
//! shipping.

use legaia_art::queue::Character;
use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::arts_ap_grant::{
    ApMode, ArtApSpec, HOOK_A_VA, HOOK_B_VA, HOOK_C_VA, HOOK_D_VA, OVERLAY_BASE_VA,
    OVERLAY_PROT_INDEX,
};
use legaia_patcher::arts_power::parse_combo;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::oscillating_ap::{BITS_LEN, HOOK_DMG_VA, HOOK_LIST_VA, OscillatingApInjection};
use legaia_patcher::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, HOOK_SETUP_VA, SCUS_GAP_END_VA,
    SCUS_GAP_VA, SLOT6_END_VA, SLOT6_VA,
};

const PCT: u8 = 20;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

fn scus_word(scus: &[u8], va: u32) -> u32 {
    let off = file_offset_for_va(scus, va).unwrap();
    u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
}

#[test]
fn hosted_regions_are_zero_and_fingerprints_are_the_us_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    for (va, end) in [
        (ARENA1_VA, ARENA1_END_VA),
        (ARENA2_VA, ARENA2_END_VA),
        (SLOT6_VA, SLOT6_END_VA),
        (SCUS_GAP_VA, SCUS_GAP_END_VA),
    ] {
        let off = file_offset_for_va(&scus, va).unwrap();
        assert!(
            scus[off..off + (end - va) as usize].iter().all(|&b| b == 0),
            "{va:#x}..{end:#x} is all-zero dead space pre-patch"
        );
    }
    // Independently transcribed retail words (not the module's constants).
    assert_eq!(
        scus_word(&scus, HOOK_SETUP_VA),
        0x3C02_8008,
        "lui v0,0x8008"
    );
    assert_eq!(
        scus_word(&scus, HOOK_SETUP_VA + 4),
        0x3C03_8008,
        "lui v1,0x8008"
    );
    assert_eq!(
        scus_word(&scus, HOOK_SETUP_VA + 8),
        0x9063_BD60,
        "lbu v1,-0x42a0(v1)"
    );
    assert_eq!(
        scus_word(&scus, 0x8004_BC80),
        0x2442_F324,
        "addiu v0,v0,-0xcdc - the anim commit's (id-0x10)*0xD0+0x24 fold"
    );
    assert_eq!(
        scus_word(&scus, 0x8004_BC84),
        0xAC82_0000,
        "sw v0,0x0(a0) - record0[q*4], q the staging slot"
    );
    assert_eq!(
        scus_word(&scus, 0x8004_B710),
        0x8C42_0058,
        "lw v0,0x58(v0) - bank = record0[+0x58]"
    );
    assert_eq!(
        scus_word(&scus, 0x8004_B718),
        0x2450_0004,
        "addiu s0,v0,0x4 - bank + 4"
    );
    // The arts-list renderer's AP read the list detour replaces, its record
    // cursor and the row compare that pin the -8 / -7 / -6 offsets.
    assert_eq!(
        scus_word(&scus, 0x8003_4460),
        0x24B5_0008,
        "addiu s5,a1,0x8"
    );
    assert_eq!(
        scus_word(&scus, 0x8003_4478),
        0x92A2_FFF9,
        "lbu v0,-0x7(s5)"
    );
    assert_eq!(
        scus_word(&scus, 0x8003_44D4),
        0x8C42_06C0,
        "lw v0,0x6c0(v0)"
    );
    assert_eq!(
        scus_word(&scus, HOOK_LIST_VA),
        0x92B0_FFFA,
        "lbu s0,-0x6(s5)"
    );
    assert_eq!(
        scus_word(&scus, HOOK_LIST_VA + 4),
        0x3042_0800,
        "andi v0,v0,0x800"
    );
    assert_eq!(
        scus_word(&scus, HOOK_LIST_VA + 8),
        0x1040_0002,
        "beq v0,zero,+2 - the return site"
    );
    assert_eq!(
        scus_word(&scus, 0x8003_44E8),
        0x0010_8042,
        "srl s0,s0,0x1 - the halving the read feeds"
    );

    let patcher = DiscPatcher::open(disc).expect("open");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();
    assert_eq!(
        overlay_word(&ov, HOOK_A_VA),
        0x94A2_0170,
        "A lhu v0,0x170(a1)"
    );
    assert_eq!(overlay_word(&ov, HOOK_A_VA + 4), 0x0000_7812, "A mflo t7");
    assert_eq!(
        overlay_word(&ov, HOOK_B_VA),
        0x2665_FFF5,
        "B addiu a1,s3,-0xb"
    );
    assert_eq!(
        overlay_word(&ov, HOOK_C_VA),
        0x9462_0170,
        "C lhu v0,0x170(v1)"
    );
    assert_eq!(
        overlay_word(&ov, HOOK_D_VA),
        0x9462_0170,
        "D lhu v0,0x170(v1)"
    );
    assert_eq!(
        overlay_word(&ov, 0x801E_F340),
        0x91C2_0000,
        "lbu v0,0x0(t6)"
    );
    // The damage site and the 9999 cap in front of it.
    assert_eq!(overlay_word(&ov, 0x801E_DA00), 0x0070_102B, "sltu v0,v1,s0");
    assert_eq!(
        overlay_word(&ov, 0x801E_DA04),
        0x1040_0002,
        "beq v0,zero,+2"
    );
    assert_eq!(overlay_word(&ov, 0x801E_DA08), 0x3C02_801D, "lui v0,0x801d");
    assert_eq!(overlay_word(&ov, 0x801E_DA0C), 0x0060_8021, "move s0,v1");
    assert_eq!(
        overlay_word(&ov, HOOK_DMG_VA),
        0x2443_9370,
        "addiu v1,v0,-0x6c90"
    );
    assert_eq!(
        overlay_word(&ov, HOOK_DMG_VA + 4),
        0x3282_00FF,
        "andi v0,s4,0xff"
    );
    assert_eq!(
        overlay_word(&ov, HOOK_DMG_VA + 8),
        0x0002_1080,
        "sll v0,v0,0x2 - the return site"
    );
    // The kernel's spill of its entry argument, which the damage routine reads.
    assert_eq!(
        overlay_word(&ov, 0x801E_C418),
        0xAFA5_0054,
        "sw a1,0x54(sp)"
    );
    assert_eq!(
        overlay_word(&ov, 0x801E_C3EC),
        0x27BD_FFB0,
        "addiu sp,sp,-0x50 - one frame"
    );
}

#[test]
fn injection_lands_exactly_and_is_surgical() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus0 = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let ov0 = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    let plan = OscillatingApInjection::plan(&scus0, &ov0, PCT).expect("plan");
    let report = apply::inject_oscillating_ap(&mut patcher, PCT).expect("inject");
    assert_eq!(report.damage_pct, PCT);
    assert_eq!(report.roll_va, plan.roll_va);
    assert_eq!(report.damage_va, plan.damage_va);
    assert_eq!(report.bits_va, plan.bits_va);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    // The four 0898 detours + the SCUS setup detour: `j routine` + nop each.
    let check_detour = |w: u32, w1: u32, target: u32, what: &str| {
        assert_eq!(w >> 26, 0x02, "{what} became a `j`");
        assert_eq!(
            (w & 0x03ff_ffff) << 2,
            target & 0x0fff_ffff,
            "{what}: j -> routine"
        );
        assert_eq!(w1, 0, "{what}: delay slot is nop");
    };
    for (site, target, what) in [
        (HOOK_A_VA, plan.guard_va, "site A"),
        (HOOK_C_VA, plan.debit_va, "site C"),
        (HOOK_D_VA, plan.refund_va, "site D"),
        (HOOK_DMG_VA, plan.damage_va, "damage site"),
    ] {
        check_detour(
            overlay_word(&ov, site),
            overlay_word(&ov, site + 4),
            target,
            what,
        );
    }
    check_detour(
        scus_word(&scus, HOOK_SETUP_VA),
        scus_word(&scus, HOOK_SETUP_VA + 4),
        plan.roll_va,
        "setup site",
    );
    check_detour(
        scus_word(&scus, HOOK_LIST_VA),
        scus_word(&scus, HOOK_LIST_VA + 4),
        plan.list_va,
        "list site",
    );
    assert_eq!(overlay_word(&ov, HOOK_B_VA), 0x2665_FFF5, "site B intact");
    assert_eq!(
        overlay_word(&ov, 0x801E_F340),
        0x91C2_0000,
        "t6 read intact"
    );
    assert_eq!(
        overlay_word(&ov, 0x801E_DA0C),
        0x0060_8021,
        "the cap's last word intact"
    );

    // Layout: one piece per region, the roll filling slot 6 exactly, the side
    // table + counter inside the gap and still zero on disc.
    assert_eq!(plan.leaf_va, ARENA1_VA);
    assert!(plan.guard_va > plan.leaf_va && plan.debit_va > plan.guard_va);
    assert!(plan.list_va > plan.debit_va && plan.list_va < ARENA1_END_VA);
    assert_eq!(plan.refund_va, ARENA2_VA);
    assert_eq!(plan.roll_va, SLOT6_VA);
    assert_eq!(plan.damage_va, SCUS_GAP_VA);
    assert!(plan.bits_va > plan.damage_va && plan.counter_va + 4 <= SCUS_GAP_END_VA);
    let roll_off = file_offset_for_va(&scus, SLOT6_VA).unwrap();
    let roll_edit = plan
        .edits
        .iter()
        .find(|e| e.prot_index.is_none() && e.file_off == roll_off)
        .expect("the roll's edit");
    assert_eq!(
        roll_edit.bytes.len() as u32,
        SLOT6_END_VA - SLOT6_VA,
        "the roll fills slot 6 exactly"
    );
    let bits_off = file_offset_for_va(&scus, plan.bits_va).unwrap();
    assert!(
        scus[bits_off..bits_off + BITS_LEN + 4]
            .iter()
            .all(|&b| b == 0),
        "side table + counter are zero on disc (the roll fills them per battle)"
    );

    // Every planned edit landed byte-exact; nothing else moved.
    let mut scus_edits: Vec<(usize, &[u8])> = Vec::new();
    let mut ov_edits: Vec<(usize, &[u8])> = Vec::new();
    for e in &plan.edits {
        match e.prot_index {
            None => scus_edits.push((e.file_off, &e.bytes)),
            Some(i) if i == OVERLAY_PROT_INDEX => ov_edits.push((e.file_off, &e.bytes)),
            Some(i) => panic!("unexpected PROT index {i}"),
        }
    }
    assert_eq!(scus_edits.len(), 9, "setup + list detours, seven routines");
    assert_eq!(ov_edits.len(), 4, "four 0898 detours");
    for (off, b) in &scus_edits {
        assert_eq!(
            &scus[*off..*off + b.len()],
            *b,
            "SCUS edit at {off:#x} landed"
        );
    }
    for (off, b) in &ov_edits {
        assert_eq!(
            &ov[*off..*off + b.len()],
            *b,
            "0898 edit at {off:#x} landed"
        );
    }
    let in_any = |edits: &[(usize, &[u8])], i: usize| {
        edits.iter().any(|&(o, b)| (o..o + b.len()).contains(&i))
    };
    assert_eq!(scus.len(), scus0.len());
    for (i, (&a, &b)) in scus0.iter().zip(scus.iter()).enumerate() {
        if !in_any(&scus_edits, i) {
            assert_eq!(a, b, "SCUS byte {i:#x} changed outside a planned edit");
        }
    }
    assert_eq!(ov.len(), ov0.len());
    for (i, (&a, &b)) in ov0.iter().zip(ov.iter()).enumerate() {
        if !in_any(&ov_edits, i) {
            assert_eq!(a, b, "0898 byte {i:#x} changed outside a planned edit");
        }
    }
    // The menu's AP bytes are NOT touched: the deal is per battle, the list is
    // static, so it keeps showing retail's numbers.
    let before = legaia_art::arts_table::parse_from_scus(&scus0).expect("stock arts table");
    let after = legaia_art::arts_table::parse_from_scus(&scus).expect("patched arts table");
    assert_eq!(before, after, "arts-name table untouched");

    // The disc still parses + re-opens (EDC/ECC re-encoded on every touched sector).
    DiscPatcher::open(patcher.image().to_vec()).expect("patched image re-opens");
    read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS re-reads");
    patcher
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("0898 re-reads");
}

#[test]
fn injection_is_byte_deterministic_and_idempotent() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).expect("open a");
    let mut b = DiscPatcher::open(disc).expect("open b");
    apply::inject_oscillating_ap(&mut a, PCT).unwrap();
    apply::inject_oscillating_ap(&mut b, PCT).unwrap();
    assert_eq!(a.image(), b.image(), "a fixed input is byte-identical");

    let before = a.image().to_vec();
    assert!(
        apply::inject_oscillating_ap(&mut a, PCT).is_err(),
        "re-injecting into the now-live regions is refused"
    );
    assert_eq!(
        a.image(),
        &before[..],
        "a refused re-apply leaves the image unchanged"
    );
    // A different percent on the already-patched image is refused the same way.
    let mut c = DiscPatcher::open(before.clone()).expect("open c");
    assert!(
        apply::inject_oscillating_ap(&mut c, 50).is_err(),
        "still refused on a patched image"
    );
}

#[test]
fn mutually_exclusive_with_the_other_arena_features() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ap_specs = vec![ArtApSpec {
        character: Some(Character::Vahn),
        combo: parse_combo("RDLDL").unwrap(),
        mode: ApMode::Grant(10),
    }];
    // Whichever runs second finds a region no longer all-zero and refuses.
    let mut p = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_shiny_seru(&mut p, legaia_patcher::shiny_seru::DEFAULT_PCT).expect("shiny");
    assert!(
        apply::inject_oscillating_ap(&mut p, PCT).is_err(),
        "refused after shiny-Seru"
    );

    let mut q = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_arts_ap_grant(&mut q, &ap_specs).expect("ap override");
    assert!(
        apply::inject_oscillating_ap(&mut q, PCT).is_err(),
        "refused after the AP override"
    );

    let mut r = DiscPatcher::open(disc).expect("open");
    apply::inject_oscillating_ap(&mut r, PCT).expect("oscillating");
    assert!(
        apply::inject_shiny_seru(&mut r, legaia_patcher::shiny_seru::DEFAULT_PCT).is_err(),
        "shiny-Seru refused after oscillating AP"
    );
    assert!(
        apply::inject_arts_ap_grant(&mut r, &ap_specs).is_err(),
        "the AP override refused after oscillating AP"
    );
}

#[test]
fn planner_refuses_bad_input_and_unrecognized_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let patcher = DiscPatcher::open(disc).expect("open");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    assert!(
        OscillatingApInjection::plan(&scus, &ov, 0).is_ok(),
        "0% is a valid setting"
    );
    assert!(
        OscillatingApInjection::plan(&scus, &ov, 100).is_ok(),
        "100% is a valid setting"
    );
    assert!(
        OscillatingApInjection::plan(&scus, &ov, 101).is_err(),
        "over 100% refused"
    );

    // Corrupt each fingerprinted site in turn -> refuse.
    for va in [
        HOOK_A_VA,
        HOOK_C_VA,
        HOOK_D_VA,
        HOOK_DMG_VA,
        0x801E_DA0C,
        0x801E_F340,
    ] {
        let mut bad = ov.clone();
        let off = (va - OVERLAY_BASE_VA) as usize;
        bad[off] ^= 0xFF;
        assert!(
            OscillatingApInjection::plan(&scus, &bad, PCT).is_err(),
            "corrupted 0898 word at {va:#x} is refused"
        );
    }
    for va in [
        HOOK_SETUP_VA,
        HOOK_SETUP_VA + 4,
        HOOK_LIST_VA,
        HOOK_LIST_VA + 4,
        0x8004_B710,
        0x8004_B718,
        0x8004_BC80,
        0x8004_BC84,
        0x8003_4460,
    ] {
        let mut bad = scus.clone();
        let off = file_offset_for_va(&scus, va).unwrap();
        bad[off] ^= 0xFF;
        assert!(
            OscillatingApInjection::plan(&bad, &ov, PCT).is_err(),
            "corrupted SCUS word at {va:#x} is refused"
        );
    }
    // A dirty region -> refuse.
    for va in [ARENA1_VA + 8, ARENA2_VA, SLOT6_VA + 60, SCUS_GAP_VA + 0x90] {
        let mut dirty = scus.clone();
        let off = file_offset_for_va(&scus, va).unwrap();
        dirty[off] = 0x5A;
        assert!(
            OscillatingApInjection::plan(&dirty, &ov, PCT).is_err(),
            "dirty region byte at {va:#x} is refused"
        );
    }
}
