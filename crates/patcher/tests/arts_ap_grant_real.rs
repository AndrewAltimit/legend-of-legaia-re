//! Disc-gated oracle for the **arts AP override** feature (see
//! `legaia_patcher::arts_ap_grant`): make a Tactical Art *grant* AP (Spirit,
//! `actor[+0x170]`, clamped at 100) or charge a flat cost, instead of retail's
//! computed `multiplier x command_count`. Three same-size detours into the party
//! arts queue-builder (PROT 0898) at `0x801EF410` / `0x801EF490` / `0x801EF988`,
//! the routines in the verified-dead SCUS arenas `shiny_seru::ARENA1_VA` /
//! `ARENA2_VA`, a per-(character, row) `i8` config table in `SCUS_GAP_VA`, and a
//! same-size rewrite of each targeted art's menu AP byte in the static
//! arts-name table.
//!
//! These apply it to a scratch copy of the real disc and assert, off the patched
//! image, that every hosted region was all-zero pre-patch; each detour became a
//! `j routine` plus a `nop`; the index-proof site B (`addiu a1,s3,-0xb`) and the
//! character-discriminator read (`lbu v0,0x0(t6)`) are intact; the routines,
//! config table and display bytes land exactly where the plan says; **an
//! override for one character leaves the other characters' cells and menu bytes
//! at retail**; every byte outside the planned edits is untouched; the disc still
//! parses and stays EDC/ECC-valid; a fixed input is byte-deterministic;
//! re-applying is refused (idempotent); the feature is refused on top of
//! shiny-Seru (shared regions); and an unrecognized build or dirty arena is
//! refused. Gates on `LEGAIA_DISC_BIN`; skips and passes when unset.
//!
//! HONESTY GATE: this proves only WHERE the bytes land, never in-game behaviour.
//! A live battle playtest (a configured art grants / costs what it says, admits
//! at the right AP level, clamps at 100, the refund isn't double-counted, and
//! the pause-menu arts list shows the new number) is still required before
//! shipping.

use legaia_art::queue::Character;
use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::arts_ap_grant::{
    self, ApMode, ArtApSpec, ArtsApGrantInjection, HOOK_A_VA, HOOK_B_VA, HOOK_C_VA, HOOK_D_VA,
    OVERLAY_BASE_VA, OVERLAY_PROT_INDEX, ROW_STRIDE, TABLE_LEN,
};
use legaia_patcher::arts_power::parse_combo;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shiny_seru::{ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, SCUS_GAP_VA};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// A representative override set. `RDLDL` is Vahn's Burning Flare at arts-table
/// row 1; **only Vahn** is targeted, which is exactly what the per-character
/// keying has to prove (row 1 is also Noa's Hurricane Kick and Gala's Explosive
/// Fist, and neither may move). The second entry is a flat-cost override on
/// Gala's Thunder Punch.
fn specs() -> Vec<ArtApSpec> {
    vec![
        ArtApSpec {
            character: Some(Character::Vahn),
            combo: parse_combo("RDLDL").unwrap(),
            mode: ApMode::Grant(10),
        },
        ArtApSpec {
            character: Some(Character::Gala),
            combo: parse_combo("RRL").unwrap(),
            mode: ApMode::Cost(7),
        },
    ]
}

#[test]
fn hosted_regions_are_all_zero_before_patch() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    for (va, len) in [
        (ARENA1_VA, ARENA1_END_VA - ARENA1_VA),
        (ARENA2_VA, ARENA2_END_VA - ARENA2_VA),
        (SCUS_GAP_VA, TABLE_LEN as u32),
    ] {
        let off = file_offset_for_va(&scus, va).unwrap();
        assert!(
            scus[off..off + len as usize].iter().all(|&b| b == 0),
            "{va:#x}..+{len} is all-zero dead space pre-patch"
        );
    }
    // Build fingerprints: the four pinned queue-builder words + the party-record
    // id read the config index keys on are the US build.
    let patcher = DiscPatcher::open(disc).expect("open");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();
    assert_eq!(
        overlay_word(&ov, HOOK_A_VA),
        0x94A2_0170,
        "A lhu v0,0x170(a1)"
    );
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
        "lbu v0,0x0(t6) - DAT_8007BD10[slot], the character discriminator"
    );
}

