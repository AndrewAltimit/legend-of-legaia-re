//! Disc-gated: Noa's windowed battle clip draws her `0xFF` equipment variant
//! in its attach channel - the mesh-variant swap `FUN_8004CCD4` writing the
//! seat's per-channel model table and the draw pass `FUN_80048A08` drawing
//! what the table names - all the way into the mesh the native host uploads.
//!
//! Noa's walk (action slot 1) carries a live `+0xA4` window over its whole
//! length; her idle loop carries none
//! (`docs/formats/battle-data-pack.md` § Equipment-variant track). Seated
//! on the walk, the drawn set swaps the attach bone for the variant; back on
//! the idle loop, the bone comes back. The masked posed mesh the native window
//! builds (`battle_attach_swap::mask_tmd` + `tmd_to_vram_mesh_posed_rot`)
//! differs between the two only by that swap, and it never carries the
//! `0xFE` extra or the second variant on the 16-part walk. Skip-passes
//! without `LEGAIA_DISC_BIN` or the extracted tree.

use std::path::PathBuf;

use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
use legaia_engine_battle::battle_attach_swap::{mask_tmd, windows_from_entry};
use legaia_engine_core::battle_party_form::install_party_battle_forms;
use legaia_engine_core::world::World;
use legaia_engine_scene::scene::ProtIndex;

const NOA: usize = 1;

fn extracted() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(PathBuf::from("../../extracted"));
    dirs.push(PathBuf::from("extracted"));
    let d = dirs.into_iter().find(|d| d.join("PROT.DAT").is_file());
    if d.is_none() {
        eprintln!("[skip] extracted tree missing");
    }
    d
}

#[test]
fn noa_windowed_clip_draws_the_variant() {
    let Some(dir) = extracted() else {
        return;
    };
    let index = ProtIndex::open_extracted(&dir).expect("open extracted tree");
    let mut world = World::default();
    let blank = vec![0u8; 0x414];
    world.party.roster.members = (0..4)
        .map(|_| legaia_save::CharacterRecord::parse(&blank).expect("record"))
        .collect();
    world.party.party_count = 3;
    world.enter_battle(3, 1);
    let forms = install_party_battle_forms(&index, &mut world);
    let form = forms
        .forms
        .iter()
        .find(|f| f.member == NOA)
        .expect("Noa's form");
    assert!(form.assembled, "Noa assembles");
    let table = form.model_table.clone().expect("Noa's model table");
    let nobj = table.table.len();

    let clips = world.actors[NOA]
        .battle_action_clips
        .clone()
        .expect("action clips");
    let walk = clips[1].clone().expect("walk clip");
    let track = walk.variant_track;
    assert_eq!(
        track.stream_parts, table.idle_parts,
        "the walk is not a surplus swing"
    );
    let windows = windows_from_entry(&track.windows);
    let inside = (0..walk.frame_count as i16)
        .find(|&f| windows.iter().any(|w| w.active(f)))
        .expect("Noa's walk carries a live window");
    let idle = clips[0].clone().expect("idle clip");
    assert_eq!(
        idle.variant_track.windows, [0; 8],
        "the idle loop has no window"
    );

    let seat = |world: &mut World,
                clip: &legaia_actor_formats::monster_archive::MonsterAnimation,
                id: u8,
                frame: i16| {
        let player = MonsterAnimPlayer::new(clip).expect("player");
        world.set_actor_battle_animation(NOA, player);
        world.actors[NOA].battle.current_anim = id;
        assert!(world.seat_battle_clip_cursor(NOA, id, i32::from(frame) * 16));
        world.battle_live_drawn_objects(NOA).expect("drawn set")
    };
    // The walk's window first, then the idle loop: the swap writes the
    // default back on the first frame outside any window.
    let drawn_in = seat(&mut world, &walk, 1, inside);
    let pose = world.actors[NOA].pose_frame.clone().expect("pose");
    let drawn_out = seat(&mut world, &idle, 0, 0);

    // Out of the window: the bones only.
    let bones = usize::from(table.idle_parts);
    assert!(drawn_out[..bones].iter().all(|&d| d));
    assert!(drawn_out[bones..].iter().all(|&d| !d));
    // In it: exactly one attach channel swapped to its variant.
    let swapped: Vec<usize> = (0..nobj).filter(|&o| drawn_in[o] != drawn_out[o]).collect();
    assert_eq!(
        swapped.len(),
        2,
        "one bone out, one variant in: {swapped:?}"
    );
    let (bone, variant) = (swapped[0], swapped[1]);
    assert!(bone < bones && variant >= nobj - 2);

    // The native window's mesh: the masked posed rebuild, both sets under
    // the walk's pose so only the swap differs.
    let mesh = |drawn: &[bool]| {
        let t = mask_tmd(&form.tmd, drawn);
        legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&t, &form.tmd_bytes, &pose.bone_outputs)
    };
    let m_in = mesh(&drawn_in);
    let m_out = mesh(&drawn_out);
    let all = mesh(&vec![true; nobj]);
    assert_ne!(
        m_in.positions, m_out.positions,
        "the variant renders differently"
    );
    assert!(
        m_out.positions.len() < all.positions.len(),
        "surplus objects are not drawn"
    );
    eprintln!(
        "Noa walk: window frame {inside}, object {bone} -> variant {variant}; \
         {} / {} / {} vertices (in / out / every object)",
        m_in.positions.len(),
        m_out.positions.len(),
        all.positions.len()
    );
}
