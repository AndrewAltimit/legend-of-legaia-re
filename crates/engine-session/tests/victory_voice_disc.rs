//! Disc-gated: a won battle's **victory voice** sounds.
//!
//! Retail's results sequencer `FUN_8004E568` streams the pose's voice - a
//! `monster.snd` bank picked off the SCUS voice table at `0x80078880` - into
//! VAB slot 7 over the formation's own bank (`FUN_8003E104(clip, 7, ..)` at
//! `0x8004ECE4`, `FUN_8001E54C(7, ..)` at `0x8004ED68`), and the win-pose
//! clip's cue track fires `0xA1`, a party cue below `0xA7`, which the funnel
//! `FUN_8004FE5C` keys in the actor's category `7`. Every one of the 24 win
//! poses carries that cue.
//!
//! Drives the Rim Elm Tetsu spar headless, cuts the monster to 1 HP so the
//! party's next hit wins, and waits for the pose clip's runtime cue. The
//! row's category must name slot 7, slot 7 must answer the voice bank
//! (`World::battle_monster_sound_banks`), and keying the row in that bank
//! must be audible.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_audio::{SfxBank, Spu};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::sound_state::SfxRingOp;
use legaia_scene_formats::encounter_record::RIM_ELM_TRAINING_FORMATION_ID;

/// The win poses' voice cue (`(frame, 0xA1)` on every base-archive pose
/// record), as the ring carries it: `0xA1 + 0x19C`.
const VOICE_RING_ID: i16 = 0xA1 + 0x19C;

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
fn the_win_pose_keys_its_voice_from_slot_7() {
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
    let mut session = NativeSession::open(&extracted, &cfg).expect("open");
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

    let mut hit: Option<([u8; 8], u16)> = None;
    for _ in 0..12000 {
        let _ = session.host.tick().expect("tick");
        let w = &mut session.host.world;
        let ops = w.take_sfx_ring_ops();
        if w.mode != SceneMode::Battle {
            continue;
        }
        if w.battle.victory.is_none() {
            for a in w.actors.iter_mut() {
                if a.battle_monster_id.is_some() && a.battle.hp > 1 {
                    a.battle.hp = 1;
                }
            }
            continue;
        }
        let voice = w
            .battle
            .victory
            .and_then(|v| v.voice_bank)
            .expect("a won battle streams a voice bank");
        assert_eq!(
            w.battle_monster_sound_banks()
                .iter()
                .find(|(slot, _)| *slot == 7)
                .map(|b| b.1),
            Some(voice),
            "slot 7 answers the voice bank"
        );
        for op in ops {
            let id = match op {
                SfxRingOp::Push(id) | SfxRingOp::PushUnlessSustained { id, .. } => id,
                _ => continue,
            };
            if id == VOICE_RING_ID
                && let Some(row) = w.runtime_sfx_descriptor(id)
            {
                hit = Some((row, voice));
            }
        }
        if hit.is_some() {
            break;
        }
    }
    let (row, bank) = hit.expect("the win pose's cue 0xA1 reaches the ring");
    assert_eq!(row[4], 7, "the party cue keys category 7: {row:02x?}");
    // The table's bands: Vahn's voices sit at banks `0xB8..=0xBC`.
    assert!(
        (0xB8..=0xCC).contains(&bank),
        "a hero voice bank: {bank:#x}"
    );

    let archive = session
        .host
        .index
        .entry_bytes_extended(
            legaia_container_formats::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32,
        )
        .expect("monster.snd");
    let bytes = legaia_container_formats::vab_multi_bank::bank_bytes(&archive, usize::from(bank))
        .expect("bank in the archive");
    let report = psx_vab::parse(bytes, 4).expect("the bank's VAB header at +4");
    let mut spu = Spu::new();
    let vab = legaia_engine_audio::bgm_tail::upload_at(&mut spu, 0x20000, &report, &bytes[4..]);
    let voice = SfxBank::default()
        .play_descriptor(&row, &mut spu, &vab)
        .unwrap_or_else(|| panic!("row {row:02x?} keys a voice in bank {bank:#x}"));
    let mut buf = vec![0i16; 2 * 8192];
    spu.render_into(&mut buf);
    let peak = buf.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    eprintln!("[ran] row {row:02x?} -> slot 7 bank {bank:#x} voice {voice} peak {peak}");
    assert!(peak > 0, "the victory voice is audible");
}
