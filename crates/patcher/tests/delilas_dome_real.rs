//! Disc oracle for the **Delilas Challenge** dome-course code injection: it
//! validates every hand-assembled address against the real US build - the
//! seed-hook `lw`, the template `lui`/`addiu`, the course-3 descriptor slot
//! (the stock actor template), and the SCUS routine cave being all-zero dead
//! space. Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::delilas_dome::{
    ARENA_BASE_VA, ARENA_OVERLAY_PROT_INDEX, DomeInjection, REWARD_HOOK_ORIG, REWARD_HOOK_VA,
    REWARD_ROUTINE_VA, ROSTER_VA, ROUTINE_VA, SEAT_HOOK_ORIG, SEAT_HOOK_VA, SEAT_ROUTINE_VA,
    SEED_HOOK_ORIG, SEED_HOOK_VA, TEMPLATE_BYTES, TEMPLATE_REF_ADDIU_ORIG, TEMPLATE_REF_LUI_ORIG,
    TEMPLATE_REF_LUI_VA, TEMPLATE_VA,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus_word(scus: &[u8], va: u32) -> u32 {
    let off = file_offset_for_va(scus, va).expect("resolve SCUS va");
    u32::from_le_bytes(scus[off..off + 4].try_into().unwrap())
}

fn overlay_word(entry: &[u8], va: u32) -> u32 {
    let off = (va - ARENA_BASE_VA) as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn baseline_sites_match_the_known_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let overlay = patcher
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");

    // Seed hook: the stock `lw v0,-0x4540(s1)` word reload.
    assert_eq!(
        overlay_word(&overlay, SEED_HOOK_VA),
        SEED_HOOK_ORIG,
        "seed hook is the recognized `lw v0,-0x4540(s1)`"
    );
    // Second-seat hook: the installer's stock `sb zero,1(v0)` seat-1 zero.
    assert_eq!(
        overlay_word(&overlay, SEAT_HOOK_VA),
        SEAT_HOOK_ORIG,
        "seat hook is the recognized `sb zero,1(v0)`"
    );
    // Reward hook: the settlement's stock `lw v0,0(v0)` payout-table load.
    assert_eq!(
        overlay_word(&overlay, REWARD_HOOK_VA),
        REWARD_HOOK_ORIG,
        "reward hook is the recognized `lw v0,0(v0)`"
    );
    // Template reference: `lui a0,0x801d` ; `addiu a0,a0,0x1a20`.
    assert_eq!(
        overlay_word(&overlay, TEMPLATE_REF_LUI_VA),
        TEMPLATE_REF_LUI_ORIG
    );
    assert_eq!(
        overlay_word(&overlay, TEMPLATE_REF_LUI_VA + 4),
        TEMPLATE_REF_ADDIU_ORIG
    );
    // The course-3 descriptor slot (0x801D1A20) currently holds the 24-byte
    // hub actor template.
    let desc_off = (0x801D_1A20 - ARENA_BASE_VA) as usize;
    assert_eq!(
        &overlay[desc_off..desc_off + 24],
        &TEMPLATE_BYTES,
        "descriptor slot holds the stock actor template"
    );
}

#[test]
fn plan_validates_against_the_real_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let overlay = patcher
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");

    let plan = DomeInjection::plan(&scus, &overlay).expect("plan against real build");

    // Five SCUS-cave writes (seed routine + template + roster + seat routine
    // + reward routine), all landing in all-zero dead space.
    assert_eq!(
        plan.scus.len(),
        5,
        "seed + template + roster + seat + reward"
    );
    for w in &plan.scus {
        assert!(!w.bytes.is_empty());
        assert!(
            scus[w.off..w.off + w.bytes.len()].iter().all(|&b| b == 0),
            "cave write at +{:#x} lands in zero dead space",
            w.off
        );
    }
    // Each cave slot resolves where we expect.
    for (i, va) in [
        ROUTINE_VA,
        TEMPLATE_VA,
        ROSTER_VA,
        SEAT_ROUTINE_VA,
        REWARD_ROUTINE_VA,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(plan.scus[i].off, file_offset_for_va(&scus, va).unwrap());
    }
    // The relocated template copies the stock bytes verbatim.
    assert_eq!(plan.scus[1].bytes, TEMPLATE_BYTES.to_vec());

    // Five overlay writes: seed detour, template repoint, descriptor, seat
    // detour, reward detour; every detour is a `j` (opcode 2).
    assert_eq!(plan.overlay.len(), 5);
    for idx in [0usize, 3, 4] {
        let detour = u32::from_le_bytes(plan.overlay[idx].bytes[..4].try_into().unwrap());
        assert_eq!(detour >> 26, 0x02, "overlay write {idx} is a `j` detour");
    }
    // The descriptor write covers the whole 24-byte template slot (8-byte
    // descriptor + 16 zero) so no stale course-4 slot survives.
    assert_eq!(plan.overlay[2].bytes.len(), 24);
    assert_eq!(
        &plan.overlay[2].bytes[..4],
        &2u32.to_le_bytes(),
        "course-3 round count = 2 (Che & Lu, then Gi)"
    );

    // Refuses a build it doesn't recognize (flip a hook-site byte).
    let mut bad = overlay.clone();
    let seed_off = (SEED_HOOK_VA - ARENA_BASE_VA) as usize;
    bad[seed_off] ^= 0xFF;
    assert!(
        DomeInjection::plan(&scus, &bad).is_err(),
        "must refuse an unrecognized seed-hook site"
    );

    // The baseline read is non-vacuous: the stock seed word is present.
    assert_eq!(scus_word(&scus, ROUTINE_VA), 0, "cave starts zero");
}
