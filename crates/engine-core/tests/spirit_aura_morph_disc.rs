//! The Spirit charge's aura, against the real disc: the Spirit clip's effect
//! script spawns the table-form prototypes `0x07` / `0x08` (PROT 0898's
//! `0x801F6324` table), whose move-VM programs arm a VDF morph lane on
//! `vdf.dat` (PROT 0872) entry 12 and hold it for `0x4F` frames. The part has
//! to (a) survive that hold - the wait timer drains at retail's per-frame
//! rate, not the scene-graph step - and (b) draw its **morphed** mesh, which
//! is what turns a small rest mesh into the cone around the actor.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::{GlobalTmd, SceneMode, World};
use legaia_prot::archive::Archive;

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file())
}

fn entry(archive: &mut Archive, idx: usize) -> Vec<u8> {
    let e = archive.entries.get(idx).cloned().expect("PROT entry");
    let mut bytes = Vec::new();
    archive.read_entry(&e, &mut bytes).expect("read entry");
    bytes
}

fn extent(g: &GlobalTmd) -> i32 {
    g.tmd
        .objects
        .iter()
        .flat_map(|o| o.vertices.iter())
        .map(|v| i32::from(v.x).abs().max(i32::from(v.z).abs()))
        .max()
        .unwrap_or(0)
}

#[test]
fn the_spirit_aura_prototypes_hold_and_draw_their_morphed_cone() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let overlay = entry(
        &mut archive,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.tables.move_power = Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("catalog"));
    world.tables.move_power_overlay = Some(Arc::from(overlay.as_slice()));
    world.tables.battle_vdf = Some(Arc::from(entry(&mut archive, 872).as_slice()));
    // The effect-model library (PROT 0871) at pool `3..`, as scene entry
    // seeds it. The pack spans the entry's extended footprint, so read the
    // next entry's bytes on too.
    let mut etmd = entry(&mut archive, 871);
    etmd.extend(entry(&mut archive, 872));
    for (i, body) in legaia_asset::pack::extract_pack(&etmd)
        .expect("etmd pack")
        .iter()
        .enumerate()
        .take(30)
    {
        if let Ok(tmd) = legaia_tmd::parse(body) {
            world.set_global_tmd(
                3 + i,
                Arc::new(GlobalTmd {
                    tmd,
                    raw: body.to_vec(),
                }),
            );
        }
    }

    assert!(world.spawn_action_table_effect(0x07, [0, 0, -800]));
    let mut grown = None;
    for tick in 0..30 {
        world.tick_move_fx(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        let draws = world.active_move_fx_part_draws();
        assert_eq!(draws.len(), 1, "the aura part is still up at tick {tick}");
        let d = draws[0];
        let rest = world
            .global_tmd(d.model_index as i16)
            .cloned()
            .expect("the aura's rest mesh is in the pool");
        if let Some(m) = world.morphed_part_tmd(&d) {
            grown = Some((extent(&rest), extent(&m)));
        }
    }
    let (rest, morphed) = grown.expect("the aura part armed a morph lane");
    assert!(
        morphed > rest * 2,
        "vdf.dat entry 12 grows the cone: rest extent {rest}, morphed {morphed}"
    );
}

/// The aura's growth, fade and spin against the two retail mid-Spirit
/// captures that hold it (the morph lane `+0xA0`, level `+0x78` and Y bank
/// `+0x26` of the live part beside its wait `+0x54`): `7` frames in, weight
/// `0x2CA`, level `0xC80`; `28` frames in, weight `0xB28`, level `0x200`. The
/// lane grows `0x66` a frame from the frame after the spawn, the level falls
/// `0x80` a frame (op `0x0D`'s `-0x80 << 3` rate through the part tick's
/// level block) toward `0` - black under an additive word, so the cone fades
/// **in** - and the bank turns `0x222` a frame.
#[test]
fn the_spirit_aura_grows_and_fades_at_the_captured_rates() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let overlay = entry(
        &mut archive,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.tables.move_power = Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("catalog"));
    world.tables.move_power_overlay = Some(Arc::from(overlay.as_slice()));
    world.tables.battle_vdf = Some(Arc::from(entry(&mut archive, 872).as_slice()));
    assert!(world.spawn_action_table_effect(0x07, [0, 0, -800]));
    let mut seen = Vec::new();
    for _ in 0..40 {
        world.tick_move_fx(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        let part = &world.casting.active_action_fx[0].parts[0];
        let s = &part.state;
        let weight = legaia_engine_vm::vdf_morph::actor_morph_lanes(s)
            .first()
            .map(|l| l.1)
            .unwrap_or(0);
        seen.push((s.wait_timer, weight, s.field_78, s.render_26));
        let draw = world.active_move_fx_part_draws()[0];
        assert!(draw.colour.semi && draw.colour.abr == 1, "an additive word");
        assert_eq!(
            draw.colour.ir0,
            (s.field_78 | 1).min(0x1000),
            "the level is the cue"
        );
        assert!(!draw.draws_rest_mesh());
    }
    let at = |wait: i16| {
        seen.iter()
            .find(|e| e.0 == wait)
            .copied()
            .unwrap_or_else(|| panic!("no tick at wait {wait}: {seen:?}"))
    };
    let (_, w7, l7, r7) = at(576);
    assert_eq!((w7, l7), (0x2CA, 0xC80), "7 frames in");
    let (_, w28, l28, r28) = at(408);
    assert_eq!((w28, l28), (0xB28, 0x200), "28 frames in");
    assert_eq!(
        r28.wrapping_sub(r7),
        0x222 * 21,
        "the bank spins 0x222 a frame"
    );
}
