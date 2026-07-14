//! Disc-gated coverage for the dance minigame's **presentation** API on
//! `LegaiaMinigames` (`minigames_dance.rs`) - the same surface the site's
//! minigames page drives, exercised natively so a schema break fails before
//! a browser ever sees it.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_web_viewer::minigames::LegaiaMinigames;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn dance_presentation_api_decodes() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };
    let bytes = std::fs::read(&prot).expect("read PROT.DAT");
    let mut mg = LegaiaMinigames::new();
    let status = mg.load_disc(bytes).expect("load_disc");
    assert!(
        status.contains(r#""art":true"#),
        "dance art should decode: {status}"
    );

    assert!(mg.dance_art_ready());

    // The HUD page decodes through the row-500 palettes the widgets name.
    for pal in [0usize, 5, 6, 8, 13, 14] {
        let page = mg.dance_hud_page_rgba(pal);
        assert_eq!(page.len(), 256 * 256 * 4, "palette {pal}");
        assert!(page.chunks_exact(4).any(|p| p[3] != 0));
    }

    // 34 widget records; spot-check the traced digit-font row.
    let widgets = mg.dance_widgets_json();
    let rows = widgets.matches("{\"u\":").count();
    assert_eq!(rows, 34, "widget table rows: {widgets}");

    // The traced layout parses and names the retail anchors.
    let layout = mg.dance_layout_json();
    for needle in [
        r#""screen":[320,240]"#,
        r#""screen_offset":[0,4]"#,
        r#""xs":[64,160,256]"#,
        r#""x":120,"y":192"#,
    ] {
        assert!(layout.contains(needle), "layout missing {needle}: {layout}");
    }

    // Face windows: Noa (rig 0, her field atlas) + the pack strips, every
    // pose non-empty.
    let meta = mg.dance_face_meta_json();
    assert!(meta.contains(r#""ok":true"#));
    for dancer in 0..3 {
        for pose in 0..4 {
            let face = mg.dance_face_rgba(dancer, pose);
            assert!(
                !face.is_empty() && face.chunks_exact(4).any(|p| p[3] != 0),
                "face {dancer} pose {pose} empty"
            );
        }
    }

    // The floor cast: Noa (kind 0, her field mesh) in the centre plus the two
    // dedicated dancer NPCs of the qualifier floor (kinds 2 and 3, the
    // dance-hall scene module's meshes), each with real geometry and the
    // scene's choreography clips.
    assert!(
        status.contains(r#""body":true"#),
        "dance cast should decode: {status}"
    );
    assert!(mg.dance_body_ready());
    assert_eq!(mg.dance_body_count(), 3, "three dancers on the floor");
    // The centre dancer is the human (Noa, kind 0); the AI dancers are the
    // scene NPCs the overlay's qualifier spawn table names (kinds 2 / 3 =
    // face-strip rigs 2 / 3), NOT party members.
    let human = mg.dance_body_human_index();
    assert_eq!(human, 1, "Noa dances centre on the qualifier floor");
    assert_eq!(
        mg.dance_body_kind(human),
        0,
        "centre dancer is kind 0 (Noa)"
    );
    assert_eq!(mg.dance_body_kind(0), 2, "left dancer is the rig-2 NPC");
    assert_eq!(mg.dance_body_kind(2), 3, "right dancer is the rig-3 NPC");
    for dancer in 0..mg.dance_body_count() {
        let pos = mg.dance_body_positions(dancer);
        assert!(!pos.is_empty(), "dancer {dancer} has no vertices");
        assert!(pos.len().is_multiple_of(3));
        let verts = pos.len() / 3;
        let idx = mg.dance_body_indices(dancer);
        assert!(
            !idx.is_empty() && idx.len().is_multiple_of(3),
            "dancer {dancer} idx"
        );
        assert!(idx.iter().all(|&i| (i as usize) < verts));
        let oids = mg.dance_body_object_ids(dancer);
        assert_eq!(oids.len(), verts, "dancer {dancer} object ids parallel");
        assert_eq!(mg.dance_body_uvs(dancer).len(), verts * 2);
        assert_eq!(mg.dance_body_cba_tsb(dancer).len(), verts * 2);
        assert_eq!(mg.dance_body_flat_rgba(dancer).len(), verts * 4);
        let parts = mg.dance_body_part_count(dancer);
        assert!(parts > 1, "dancer {dancer} is a multi-object rig");
        // Every clip slot (idle, the dance-groove loop, and the 11
        // judge-triggered moves) decodes with real dimensions, bones matching
        // the dancer's rig, and a pose stream padded to the mesh's parts.
        for clip in 0..13u32 {
            let dims = mg.dance_body_anim_dims(dancer, clip);
            assert_eq!(dims.len(), 2);
            assert!(
                dims[0] > 0 && dims[1] > 0,
                "dancer {dancer} clip {clip} dims: {dims:?}"
            );
            assert_eq!(
                dims[0],
                mg.dance_body_part_count(dancer),
                "dancer {dancer} clip {clip} bones match the rig"
            );
            let frames = mg.dance_body_pose_frames(dancer, clip, parts);
            assert_eq!(
                frames.len() as u32,
                dims[1] * parts * 6,
                "dancer {dancer} clip {clip} pose stream shape"
            );
            // A dance clip must actually move: some (frame, bone) transform
            // must differ from frame 0's.
            if dims[1] > 1 {
                let stride = (parts * 6) as usize;
                assert!(
                    frames
                        .chunks(stride)
                        .skip(1)
                        .any(|f| f != &frames[..stride]),
                    "dancer {dancer} clip {clip} is static"
                );
            }
        }
    }

    // The cast map the page drives the clips from.
    let cast = mg.dance_cast_json();
    for needle in [
        r#""human":1"#,
        r#""kind":0"#,
        r#""kind":2"#,
        r#""kind":3"#,
        r#""seq_square":[4,6,8]"#,
        r#""seq_circle":[5,7,9]"#,
        r#""beat":[10,11,12]"#,
    ] {
        assert!(cast.contains(needle), "cast missing {needle}: {cast}");
    }
    // The field VRAM the bodies sample is the full 1 MB PSX framebuffer.
    assert_eq!(mg.dance_body_vram().len(), 1024 * 512 * 2);

    // The dance hall itself: the other7 scene's placement + terrain layers
    // bake to one static mesh in the dancer frame (human spawn = origin).
    let env_pos = mg.dance_env_positions();
    assert!(!env_pos.is_empty(), "hall env baked");
    assert!(env_pos.len().is_multiple_of(3));
    let env_verts = env_pos.len() / 3;
    let env_idx = mg.dance_env_indices();
    assert!(!env_idx.is_empty() && env_idx.len().is_multiple_of(3));
    assert!(env_idx.iter().all(|&i| (i as usize) < env_verts));
    assert_eq!(mg.dance_env_uvs().len(), env_verts * 2);
    assert_eq!(mg.dance_env_cba_tsb().len(), env_verts * 2);
    assert_eq!(mg.dance_env_flat_rgba().len(), env_verts * 4);
    // The stage must surround the origin: geometry on every side of the
    // human spawn, the walkable floor near y = 0, and the ceiling above
    // (negative y in the retail Y-down frame).
    let (mut xs, mut ys, mut zs) = ((0f32, 0f32), (0f32, 0f32), (0f32, 0f32));
    for v in env_pos.chunks_exact(3) {
        xs = (xs.0.min(v[0]), xs.1.max(v[0]));
        ys = (ys.0.min(v[1]), ys.1.max(v[1]));
        zs = (zs.0.min(v[2]), zs.1.max(v[2]));
    }
    assert!(
        xs.0 < -300.0 && xs.1 > 300.0,
        "hall spans the origin in x: {xs:?}"
    );
    assert!(
        zs.0 < -300.0 && zs.1 > 200.0,
        "hall spans the origin in z: {zs:?}"
    );
    assert!(
        ys.0 < -300.0 && ys.1 > 50.0,
        "hall spans floor + ceiling: {ys:?}"
    );
    // Some of the hall's prims are ABE (the spotlight glows / smoke) - the
    // page's additive pass keys off TSB bit 15.
    assert!(
        mg.dance_env_cba_tsb()
            .iter()
            .skip(1)
            .step_by(2)
            .any(|&t| t & 0x8000 != 0),
        "hall carries ABE prims"
    );

    // SFX: the cue bank (PROT 1228 + the TOC-tail entry 1231) and the traced
    // cue ids all decode to PCM.
    let sfx = mg.dance_sfx_json();
    assert!(sfx.contains("\"id\":528"), "miss cue 0x210 present: {sfx}");
    for cue in [0x210u16, 0x202, 0x203, 0x205, 0x201] {
        let pcm = mg.dance_sfx_pcm(cue);
        assert!(!pcm.is_empty(), "cue {cue:#X} PCM empty");
        assert!(mg.dance_sfx_rate(cue) > 0);
    }

    // The direct-keyed hit stings (program 1, paired tones).
    for r in 0..3u8 {
        for layer in 0..2u8 {
            assert!(
                !mg.dance_sting_pcm(r, layer).is_empty(),
                "sting {r}/{layer} empty"
            );
        }
    }

    // BGM pair resolves; a short render produces non-silent stereo PCM.
    assert!(mg.dance_bgm_ready_json().contains(r#""ok":true"#));
    let pcm = mg.dance_bgm_pcm_i16(false, 2.0);
    assert_eq!(pcm.len(), 2 * 2 * 44100);
    assert!(pcm.iter().any(|&s| s != 0), "BGM rendered silent");
}