/// The retail menu AP byte is a hand-maintained mirror of the builder's
/// computed cost. Pin that here so the display edit's premise stays honest: the
/// value the list shows is `multiplier x command_count` with the multiplier
/// keyed by the art's position in its character's list (`0` -> 11, `1..3` ->
/// 10, `>= 4` -> 6), NOT by the arts-table display index (Noa's list skips
/// indices 2 and 3, and her rows still follow the visit order).
#[test]
fn retail_menu_ap_mirrors_the_builder_formula() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let entries = legaia_art::arts_table::parse_from_scus(&scus).expect("arts table");
    let mut visit = [0usize; 3];
    let mut checked = 0usize;
    for e in &entries {
        let v = &mut visit[e.character as usize];
        let mult: usize = match *v {
            0 => 11,
            1..=3 => 10,
            _ => 6,
        };
        *v += 1;
        let cmds = if e.is_miracle && e.commands.is_empty() {
            continue;
        } else {
            e.commands.len()
        };
        assert_eq!(
            usize::from(e.ap),
            mult * cmds,
            "{:?} row {} {:?}: menu AP is multiplier x command count",
            e.character,
            e.index,
            e.name
        );
        checked += 1;
    }
    assert!(checked >= 40, "checked {checked} arts (expected all 45)");
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

    // Plan first so we can verify exactly the planned bytes landed.
    let (config, resolved) = arts_ap_grant::resolve(&scus0, &specs()).expect("resolve");
    let plan = ArtsApGrantInjection::plan(&scus0, &ov0, config, resolved).expect("plan");
    // Per-character keying: RDLDL resolved to Vahn's row 1 and NOTHING else.
    assert_eq!(plan.resolved.len(), 2, "one resolution per targeted art");
    let vahn = plan
        .resolved
        .iter()
        .find(|r| r.character == Character::Vahn)
        .expect("Vahn resolution");
    assert_eq!(vahn.row, 1);
    assert_eq!(vahn.mode, ApMode::Grant(10));
    assert_eq!(vahn.display_ap, 0, "a grant shows 0 in the menu list");
    assert_ne!(vahn.previous_display_ap, 0, "retail showed a real cost");
    let gala = plan
        .resolved
        .iter()
        .find(|r| r.character == Character::Gala)
        .expect("Gala resolution");
    assert_eq!(gala.mode, ApMode::Cost(7));
    assert_eq!(gala.display_ap, 7, "a cost shows itself");

    let report = apply::inject_arts_ap_grant(&mut patcher, &specs()).expect("inject");
    assert_eq!(report.resolved, plan.resolved);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    // The three detours became `j routine` + nop, each targeting its region VA.
    for (site, target) in [
        (HOOK_A_VA, plan.guard_va),
        (HOOK_C_VA, plan.debit_va),
        (HOOK_D_VA, plan.refund_va),
    ] {
        let w = overlay_word(&ov, site);
        assert_eq!(w >> 26, 0x02, "site {site:#x} became a `j`");
        assert_eq!((w & 0x03ff_ffff) << 2, target & 0x0fff_ffff, "j -> routine");
        assert_eq!(overlay_word(&ov, site + 4), 0, "delay slot is nop");
    }
    // Site B (index proof) + the character read are untouched.
    assert_eq!(overlay_word(&ov, HOOK_B_VA), 0x2665_FFF5, "site B intact");
    assert_eq!(
        overlay_word(&ov, 0x801E_F340),
        0x91C2_0000,
        "t6 read intact"
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

    // Config table: exactly two live cells, in the right per-character blocks.
    let table_off = file_offset_for_va(&scus, plan.table_va).unwrap();
    let cell = |c: Character, row: u8| scus[table_off + (c as usize) * ROW_STRIDE + row as usize];
    assert_eq!(cell(Character::Vahn, 1) as i8, 10, "Vahn row 1 grants 10");
    assert_eq!(
        cell(Character::Noa, 1) as i8,
        0,
        "Noa's row 1 art is untouched - the keying is per character"
    );
    assert_eq!(cell(Character::Gala, 1) as i8, 0, "Gala's row 1 untouched");
    assert_eq!(cell(Character::Gala, gala.row) as i8, -7, "Gala costs 7");
    let live = (0..TABLE_LEN).filter(|&i| scus[table_off + i] != 0).count();
    assert_eq!(live, 2, "exactly the two configured cells are non-zero");

    // Menu display bytes: only the two targeted records changed, and the arts
    // that merely share a row still read their retail number.
    let after = legaia_art::arts_table::parse_from_scus(&scus).expect("patched arts table");
    let before = legaia_art::arts_table::parse_from_scus(&scus0).expect("stock arts table");
    for (b, a) in before.iter().zip(after.iter()) {
        let targeted = (b.character == Character::Vahn && b.index == 1)
            || (b.character == Character::Gala && b.index == gala.row);
        if targeted {
            continue;
        }
        assert_eq!(
            b.ap, a.ap,
            "{:?} row {} menu AP unchanged (not targeted)",
            b.character, b.index
        );
    }
    let vahn_row1 = after
        .iter()
        .find(|e| e.character == Character::Vahn && e.index == 1)
        .unwrap();
    assert_eq!(vahn_row1.ap, 0, "granting art now reads 0 AP in the list");
    let gala_hit = after
        .iter()
        .find(|e| e.character == Character::Gala && e.index == gala.row)
        .unwrap();
    assert_eq!(gala_hit.ap, 7, "cost art now reads its configured 7 AP");

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
    apply::inject_arts_ap_grant(&mut a, &specs()).unwrap();
    apply::inject_arts_ap_grant(&mut b, &specs()).unwrap();
    assert_eq!(a.image(), b.image(), "a fixed input is byte-identical");

    // Idempotent: re-applying the SAME override on the already-patched image
    // fails (the arena is no longer dead) rather than stacking a second
    // injection - the patched bytes stay exactly as the first pass left them.
    let before = a.image().to_vec();
    assert!(
        apply::inject_arts_ap_grant(&mut a, &specs()).is_err(),
        "re-injecting into the now-live arena is refused"
    );
    assert_eq!(
        a.image(),
        &before[..],
        "a refused re-apply leaves the image unchanged"
    );
}

