//! Battle / minigame punch-SFX authenticity oracle.
//!
//! Proves the two headline hit cues actually **sound** through the retail
//! resolution path:
//!
//! * cue `0x1A` - the Tactical-Arts generic strike cue (art-record Hit Effect
//!   Cue kind), and
//! * cue `0x09` - the Baka Fighter exchange-hit cue the duel overlay writes
//!   into the SFX ring (`FUN_801D3B18`).
//!
//! Both are decoded from the disc's static SFX descriptor table
//! (`legaia_asset::sfx_table`) and fired against the **class-2 sound bank**
//! (extraction PROT 0869) the retail battle scene loader / Baka init load - the
//! bank whose low programs (`0`, `3`) carry these cues. The cue names its tone
//! by explicit region **index** (`VabBank::play_tone`), so a note that falls
//! outside the tone's key window (which the old key-range `play_note` path
//! silently dropped to silence) still keys a voice.
//!
//! Skip-passes (CLAUDE.md disc-gated convention) when `LEGAIA_DISC_BIN` is unset
//! or the extracted `SCUS_942.54` / `PROT.DAT` aren't on disk.

use std::path::PathBuf;

use legaia_asset::sfx_table::SfxTable;
use legaia_engine_audio::{Spu, VabBank, spu::ram::SpuAllocator};
use legaia_engine_core::scene::SceneHost;

/// Cue ids (descriptor indices) under test.
const CUE_ART_STRIKE: u8 = 0x1A;
const CUE_BAKA_HIT: u8 = 0x09;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("SCUS_942.54").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn battle_and_baka_hit_cues_sound_against_the_class2_bank() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/SCUS_942.54 + PROT.DAT not present");
        return;
    };

    // SCUS -> the static SFX descriptor table -> the engine SFX bank, carrying
    // program + tone + note + voice-count (the full descriptor).
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS_942.54");
    let table = SfxTable::from_scus(&scus).expect("parse SFX descriptor table");
    let bank = legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    );

    // Sanity: the two cues carry their disc descriptors (program 3 tone 0 for
    // the strike, program 0 tone 9 for the Baka hit). The strike's note (67)
    // is what the old key-range path resolved against - the tone-index path
    // does not depend on the tone's key window.
    let strike = table.get(CUE_ART_STRIKE).expect("0x1A descriptor");
    assert_eq!((strike.program, strike.tone), (3, 0), "0x1A = p3 t0");
    let baka = table.get(CUE_BAKA_HIT).expect("0x09 descriptor");
    assert_eq!((baka.program, baka.tone), (0, 9), "0x09 = p0 t9");

    // PROT 0869 -> the class-2 VAB (scene-VAB-style stream: chunk header at +0,
    // VAB at +4). Upload into a fresh SPU, then fire each cue.
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let bytes = host
        .index
        .entry_bytes_extended(869)
        .expect("read PROT 0869");
    let (report, vab_off) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        .expect("class-2 bank has a VAB header at +4 or +0");
    let body = &bytes[vab_off..];

    // The cues use programs 0 and 3 - both must be resident in this bank.
    assert!(
        report.tones.len() > 3,
        "class-2 bank exposes program 3 (found {} programs)",
        report.tones.len()
    );

    let mut spu = Spu::new();
    let mut alloc = SpuAllocator::new(0x1000, 0x40_000);
    let vab = VabBank::upload(&mut spu, &mut alloc, &report, body);

    // Fire the Tactical-Arts strike cue: it must key a voice via the
    // tone-index path (the whole point of the fix).
    let voice = bank
        .play_one_shot(CUE_ART_STRIKE, &mut spu, &vab)
        .expect("strike cue 0x1A keys a voice against the class-2 bank");
    assert!(
        !spu.voices[voice as usize].is_off(),
        "strike cue voice is playing (not silence)"
    );

    // Fire the Baka Fighter exchange-hit cue on a clean SPU: same guarantee.
    let mut spu2 = Spu::new();
    let mut alloc2 = SpuAllocator::new(0x1000, 0x40_000);
    let vab2 = VabBank::upload(&mut spu2, &mut alloc2, &report, body);
    let vhit = bank
        .play_one_shot(CUE_BAKA_HIT, &mut spu2, &vab2)
        .expect("Baka hit cue 0x09 keys a voice against the class-2 bank");
    assert!(
        !spu2.voices[vhit as usize].is_off(),
        "Baka hit cue voice is playing (not silence)"
    );
    // 0x09 is a 2-voice cue: the second consecutive region keys the next voice.
    assert!(
        spu2.voices.iter().filter(|v| !v.is_off()).count() >= 2,
        "the 2-voice Baka hit cue keyed both of its voices"
    );

    eprintln!(
        "[ok] strike 0x1A + Baka 0x09 sound against class-2 bank PROT 0869 \
         ({} programs, {} samples)",
        report.tones.len(),
        report.vag_samples.len()
    );
}
