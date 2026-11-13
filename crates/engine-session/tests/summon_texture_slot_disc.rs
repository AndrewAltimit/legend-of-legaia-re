//! Disc-gated regression (#599): a summon creature's texture never lands on
//! a seated monster's.
//!
//! Retail's loader `FUN_800542C8` uploads one texture pool per distinct
//! monster id, in ascending id order, into battle slots `0, 1, ..`; a
//! repeated species shares its first seat's slot. Every summon creature's
//! pool then goes into slot `2` (`FUN_801F19EC`, `li a3,0x2` at
//! `0x801F1D14`). The port used to key the slot on the seat, so in Mt.
//! Rikuroa's `[Worm, Pump Bat, Pump Bat]` the second Pump Bat sat on slot
//! `2`, and a Vera cast repainted it in the creature's silver and purple for
//! the rest of the fight.
//!
//! Seeded from the ladder card's first save (Mt. Rikuroa): the formation is
//! entered through the ordinary encounter path, the stage installs each
//! monster's slot, the monster pools and then Vera's creature are written
//! into one VRAM as both hosts do, and every monster's CLUT row and page
//! must read the same before and after the creature lands.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`, an extracted tree or the save
//! library.

use legaia_engine_save_state::WorldSaveState;
use std::path::PathBuf;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::battle_stage;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

/// Vera's Seru magic.
const VERA: u8 = 0x83;
const PUMP_BAT: u16 = 46;
const WORM: u16 = 34;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn first_dir(env: &str, rel: &str, probe: &str) -> Option<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .into_iter()
        .chain([repo_root().join(rel)])
        .find(|d| d.join(probe).exists())
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

/// A monster texture slot's VRAM footprint: its CLUT row (`484 + slot`,
/// 256 halfwords) and its page column (`(320 + slot * 64, 256)`, 64 x 256).
fn slot_footprint(vram: &psx_tim::Vram, slot: u8) -> Vec<u16> {
    let slot = usize::from(slot);
    let row = 484 + slot;
    let mut out: Vec<u16> = (0..256).map(|x| vram.pixel(x, row)).collect();
    for y in 256..512 {
        for x in 0..64 {
            out.push(vram.pixel(320 + slot * 64 + x, y));
        }
    }
    out
}

#[test]
fn a_summon_creature_leaves_the_monsters_textures_alone() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = first_dir("LEGAIA_EXTRACTED_DIR", "extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let Some(library) = first_dir("LEGAIA_SAVES_LIBRARY", "saves/library", "cards") else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return;
    };
    let Ok(card) = legaia_save::emu::MountedCard::open(
        &library
            .join("cards")
            .join("playthrough-ladder-pro00-14.mcr"),
    ) else {
        eprintln!("[skip] ladder card not in the library");
        return;
    };
    let Some(sc) = card.sc_block(1).map(<[u8]>::to_vec) else {
        eprintln!("[skip] ladder card block 1 empty");
        return;
    };
    let scene = ascii(&sc[0x408..0x410]);
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open NativeSession");
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift the SC block");
    session.host.world.load_full(sf.clone());
    let opts = FieldLiveOpts {
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    };
    if !session.resume_save(sf, &scene, &opts).entered_scene() {
        eprintln!("[skip] ladder card block 1 does not resume into {scene}");
        return;
    }
    // The formation with two Pump Bats and a Worm, wherever it sits.
    let w = &session.host.world;
    let Some(fid) = w.registered_formation_ids().into_iter().find(|&id| {
        w.tables.formation_table.formation(id).is_some_and(|d| {
            let mut ids: Vec<u16> = d.slots.iter().map(|s| s.monster_id).collect();
            ids.sort_unstable();
            ids == [WORM, PUMP_BAT, PUMP_BAT]
        })
    }) else {
        eprintln!("[skip] {scene} registers no [Worm, Pump Bat, Pump Bat] formation");
        return;
    };
    assert!(session.host.world.force_encounter(fid));
    for _ in 0..600 {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(
        session.host.world.mode,
        SceneMode::Battle,
        "the fight opens"
    );

    let archive = session
        .host
        .index
        .entry_bytes(battle_stage::MONSTER_ARCHIVE_PROT_INDEX)
        .expect("monster archive");
    battle_stage::stage_battle_world(&mut session.host, None, Some(&archive));

    // Retail's slots: the Worm (the lower id) first, the Pump Bats sharing
    // the next one; slot 2 is nobody's.
    let mut vram = psx_tim::Vram::new();
    let mut monster_slots: Vec<(u16, u8)> = Vec::new();
    for (actor, id, slot) in session.host.world.battle_monster_texture_slots() {
        assert_eq!(
            session.host.world.actors[actor].battle_tex_slot,
            Some(slot),
            "the stage installs the slot the renderers inject at"
        );
        let mesh = legaia_actor_formats::monster_archive::mesh(&archive, id)
            .expect("decode")
            .expect("mesh");
        mesh.battle_render_mesh(slot, &mut vram)
            .expect("monster mesh");
        monster_slots.push((id, slot));
    }
    monster_slots.sort_unstable();
    assert_eq!(
        monster_slots,
        vec![(WORM, 0), (PUMP_BAT, 1), (PUMP_BAT, 1)],
        "slots follow the ids"
    );
    let before: Vec<Vec<u16>> = monster_slots
        .iter()
        .map(|&(_, s)| slot_footprint(&vram, s))
        .collect();

    // Vera's creature lands where every summon's does.
    let seat = battle_stage::seat_summon_creature(&mut session.host, VERA, &archive, None)
        .expect("Vera's creature seats");
    assert_eq!(seat.tex_slot, battle_stage::SUMMON_CREATURE_TEX_SLOT);
    seat.asset
        .mesh
        .battle_render_mesh(seat.tex_slot, &mut vram)
        .expect("creature mesh");
    for (&(id, slot), was) in monster_slots.iter().zip(&before) {
        assert_ne!(
            slot, seat.tex_slot,
            "monster {id} shares the creature's slot"
        );
        assert!(
            slot_footprint(&vram, slot) == *was,
            "monster {id}'s texture (slot {slot}) changed under the summon"
        );
    }
    eprintln!(
        "[ok] {scene} F{fid}: monster slots {monster_slots:?}, creature on {}",
        seat.tex_slot
    );
}