#[test]
fn mutually_exclusive_with_shiny_seru() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Both reuse the same verified-dead regions. The CLI refuses the combination
    // up front; the apply layer also enforces it structurally: whichever runs
    // second finds a region no longer all-zero and refuses.
    let mut p = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_shiny_seru(&mut p, legaia_patcher::shiny_seru::DEFAULT_PCT).expect("shiny");
    assert!(
        apply::inject_arts_ap_grant(&mut p, &specs()).is_err(),
        "arts-ap override refused after shiny-Seru (shared regions)"
    );

    let mut q = DiscPatcher::open(disc).expect("open");
    apply::inject_arts_ap_grant(&mut q, &specs()).expect("ap override");
    assert!(
        apply::inject_shiny_seru(&mut q, legaia_patcher::shiny_seru::DEFAULT_PCT).is_err(),
        "shiny-Seru refused after the arts-ap override (shared regions)"
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

    let spec = |c: Option<Character>, combo: &str, mode: ApMode| ArtApSpec {
        character: c,
        combo: parse_combo(combo).unwrap(),
        mode,
    };
    // Unknown combo / out-of-range amount are refused at resolve time.
    assert!(arts_ap_grant::resolve(&scus, &[spec(None, "LLLLLLLL", ApMode::Grant(5))]).is_err());
    assert!(arts_ap_grant::resolve(&scus, &[spec(None, "RDLDL", ApMode::Grant(0))]).is_err());
    assert!(arts_ap_grant::resolve(&scus, &[spec(None, "RDLDL", ApMode::Cost(0))]).is_err());
    assert!(arts_ap_grant::resolve(&scus, &[spec(None, "RDLDL", ApMode::Cost(200))]).is_err());
    // A combo the named character does not have is refused (Vahn has no RRLLL).
    assert!(
        arts_ap_grant::resolve(
            &scus,
            &[spec(Some(Character::Vahn), "RRLLL", ApMode::Cost(5))]
        )
        .is_err()
    );
    // The same art configured both ways is a conflict.
    assert!(
        arts_ap_grant::resolve(
            &scus,
            &[
                spec(Some(Character::Vahn), "RDLDL", ApMode::Cost(5)),
                spec(Some(Character::Vahn), "RDLDL", ApMode::Grant(5)),
            ]
        )
        .is_err()
    );
    // An unqualified combo two characters share resolves to BOTH, in their own
    // cells (this is the de-shared behaviour, not a conflict).
    let (cfg, res) =
        arts_ap_grant::resolve(&scus, &[spec(None, "RRL", ApMode::Cost(9))]).expect("shared combo");
    assert!(res.len() >= 2, "RRL is Vahn's and Gala's");
    let live: Vec<usize> = (0..TABLE_LEN).filter(|&i| cfg[i] != 0).collect();
    assert_eq!(live.len(), res.len(), "one cell per resolved art");

    // A valid plan on the real build.
    let (config, resolved) = arts_ap_grant::resolve(&scus, &specs()).unwrap();
    assert!(ArtsApGrantInjection::plan(&scus, &ov, config, resolved.clone()).is_ok());

    // Corrupt a 0898 hook -> refuse.
    let mut ov_bad = ov.clone();
    let doff = (HOOK_C_VA - OVERLAY_BASE_VA) as usize;
    ov_bad[doff] ^= 0xFF;
    assert!(ArtsApGrantInjection::plan(&scus, &ov_bad, config, resolved.clone()).is_err());

    // Corrupt the character-discriminator read -> refuse (the index would key
    // on a register that no longer holds &DAT_8007BD10[slot]).
    let mut ov_char = ov.clone();
    let coff = (0x801E_F340 - OVERLAY_BASE_VA) as usize;
    ov_char[coff] ^= 0xFF;
    assert!(ArtsApGrantInjection::plan(&scus, &ov_char, config, resolved.clone()).is_err());

    // Dirty each landing zone -> refuse (all-zero guard).
    for va in [ARENA1_VA, ARENA2_VA, SCUS_GAP_VA] {
        let mut scus_dirty = scus.clone();
        let goff = file_offset_for_va(&scus_dirty, va).unwrap();
        scus_dirty[goff + 8] = 0x42;
        assert!(
            ArtsApGrantInjection::plan(&scus_dirty, &ov, config, resolved.clone()).is_err(),
            "dirty {va:#x} refused"
        );
    }
}
