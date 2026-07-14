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

    // Dancer bodies: Noa's field-form mesh (PROT 0874 §0 slot 1) plus the two
    // AI dancers (Vahn / Gala), each with real geometry and a beat pose bank.
    assert!(
        status.contains(r#""body":true"#),
        "dancer bodies should decode: {status}"
    );
    assert!(mg.dance_body_ready());
    assert_eq!(mg.dance_body_count(), 3, "three dancer bodies");
    // The centre dancer is the human (Noa), drawn from pack slot 1.
    assert_eq!(mg.dance_body_human_index(), 1);
    assert_eq!(
        mg.dance_body_char_slot(mg.dance_body_human_index()),
        1,
        "centre dancer is Noa (field pack slot 1)"
    );
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
        // Both clips (idle + walk) decode with real dimensions and a pose
        // stream padded to the mesh's part count.
        for clip in 0..2u32 {
            let dims = mg.dance_body_anim_dims(dancer, clip);
            assert_eq!(dims.len(), 2);
            assert!(
                dims[0] > 0 && dims[1] > 0,
                "dancer {dancer} clip {clip} dims"
            );
            let frames = mg.dance_body_pose_frames(dancer, clip, parts);
            assert_eq!(
                frames.len() as u32,
                dims[1] * parts * 6,
                "dancer {dancer} clip {clip} pose stream shape"
            );
        }
    }
    // The field VRAM the bodies sample is the full 1 MB PSX framebuffer.
    assert_eq!(mg.dance_body_vram().len(), 1024 * 512 * 2);

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
