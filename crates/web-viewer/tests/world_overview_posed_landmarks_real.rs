//! Disc-gated: the world-overview page poses the overworld landmarks whose
//! object bind names a clip, at the frame their spawn prologue picks under
//! the page's story flags.
//!
//! Conkram's shell lid (`map03` record 458, pack slot 16, anchor `(66, 104)`,
//! clip 4) is the case the page drew wrong: its walk stamp resolved straight
//! from the `.MAP`, unposed, so the lid lay flat in front of the town. Its
//! prologue (`map03` `P0[0]`) runs `74 C8 07 00`: flag `0x4C8` clear takes
//! the `4C 35` arm (frame 0, the lid standing open behind the town - a
//! retail capture beside the town holds the actor at cursor `0`), set takes
//! the `4C 36` arm (the last of 15 frames, the lid shut over the town).
//! `0x4C8` is raised by `chitei2`'s entry script once the collapse countdown
//! (`0x4C7`) has run out.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::LegaiaViewer;

const KARISTO: u32 = 391;
const SHELL_SLOT: u32 = 16;
const SHELL_FLAG: u16 = 0x4C8;

fn loaded() -> Option<LegaiaViewer> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = match std::fs::read(&disc) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[skip] {disc}: {e}");
            return None;
        }
    };
    let mut v = LegaiaViewer::new_headless();
    v.load_disc(bytes).ok()?;
    Some(v)
}

/// The shell's walk-placement index: the one placed stamp of pack slot 16.
fn shell_index(v: &LegaiaViewer) -> usize {
    let slots = v.walk_placement_slots();
    let placed = v.walk_placement_placed();
    let hits: Vec<usize> = (0..slots.len())
        .filter(|&i| slots[i] == SHELL_SLOT && placed[i] == 1)
        .collect();
    assert_eq!(hits.len(), 1, "one placed shell stamp");
    hits[0]
}

/// Mean Y and Z of the shell lid's posed vertices (retail frame, +Y down).
fn posed_centroid(v: &mut LegaiaViewer, anim: u32, key: i32) -> (f32, f32) {
    v.pack_mesh_posed(SHELL_SLOT, anim, key)
        .expect("pack_mesh_posed");
    let p = v.pack_mesh_positions();
    assert!(!p.is_empty(), "the posed lid has geometry");
    let n = (p.len() / 3) as f32;
    let y = p.iter().skip(1).step_by(3).sum::<f32>() / n;
    let z = p.iter().skip(2).step_by(3).sum::<f32>() / n;
    (y, z)
}

#[test]
fn conkrams_shell_poses_from_the_arm_its_flag_picks() {
    let Some(mut v) = loaded() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping posed-landmark test");
        return;
    };
    v.set_scene_kingdom(KARISTO)
        .expect("set_scene_kingdom(map03)");
    assert_eq!(
        v.kingdom_landmark_story_flags(),
        vec![SHELL_FLAG],
        "the shell's prologue is map03's only flag-armed posed landmark"
    );
    let i = shell_index(&v);
    let anims = v.walk_placement_anim_ids();
    assert_eq!(anims[i], 4, "the shell carries its bind's clip");
    // Every decoration stamp and every unposed landmark stays raw.
    let posed = anims.iter().filter(|&&a| a != 0).count();
    assert_eq!(posed, 1, "map03 poses one landmark");

    // A new game: frame 0, the lid standing open.
    let open_key = v.walk_placement_pose_keys()[i];
    assert_eq!(open_key & 0xFFFF, 0, "flag clear: cursor 0 (frame 0)");
    let open = posed_centroid(&mut v, 4, open_key);

    // After Jette's Fortress: the last frame.
    v.set_kingdom_landmark_flags(vec![SHELL_FLAG]);
    let shut_key = v.walk_placement_pose_keys()[i];
    assert_eq!(
        shut_key & 0xFFFF,
        14 * 16,
        "flag set: the last of 15 frames"
    );
    let shut = posed_centroid(&mut v, 4, shut_key);

    // Frame 0 lifts the lid (retail +Y is down) and carries it north; the
    // last frame lowers it back over the town.
    assert!(
        open.0 < shut.0,
        "the open lid stands higher than the shut one: {open:?} vs {shut:?}"
    );

    // The raw build is what the page drew before: a pose that resolves
    // differs from it.
    v.pack_mesh(SHELL_SLOT).expect("pack_mesh");
    let raw = v.pack_mesh_positions();
    v.pack_mesh_posed(SHELL_SLOT, 4, open_key)
        .expect("pack_mesh_posed");
    let posed = v.pack_mesh_positions();
    assert_eq!(raw.len(), posed.len(), "same stream shape as the raw mesh");
    assert_ne!(raw, posed, "the pose moves the lid");
}

#[test]
fn the_other_kingdoms_pose_their_bound_landmarks() {
    let Some(mut v) = loaded() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping posed-landmark test");
        return;
    };
    // `map01` record 409 (clip 5) tests `0x27B`; `map02` records 476 / 477
    // (clip 4, three stamps) test `0x1D5`.
    for (base, flag, stamps) in [(85u32, 0x27Bu16, 1usize), (244, 0x1D5, 3)] {
        v.set_scene_kingdom(base).expect("set_scene_kingdom");
        assert!(
            v.kingdom_landmark_story_flags().contains(&flag),
            "PROT {base}: the posed landmarks test {flag:#X}"
        );
        let posed = v
            .walk_placement_anim_ids()
            .iter()
            .filter(|&&a| a != 0)
            .count();
        assert_eq!(posed, stamps, "PROT {base}: posed stamps");
    }
}
