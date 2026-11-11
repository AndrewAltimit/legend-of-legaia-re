//! Disc-gated: Mt. Rikuroa's second rockfall leaves its rock lying across
//! the cave path, as retail draws it.
//!
//! `rikuroa` `P2[45]` (walk-on tile `(64, 109)`, flag `0x282`) installs
//! effect tree `34 30 0D`. Its mesh part (`model_sel 64`, scene-pack slot
//! `59`) is a stalactite at rest, hanging from the cave roof; move-VM op
//! `0x0A` arms VDF lanes `0x0D..=0x12` and the envelope ramps each to full
//! weight, which lays the rock across the path at rows `109..110`. The
//! record also paints the walls `(64..66, 109)` there, and its post-fall arm
//! hops the player over them - the hop is retail's, over the rock retail
//! draws.
//!
//! The case-5 model draw `FUN_8001ADA4` runs the morph substitution
//! `FUN_8001C604` per group while the part's `+0x10` bit `0x1000` is up
//! (`0x8001B43C..0x8001B448`), so the part draws its own staged vertices.
//! Both hosts drew the part's rest mesh instead: the stalactite, up in the
//! roof, and nothing on the path the player then hopped over. A retail film
//! of the beat (PCSX-Redux, `autorun_seat_walk_film.lua` from
//! `rikuroa_field_card_walk` with `0x282` / `0x300` cleared, seated through
//! the cave mouth's teleport onto the walk-on tile) shows the rock on the
//! path through and after Terra's lines.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::effect_draws::effect_model_draws;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `P2[45]`'s walk-on tile, at its centre.
const TRIGGER: (i16, i16) = (64 * 128 + 64, 109 * 128 + 64);
/// The rock's scene-pack slot (`model_sel 64` less the five character
/// meshes ahead of the pack).
const ROCK_SLOT: usize = 59;

/// World-space `(min, max)` of a TMD's vertices placed at `pos`.
fn bounds(tmd: &legaia_tmd::Tmd, pos: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in tmd.objects.iter().flat_map(|o| &o.vertices) {
        for (k, c) in [v.x, v.y, v.z].into_iter().enumerate() {
            let w = pos[k] + f32::from(c);
            lo[k] = lo[k].min(w);
            hi[k] = hi[k].max(w);
        }
    }
    (lo, hi)
}

#[test]
fn the_second_rockfall_draws_its_rock_across_the_path() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, "rikuroa", &FieldLiveOpts::default());
    // The arrival beat and the first rockfall have played.
    for f in [0x2FB, 0x281] {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..120 {
        s.tick().expect("tick");
    }
    assert!(s.host.world.debug_seat_player(TRIGGER.0, TRIGGER.1));
    s.host.world.refresh_region_attributes_at_player();
    s.camera.zone.arm_arrival();
    for t in 0..600 {
        s.host.world.set_pad(if t % 8 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
    }
    let w = &s.host.world;
    assert!(w.system_flag_test(0x282), "P2[45] ran");

    let rest = {
        let scene = s.host.scene.as_ref().expect("rikuroa loaded");
        let res = s.host.resources.as_ref().expect("scene resources");
        let env = legaia_engine_scene::field_env::env_pack_tmd_indices(scene, res);
        res.tmds[env[ROCK_SLOT]].tmd.clone()
    };
    let lists = effect_model_draws(w, None, |_| None);
    let rock = lists
        .field_parts
        .iter()
        .find(|fp| fp.draw.model_index == ROCK_SLOT)
        .expect("the rock part is drawn after the fall");
    assert_eq!(rock.draw.morph.count, 6, "lanes 0x0D..=0x12 armed");
    let pos = rock.draw.world_pos;

    // At rest the mesh hangs from the roof, well above the path.
    let (_, rest_hi) = bounds(&rest, pos);
    assert!(
        rest_hi[1] < -350.0,
        "rest pose is the stalactite: {rest_hi:?}"
    );

    // The draw both hosts take: the part's own staged vertices.
    let fallen = w
        .part_morphed_tmd(&rock.draw, &rest)
        .expect("the part's lanes resolve against the scene VDF");
    let (lo, hi) = bounds(&fallen, pos);
    eprintln!("rock at {pos:?}: fallen {lo:?}..{hi:?}");
    // Lying on the path (Y down: the floor is the largest Y) ...
    assert!(lo[1] > -400.0 && hi[1] > -300.0, "fallen: {lo:?}..{hi:?}");
    // ... and across the tiles whose walls the record paints and the hop
    // clears: columns 64..=66 of row 109 / 110.
    for col in 64..=66 {
        let x = (col * 128 + 64) as f32;
        assert!(lo[0] <= x && x <= hi[0], "column {col} under the rock");
    }
    let (row_lo, row_hi) = ((lo[2] as i32) >> 7, (hi[2] as i32) >> 7);
    assert!(row_lo <= 110 && row_hi >= 109, "rows {row_lo}..={row_hi}");
}
