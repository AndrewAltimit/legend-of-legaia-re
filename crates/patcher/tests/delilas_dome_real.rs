//! Disc oracle for the **Delilas Challenge** dome-course code injection: it
//! validates every hand-assembled address against the real US build - the
//! seed-hook `lw`, the template `lui`/`addiu`, the course-3 descriptor slot
//! (the stock actor template), and the SCUS routine cave being all-zero dead
//! space. Requires `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::delilas_dome::{
    ARENA_BASE_VA, ARENA_OVERLAY_PROT_INDEX, DomeInjection, ROSTER_VA, ROUTINE_VA, SEED_HOOK_ORIG,
    SEED_HOOK_VA, TEMPLATE_BYTES, TEMPLATE_REF_ADDIU_ORIG, TEMPLATE_REF_LUI_ORIG,
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

    // Three SCUS-cave writes (routine + template + roster), all landing in
    // all-zero dead space and same-size (each write == its byte length).
    assert_eq!(plan.scus.len(), 3, "routine + template + roster");
    for w in &plan.scus {
        assert!(!w.bytes.is_empty());
        assert!(
            scus[w.off..w.off + w.bytes.len()].iter().all(|&b| b == 0),
            "cave write at +{:#x} lands in zero dead space",
            w.off
        );
    }
    // The seed routine's first cave slot resolves where we expect.
    assert_eq!(
        plan.scus[0].off,
        file_offset_for_va(&scus, ROUTINE_VA).unwrap()
    );
    assert_eq!(
        plan.scus[1].off,
        file_offset_for_va(&scus, TEMPLATE_VA).unwrap()
    );
    assert_eq!(
        plan.scus[2].off,
        file_offset_for_va(&scus, ROSTER_VA).unwrap()
    );
    // The relocated template copies the stock bytes verbatim.
    assert_eq!(plan.scus[1].bytes, TEMPLATE_BYTES.to_vec());

    // Three overlay writes: seed detour, template repoint, descriptor.
    assert_eq!(plan.overlay.len(), 3);
    // The seed detour is `j ROUTINE_VA` (opcode 2 in the high 6 bits).
    let detour = u32::from_le_bytes(plan.overlay[0].bytes[..4].try_into().unwrap());
    assert_eq!(detour >> 26, 0x02, "seed detour is a `j`");
    // The descriptor write covers the whole 24-byte template slot (8-byte
    // descriptor + 16 zero) so no stale course-4 slot survives.
    assert_eq!(plan.overlay[2].bytes.len(), 24);
    assert_eq!(
        &plan.overlay[2].bytes[..4],
        &3u32.to_le_bytes(),
        "course-3 round count = 3"
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
