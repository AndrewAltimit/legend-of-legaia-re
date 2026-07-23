//! Disc-gated oracle for the **arts AP-grant** feature (see
//! `legaia_patcher::arts_ap_grant`): make a Tactical Art *grant* AP (Spirit,
//! `actor[+0x170]`, clamped at 100) instead of costing it. Three same-size
//! detours into the party arts queue-builder (PROT 0898) at `0x801EF410` /
//! `0x801EF490` / `0x801EF988`, plus the routines + a 26-entry `i8` config table
//! injected into the verified-dead SCUS arena `shiny_seru::ARENA1_VA`.
//!
//! These apply it to a scratch copy of the real disc and assert, off the patched
//! image, that the arena was all-zero pre-patch; each detour became a `j routine`
//! plus a `nop`; the index-proof site B (`addiu a1,s3,-0xb`) is intact; the
//! routines and config table land exactly where the plan says; every byte outside
//! the planned edits is untouched; the disc still parses and stays EDC/ECC-valid;
//! a fixed input is byte-deterministic; re-applying is refused (idempotent);
//! AP-grant is refused on top of shiny-Seru (same arena bytes); and an
//! unrecognized build or dirty arena is refused. Gates on `LEGAIA_DISC_BIN`;
//! skips and passes when unset.
//!
//! HONESTY GATE: this proves only WHERE the bytes land, never in-game behaviour.
//! A live battle playtest (a configured art grants AP, admits at 0 AP, clamps at
//! 100, and the refund isn't double-counted) is still required before shipping.

use legaia_art::queue::Command;
use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::arts_ap_grant::{
    self, ArtsApGrantInjection, HOOK_A_VA, HOOK_B_VA, HOOK_C_VA, HOOK_D_VA, OVERLAY_BASE_VA,
    OVERLAY_PROT_INDEX,
};
use legaia_patcher::arts_power::parse_combo;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::shiny_seru::{ARENA1_END_VA, ARENA1_VA};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - OVERLAY_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// A representative grant set: Vahn's Burning Flare (RDLDL) = arts index 1, a
/// shared row that also covers Noa's Hurricane Kick + Gala's Double 4-Punch.
fn grants() -> Vec<(Vec<Command>, u8)> {
    vec![(parse_combo("RDLDL").unwrap(), 10)]
}

#[test]
fn arena_is_all_zero_before_patch() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let off = file_offset_for_va(&scus, ARENA1_VA).unwrap();
    let len = (ARENA1_END_VA - ARENA1_VA) as usize;
    assert!(
        scus[off..off + len].iter().all(|&b| b == 0),
        "arena {ARENA1_VA:#x}..{ARENA1_END_VA:#x} is all-zero dead space pre-patch"
    );
    // Build fingerprints: the four pinned queue-builder words are the US build.
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
    let (config, resolved) = arts_ap_grant::resolve(&scus0, &grants()).expect("resolve");
    let plan = ArtsApGrantInjection::plan(&scus0, &ov0, config, resolved).expect("plan");
    // RDLDL resolves to row 1 (arts-table index), a shared row across chars.
    assert!(
        plan.resolved.iter().any(|g| g.row == 1 && g.amount == 10),
        "RDLDL -> row 1 @ 10 AP"
    );
    assert!(
        plan.resolved.iter().any(
            |g| g.shared.len() >= 2 && g.shared.iter().any(|(_, n, _)| n.contains("Hurricane"))
        ),
        "shared row 1 also covers Noa's Hurricane Kick"
    );

    let report = apply::inject_arts_ap_grant(&mut patcher, &grants()).expect("inject");
    assert_eq!(report.resolved, plan.resolved);

    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("patched SCUS");
    let ov = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    // The three detours became `j routine` + nop, each targeting its arena VA.
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
    // Site B (index proof) is untouched.
    assert_eq!(overlay_word(&ov, HOOK_B_VA), 0x2665_FFF5, "site B intact");

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

    // Config table: row 1 = 10, every other row = 0.
    let table_off = file_offset_for_va(&scus, plan.table_va).unwrap();
    for row in 0..arts_ap_grant::NUM_ROWS {
        let want = if row == 1 { 10 } else { 0 };
        assert_eq!(scus[table_off + row], want, "config row {row}");
    }

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
    apply::inject_arts_ap_grant(&mut a, &grants()).unwrap();
    apply::inject_arts_ap_grant(&mut b, &grants()).unwrap();
    assert_eq!(a.image(), b.image(), "a fixed input is byte-identical");

    // Idempotent: re-applying the SAME grant on the already-patched image fails
    // (the arena is no longer dead) rather than stacking a second injection - the
    // patched bytes stay exactly as the first pass left them.
    let before = a.image().to_vec();
    assert!(
        apply::inject_arts_ap_grant(&mut a, &grants()).is_err(),
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
    // Both reuse the same arena bytes. The CLI refuses the combination up front;
    // the apply layer also enforces it structurally: whichever runs second finds
    // the arena no longer all-zero and refuses.
    let mut p = DiscPatcher::open(disc.clone()).expect("open");
    apply::inject_shiny_seru(&mut p, legaia_patcher::shiny_seru::DEFAULT_PCT).expect("shiny");
    assert!(
        apply::inject_arts_ap_grant(&mut p, &grants()).is_err(),
        "arts-ap-grant refused after shiny-Seru (shared arena)"
    );

    let mut q = DiscPatcher::open(disc).expect("open");
    apply::inject_arts_ap_grant(&mut q, &grants()).expect("ap-grant");
    assert!(
        apply::inject_shiny_seru(&mut q, legaia_patcher::shiny_seru::DEFAULT_PCT).is_err(),
        "shiny-Seru refused after arts-ap-grant (shared arena)"
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

    // Unknown combo / out-of-range amount are refused at resolve time.
    assert!(arts_ap_grant::resolve(&scus, &[(parse_combo("LLLLLLLL").unwrap(), 5)]).is_err());
    assert!(arts_ap_grant::resolve(&scus, &[(parse_combo("RDLDL").unwrap(), 0)]).is_err());
    assert!(arts_ap_grant::resolve(&scus, &[(parse_combo("RDLDL").unwrap(), 200)]).is_err());

    // A valid plan on the real build.
    let (config, resolved) = arts_ap_grant::resolve(&scus, &grants()).unwrap();
    assert!(ArtsApGrantInjection::plan(&scus, &ov, config, resolved.clone()).is_ok());

    // Corrupt a 0898 hook -> refuse.
    let mut ov_bad = ov.clone();
    let doff = (HOOK_C_VA - OVERLAY_BASE_VA) as usize;
    ov_bad[doff] ^= 0xFF;
    assert!(ArtsApGrantInjection::plan(&scus, &ov_bad, config, resolved.clone()).is_err());

    // Dirty the arena landing zone -> refuse (all-zero guard).
    let mut scus_dirty = scus.clone();
    let goff = file_offset_for_va(&scus_dirty, ARENA1_VA).unwrap();
    scus_dirty[goff + 8] = 0x42;
    assert!(ArtsApGrantInjection::plan(&scus_dirty, &ov, config, resolved).is_err());
}
