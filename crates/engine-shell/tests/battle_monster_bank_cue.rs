//! Disc-gated: a **monster-side** battle cue resolves to a loaded sample.
//!
//! Drives the Rim Elm Tetsu spar headless until the monster's own animation
//! cue track fires a runtime-bank cue (ring id `>= 0x200`, the `bse.dat` row
//! the funnel `FUN_8004FE5C` writes the monster's category into), then
//! resolves that row the way both hosts' ring drain does: the row's `+4`
//! category names the VAB slot, which must be one of the battle's
//! `monster.snd` banks (`World::battle_monster_sound_banks`, slot 7 = bank
//! `min id - 1`). The bank is uploaded with the hosts' own tail kernel
//! (`bgm_tail::upload_at`) and the row is keyed through
//! `SfxBank::play_descriptor`; the rendered SPU output must be non-silent.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_audio::{SfxBank, Spu};
use legaia_engine_core::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;
use legaia_engine_core::world::{SceneMode, SfxRingOp};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

/// `(VAB slot, monster.snd bank)` pairs.
type MonsterBanks = Vec<(u8, u16)>;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn a_monster_cue_keys_a_sample_from_its_monster_snd_bank() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open");
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("field");
    session
        .host
        .world
        .install_man_formation(RIM_ELM_TRAINING_FORMATION_ID);
    assert!(session.host.world.on_field_step());

    // Drive the (AI-fought) spar until a monster-side runtime cue fires.
    let mut hit: Option<(i16, [u8; 8], MonsterBanks)> = None;
    for _ in 0..6000 {
        let _ = session.host.tick().expect("tick");
        let w = &mut session.host.world;
        let ops = w.take_sfx_ring_ops();
        if w.mode != SceneMode::Battle {
            continue;
        }
        for op in ops {
            if let SfxRingOp::Push(id) = op
                && id >= 0x200
                && let Some(row) = w.runtime_sfx_descriptor(id)
                && (row[4] == 7 || row[4] == 8)
            {
                hit = Some((id, row, w.battle_monster_sound_banks()));
            }
        }
        if hit.is_some() {
            break;
        }
    }
    let (id, row, banks) = hit.expect("a monster-side runtime cue fires in the spar");
    // Tetsu (`0x4F`) alone: one bank, slot 7, index `0x4F - 1`.
    assert_eq!(banks, vec![(7, 0x4E)], "the formation's monster.snd banks");
    let bank = banks
        .iter()
        .find(|&&(slot, _)| slot == row[4])
        .map(|&(_, b)| b)
        .expect("the row's category names a staged slot");

    let archive = session
        .host
        .index
        .entry_bytes_extended(legaia_asset::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
        .expect("monster.snd");
    let bytes = legaia_asset::vab_multi_bank::bank_bytes(&archive, usize::from(bank))
        .expect("bank in the archive");
    let report = legaia_vab::parse(bytes, 4).expect("the bank's VAB header at +4");
    let mut spu = Spu::new();
    let vab = legaia_engine_audio::bgm_tail::upload_at(&mut spu, 0x20000, &report, &bytes[4..]);
    let voice = SfxBank::play_descriptor(&row, &mut spu, &vab)
        .unwrap_or_else(|| panic!("cue {id:#x} row {row:02x?} keys a voice in bank {bank}"));
    let mut buf = vec![0i16; 2 * 8192];
    spu.render_into(&mut buf);
    let peak = buf.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    eprintln!(
        "[ran] cue {id:#x} row {row:02x?} -> slot {} bank {bank} voice {voice} peak {peak}",
        row[4]
    );
    assert!(peak > 0, "the monster cue is audible");
}
