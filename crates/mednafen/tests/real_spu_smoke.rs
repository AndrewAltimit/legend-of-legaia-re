//! Smoke test: lift the SPU section out of a real mednafen save state and
//! confirm the targeted field-name strings resolve. Disc-gated per the
//! `LEGAIA_DISC_BIN` skip-pass convention - tests pass-skip when the env
//! var isn't set so CI works without redistributing Sony data.
//!
//! Sister to `crates/save/tests/real_card_roundtrip.rs`: walks
//! `~/.mednafen/mcs` for a real Legaia save state and asserts every
//! [`PsxSpu`] field name resolves to a non-empty value.

use std::path::PathBuf;

use legaia_mednafen::{PsxSpu, SPU_NUM_VOICES, SPU_RAM_BYTES, SaveState};

fn mednafen_mcs_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("LEGAIA_MEDNAFEN_DIR") {
        return Some(PathBuf::from(v));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".mednafen").join("mcs"))
}

fn find_legaia_save() -> Option<PathBuf> {
    let mcs = mednafen_mcs_dir()?;
    let entries = std::fs::read_dir(&mcs).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s.contains("Legend of Legaia") && (s.ends_with(".mc0") || s.contains(".mc")) {
            return Some(entry.path());
        }
    }
    None
}

#[test]
fn real_save_state_exposes_spu_voice_fields() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(save_path) = find_legaia_save() else {
        eprintln!("[skip] no Legend of Legaia save state in mednafen mcs/ dir");
        return;
    };
    let save = SaveState::from_path(&save_path).expect("parse save state");
    let spu = PsxSpu::new(&save);

    // SPURAM is the largest entry; ensure it resolves to exactly 512 KiB.
    let ram = spu
        .spu_ram_bytes()
        .expect("SPURAM entry exists in real save");
    assert_eq!(ram.len(), SPU_RAM_BYTES, "SPURAM should be 512 KiB exactly");

    // Voices array indices must all resolve to populated snapshots in a
    // real save state (mednafen always serialises every voice regardless
    // of whether it's active).
    let voices = spu.voices();
    assert_eq!(voices.len(), SPU_NUM_VOICES);
    for (i, v) in voices.iter().enumerate() {
        assert!(
            v.start_addr.is_some(),
            "voice {i}: StartAddr should resolve in a real save state"
        );
        assert!(
            v.adsr_phase.is_some(),
            "voice {i}: ADSR.Phase should resolve in a real save state"
        );
        assert!(
            v.adsr_control.is_some(),
            "voice {i}: ADSRControl should resolve in a real save state"
        );
    }

    // Global registers - all should be present.
    assert!(spu.voice_on_mask().is_some(), "VoiceOn missing");
    assert!(spu.voice_off_mask().is_some(), "VoiceOff missing");
    assert!(spu.block_end_mask().is_some(), "BlockEnd missing");
    assert!(spu.reverb_mode().is_some(), "Reverb_Mode missing");
    assert!(spu.spu_control().is_some(), "SPUControl missing");
    assert!(spu.master_volume().is_some(), "GlobalSweep current missing");

    // Reverb routing (the C7-REVERB finding): retail master-enables reverb
    // and runs the Studio C preset globally, with most voices reverb-routed.
    assert!(spu.regs().is_some(), "Regs shadow missing");
    assert_eq!(
        spu.reverb_master_enabled(),
        Some(true),
        "retail master-enables reverb in every captured state"
    );
    let rr = spu.reverb_registers().expect("reverb registers present");
    // dAPF1/dAPF2 are the Studio C signature.
    assert_eq!(
        (rr[0], rr[1]),
        (0x00E3, 0x00A9),
        "retail reverb preset is Studio C (dAPF1/dAPF2)"
    );
    // mednafen's `Reverb_Mode` field mirrors the per-voice EON mask exactly.
    assert_eq!(
        spu.reverb_mode().map(|m| m & 0x00FF_FFFF),
        spu.voice_reverb_mask(),
        "Reverb_Mode field equals the EON voice-reverb-enable mask"
    );
    assert_ne!(
        spu.voice_reverb_mask(),
        Some(0),
        "retail routes voices into reverb by default"
    );

    // Diagnostic: count how many voices the save state captured as active.
    let active = voices.iter().filter(|v| v.is_active()).count();
    let mvol = spu.master_volume().unwrap();
    eprintln!(
        "[ok] {} active voices, master ({}, {}), reverb mode {:?}",
        active,
        mvol.0,
        mvol.1,
        spu.reverb_mode()
    );
}
