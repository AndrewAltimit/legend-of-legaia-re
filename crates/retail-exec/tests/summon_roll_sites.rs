//! Every `jal FUN_801DD0AC` in the slot-B cast / summon images (`0903..=0966`)
//! off the disc, against the port's owner of each one. The kernel runs the
//! finisher `FUN_801DDB30` on every roll (`0x801DD478`): spell XP, the
//! victim's spirit gauge, the struck mark. A port site that rolled the bare
//! predamage dropped all three (Ozma's ladder, Mule's wave), so each site
//! here names the port routine that rolls it, and every one of those goes
//! through `World::summon_module_roll` or the band's fold
//! (`World::cast_spell_on_slots_prepaid`, whose per-hit fold runs the same
//! three). A new site on the disc, or one that moves, fails the test until
//! it is read and given an owner.

use crate::common::disc;

const KERNEL: u32 = 0x801D_D0AC;
const SLOT_B_BASE: u32 = 0x801F_69D8;

/// `(PROT entry, call VA, port owner)`. `fold` is the band's generic fold;
/// a named routine rolls through `World::summon_module_roll`.
const SITES: &[(u32, u32, &str)] = &[
    (903, 0x801F_74AC, "fold"),
    (904, 0x801F_7D38, "theeder_tick (module_code)"),
    (906, 0x801F_7370, "fold"),
    (908, 0x801F_76CC, "apply_summon_pre_hits (arm 8)"),
    (908, 0x801F_7C14, "zenoir_tick (module_code)"),
    (908, 0x801F_7DF8, "zenoir_tick (module_code)"),
    (909, 0x801F_75F4, "fold"),
    (910, 0x801F_887C, "swordie (seru_tick)"),
    (912, 0x801F_8024, "fold"),
    (913, 0x801F_8444, "fold"),
    (914, 0x801F_7718, "fold"),
    (915, 0x801F_7108, "apply_summon_pre_hits (arm 7)"),
    (915, 0x801F_789C, "mushura chase (mushura_writes)"),
    (916, 0x801F_85A8, "fold"),
    (917, 0x801F_7DE8, "fold"),
    (918, 0x801F_87A4, "fold"),
    (920, 0x801F_7844, "fold"),
    (921, 0x801F_7BE8, "fold"),
    (922, 0x801F_8E1C, "fold"),
    (923, 0x801F_8908, "fold"),
    (927, 0x801F_7E0C, "juggernaut arm 26 (juggernaut_writes)"),
    (
        927,
        0x801F_8758,
        "juggernaut stager (run_cast_module_aoe_for)",
    ),
    (928, 0x801F_8114, "apply_summon_pre_hits (arm 0xF)"),
    (928, 0x801F_89F0, "fold"),
    (929, 0x801F_7E4C, "mule_wave_hit"),
    (929, 0x801F_8758, "fold"),
    (931, 0x801F_8438, "fold"),
    (932, 0x801F_7BA0, "apply_summon_pre_hits (arm 0x12)"),
    (932, 0x801F_7EF4, "fold"),
    (933, 0x801F_7CCC, "apply_summon_pre_hits (arm 0xC)"),
    (933, 0x801F_8350, "fold"),
    (934, 0x801F_8E04, "ozma_hit_ladder, then the fold"),
];

/// Calls inside an image's inherited tail: a byte-identical copy of the
/// donor's code at the same file offset, which this image never runs
/// (`scripts/ghidra-analysis/inherited_tail.py`).
const INHERITED: &[(u32, u32, u32)] = &[(911, 0x801F_887C, 910), (951, 0x801F_8E04, 934)];

#[test]
fn every_summon_roll_site_has_an_owner_that_runs_the_finisher() {
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let jal = (3 << 26) | ((KERNEL >> 2) & 0x03FF_FFFF);
    let mut found = Vec::new();
    for entry in 903..=966u32 {
        let image = disc.read_entry(entry as usize).expect("slot-B image");
        for (i, w) in image.as_chunks::<4>().0.iter().enumerate() {
            if u32::from_le_bytes(*w) == jal {
                found.push((entry, SLOT_B_BASE + 4 * i as u32));
            }
        }
    }
    let mut expected: Vec<(u32, u32)> = SITES.iter().map(|&(e, va, _)| (e, va)).collect();
    for &(entry, va, donor) in INHERITED {
        assert!(
            SITES.iter().any(|&(e, v, _)| e == donor && v == va),
            "{entry}'s tail call names its donor {donor}'s own site"
        );
        expected.push((entry, va));
    }
    expected.sort_unstable();
    found.sort_unstable();
    assert_eq!(found, expected, "the disc's FUN_801DD0AC call sites");
}
