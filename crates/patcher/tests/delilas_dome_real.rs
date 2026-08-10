//! Disc oracle for the **Delilas Challenge** dome-course code injection: it
//! validates every hand-assembled address against the real US build - the
//! seed-hook `lw`, the seat-hook `sb`, the stream-hook `addiu` (SCUS), the
//! template `lui`/`addiu`, the course-3 descriptor slot (the stock actor
//! template), the SCUS routine cave being all-zero dead space, and the two
//! slim-clone archive slots building from the real Che/Lu blocks. Requires
//! `LEGAIA_DISC_BIN`; skips (and passes) without it.

use legaia_asset::item_names::file_offset_for_va;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::delilas_dome::{
    AI_BLOCK_CAPACITY, AI_BLOCK_VA, ARENA_BASE_VA, ARENA_OVERLAY_PROT_INDEX,
    BATTLE_OVERLAY_PROT_INDEX, CLONE_IDS, DELILAS_PAIR_IDS, DISPLAY_ROUTINE_CAPACITY,
    DISPLAY_ROUTINE_VA, DomeInjection, MAGIC_REJECT_NEW, MAGIC_REJECT_ORIG, MAGIC_REJECT_SITES,
    PRGERR_PRINT_GATE_NEW, PRGERR_PRINT_GATE_VA, REWARD_HOOK_ORIG, REWARD_HOOK_VA,
    REWARD_ROUTINE_VA, ROSTER_VA, ROUTINE_VA, SEAT_HOOK_ORIG, SEAT_HOOK_VA, SEAT_ROUTINE_VA,
    SEED_HOOK_ORIG, SEED_HOOK_VA, STREAM_HOOK_ORIG, STREAM_HOOK_VA, STREAM_ROUTINE_VA,
    STREAM2_HOOK_ORIG, STREAM2_HOOK_VA, STREAM2_ROUTINE_VA, TEMPLATE_BYTES,
    TEMPLATE_REF_ADDIU_ORIG, TEMPLATE_REF_LUI_ORIG, TEMPLATE_REF_LUI_VA, TEMPLATE_VA,
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
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS in image");
    let overlay = patcher
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .expect("read arena overlay");
    let battle = patcher
        .read_entry(BATTLE_OVERLAY_PROT_INDEX)
        .expect("read battle overlay");

    // Seed hook: the stock `lw v0,-0x4540(s1)` word reload.
    assert_eq!(
        overlay_word(&overlay, SEED_HOOK_VA),
        SEED_HOOK_ORIG,
        "seed hook is the recognized `lw v0,-0x4540(s1)`"
    );
    // Seat hook: the installer's stock `sb zero,1(v0)` seat-1 zero store.
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
    // Stream hooks (SCUS): the monster streamer's stock `addiu v1,a0,-0x1`
    // conversion and the pre-streamer's `ori a0,a0,0x2800` (the site-B hook
    // sits one instruction before its conversion - delay-slot discipline).
    assert_eq!(
        scus_word(&scus, STREAM_HOOK_VA),
        STREAM_HOOK_ORIG,
        "stream hook A is the recognized `addiu v1,a0,-0x1`"
    );
    assert_eq!(
        scus_word(&scus, STREAM2_HOOK_VA),
        STREAM2_HOOK_ORIG,
        "stream hook B is the recognized `ori a0,a0,0x2800`"
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
    // The two Magic-command reject masks are the stock `andi v0,v0,0x200`.
    for va in MAGIC_REJECT_SITES {
        let off = (va - legaia_patcher::delilas_dome::BATTLE_BASE_VA) as usize;
        let got = u32::from_le_bytes(battle[off..off + 4].try_into().unwrap());
        assert_eq!(got, MAGIC_REJECT_ORIG, "stock magic-reject mask at {va:#x}");
    }
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
    let battle = patcher
        .read_entry(BATTLE_OVERLAY_PROT_INDEX)
        .expect("read battle overlay");

    let plan = DomeInjection::plan(&scus, &overlay, &battle).expect("plan against real build");

    // Seven SCUS-cave writes (seed routine + template + roster + seat routine
    // + reward routine + the two stream routines), all in all-zero dead
    // space, plus the two stream-map hooks, the PRG ERR print-gate flip,
    // the AI block (over unreferenced FUN_80035274) and the winnings-display
    // override (over unreferenced FUN_800260DC).
    assert_eq!(
        plan.scus.len(),
        12,
        "seven cave writes + two stream hooks + PRG ERR gate + AI block + display override"
    );
    for w in &plan.scus[..7] {
        assert!(!w.bytes.is_empty());
        assert!(
            scus[w.off..w.off + w.bytes.len()].iter().all(|&b| b == 0),
            "cave write at +{:#x} lands in zero dead space",
            w.off
        );
    }
    for (i, va) in [
        ROUTINE_VA,
        TEMPLATE_VA,
        ROSTER_VA,
        SEAT_ROUTINE_VA,
        REWARD_ROUTINE_VA,
        STREAM_ROUTINE_VA,
        STREAM2_ROUTINE_VA,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(plan.scus[i].off, file_offset_for_va(&scus, va).unwrap());
    }
    // The relocated template copies the stock bytes verbatim.
    assert_eq!(plan.scus[1].bytes, TEMPLATE_BYTES.to_vec());
    // Both stream hooks are `j` detours over the stock conversion sites.
    for (i, hook_va) in [(7usize, STREAM_HOOK_VA), (8, STREAM2_HOOK_VA)] {
        assert_eq!(
            plan.scus[i].off,
            file_offset_for_va(&scus, hook_va).unwrap()
        );
        let detour = u32::from_le_bytes(plan.scus[i].bytes[..4].try_into().unwrap());
        assert_eq!(detour >> 26, 0x02, "stream hook {i} is a `j` detour");
    }
    // PRG ERR print-gate flip, then the AI block and the display override
    // over their fingerprinted unreferenced bodies.
    assert_eq!(
        plan.scus[9].off,
        file_offset_for_va(&scus, PRGERR_PRINT_GATE_VA).unwrap()
    );
    assert_eq!(plan.scus[9].bytes, PRGERR_PRINT_GATE_NEW.to_le_bytes());
    assert_eq!(
        plan.scus[10].off,
        file_offset_for_va(&scus, AI_BLOCK_VA).unwrap()
    );
    assert!(plan.scus[10].bytes.len() <= AI_BLOCK_CAPACITY);
    assert_eq!(
        plan.scus[11].off,
        file_offset_for_va(&scus, DISPLAY_ROUTINE_VA).unwrap()
    );
    assert!(plan.scus[11].bytes.len() <= DISPLAY_ROUTINE_CAPACITY);

    // Six overlay writes: seed detour, template repoint, descriptor, seat
    // detour, reward detour, winnings-display detour; every detour is a `j`
    // (opcode 2).
    assert_eq!(plan.overlay.len(), 6);
    for idx in [0usize, 3, 4, 5] {
        let detour = u32::from_le_bytes(plan.overlay[idx].bytes[..4].try_into().unwrap());
        assert_eq!(detour >> 26, 0x02, "overlay write {idx} is a `j` detour");
    }
    // The descriptor write covers the whole 24-byte template slot (8-byte
    // descriptor + 16 zero) so no stale course-4 slot survives.
    assert_eq!(plan.overlay[2].bytes.len(), 24);
    assert_eq!(
        &plan.overlay[2].bytes[..4],
        &2u32.to_le_bytes(),
        "course-3 round count = 2 (Che & Lu double-team, then Gi)"
    );

    // Three battle-overlay writes: the widened magic-reject masks + the
    // AI-retime hook `j` into the SCUS block.
    assert_eq!(plan.battle.len(), 3);
    for w in &plan.battle[..2] {
        assert_eq!(w.bytes, MAGIC_REJECT_NEW.to_le_bytes().to_vec());
    }
    let ai_hook = u32::from_le_bytes(plan.battle[2].bytes[..4].try_into().unwrap());
    assert_eq!(ai_hook >> 26, 0x02, "AI hook is a `j` detour");

    // Refuses a build it doesn't recognize (flip a hook-site byte).
    let mut bad = overlay.clone();
    let seed_off = (SEED_HOOK_VA - ARENA_BASE_VA) as usize;
    bad[seed_off] ^= 0xFF;
    assert!(
        DomeInjection::plan(&scus, &bad, &battle).is_err(),
        "must refuse an unrecognized seed-hook site"
    );

    // The baseline read is non-vacuous: the cave starts zero.
    assert_eq!(scus_word(&scus, ROUTINE_VA), 0, "cave starts zero");
}

#[test]
fn clone_slots_are_battle_unreachable_and_slims_build() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");

    // The slim clones must build from the real Che/Lu slots and re-encode
    // within the slot stride (the apply layer does exactly this).
    for (&src, &dst) in DELILAS_PAIR_IDS.iter().zip(CLONE_IDS.iter()) {
        let slot = patcher.monster_slot(src).expect("read source slot");
        let size = u32::from_le_bytes(slot[..4].try_into().unwrap()) as usize;
        let block = legaia_lzs::decompress(&slot[4..], size).expect("decode block");
        let (protected, extra_drop) = legaia_patcher::delilas_dome::slim_policy(src);
        let slim = legaia_asset::monster_archive::slim_castables(&block, protected, extra_drop)
            .expect("slim");
        let encoded = legaia_asset::monster_archive::encode_slot(&slim.bytes).expect("encode");
        assert_eq!(encoded.len(), 0x14000, "clone slot {dst} is slot-sized");
        // The slim heap footprint must be under the original's.
        let orig_heap = u32::from_le_bytes(block[8..12].try_into().unwrap());
        let slim_heap = u32::from_le_bytes(slim.bytes[8..12].try_into().unwrap());
        assert!(slim_heap < orig_heap, "clone {dst} is lighter than {src}");
    }

    // The clone ids must stay outside every encounter formation on the disc -
    // the full sweep both randomizer features use. (The retail dome rosters
    // and the `--unused-enemies` pool are checked statically in the module.)
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let mut scenes = Vec::new();
        if let Some(s) = legaia_patcher::encounter::SceneEncounters::locate(&entry, idx) {
            scenes.push(s);
        }
        scenes
            .extend(legaia_patcher::encounter::SceneEncounters::locate_streaming_mans(&entry, idx));
        for s in scenes {
            for f in 0..s.formation_count() {
                for id in s.formation_ids(f) {
                    assert!(
                        !CLONE_IDS.contains(&(id as u16)),
                        "clone id {id} referenced by entry {idx} formation {f}"
                    );
                }
            }
        }
    }
}
