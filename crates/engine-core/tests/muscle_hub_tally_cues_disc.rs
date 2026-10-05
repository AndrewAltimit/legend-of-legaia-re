//! Disc-gated: the Muscle Dome hub's INTERVAL "ka-ching" cues resolve.
//!
//! The arena init points the current-bundle slot `_DAT_8007B8D0` at its own
//! descriptor bundle (extraction 542) and streams its side bank (extraction
//! 1157) into VAB slot 3; the INTERVAL arm then writes ring ids
//! `[0x202, 0x202, 0x202, 0x203]` with countdowns `[0, 30, 60, 90]`. This
//! pins that both ids resolve, through the world's runtime bundle in dome
//! mode, to category-3 rows whose program and tones the side bank carries,
//! and that the world's tail stager names that bank. No Sony bytes are
//! asserted. Skips + passes when `LEGAIA_DISC_BIN` is absent.

use legaia_asset::minigame_sfx::{ARENA_SFX_BUNDLE_PROT_INDEX, ARENA_SIDE_BANK_PROT_INDEX};
use legaia_engine_core::muscle_dome::{HUB_TALLY_CUE_STAGGER, HUB_TALLY_CUES};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SceneMode;

#[test]
fn the_interval_tally_cues_key_the_arena_side_bank() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let bundle = host
        .index
        .entry_bytes_extended(ARENA_SFX_BUNDLE_PROT_INDEX as u32)
        .expect("arena bundle");
    let bank = host
        .index
        .entry_bytes_extended(ARENA_SIDE_BANK_PROT_INDEX)
        .expect("arena side bank");
    let vab = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bank, o).ok())
        .expect("the side bank is a VAB");

    let w = &mut host.world;
    w.minigames.muscle_sfx_bundle = bundle.to_vec();
    w.mode = SceneMode::MuscleDome;
    let side = w
        .tail_side_band_bank()
        .expect("the dome stages a side bank");
    assert_eq!(
        (side.slot, side.prot_entry),
        (3, ARENA_SIDE_BANK_PROT_INDEX)
    );

    for id in [0x202i16, 0x203] {
        let row = w.runtime_sfx_descriptor(id).expect("row resolves");
        let (program, tone, voices, category) = (row[0], row[1], row[3] & 0x1F, row[4]);
        assert_eq!(category, 3, "cue {id:#x} keys VAB slot 3");
        let p = vab
            .programs
            .get(usize::from(program))
            .expect("program in the side bank");
        let tones = usize::from(p.tones);
        assert!(
            usize::from(tone) + usize::from(voices) <= tones,
            "cue {id:#x}: tones {tone}+{voices} within {tones}"
        );
    }
    assert_eq!(HUB_TALLY_CUES, [0x202, 0x202, 0x202, 0x203]);
    assert_eq!(HUB_TALLY_CUE_STAGGER, [0, 0x1E, 0x3C, 0x5A]);
    eprintln!("[ran] arena tally cues resolve into slot 3 (PROT {ARENA_SIDE_BANK_PROT_INDEX})");
}
