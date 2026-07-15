//! Disc-gated: the opening cutscene's "It was the Seru." caption is a decoded
//! scene-texture image, shown as a bounded beat in the gap between `opdeene`'s
//! two narration crawl blocks.
//!
//! Cold-boots `opdeene` live through `SceneHost` and asserts:
//!
//! 1. entering `opdeene` decodes the caption image (a 112x32 RGBA with a
//!    transparent background and opaque white glyphs), and it starts hidden
//!    (`cutscene_caption_alpha == 0`);
//! 2. while a crawl block is on screen the caption stays hidden (it would
//!    overlap the scrolling text);
//! 3. in the first gap after a block has shown (narration inactive) the
//!    caption fades IN (alpha rises to full) - at the retail-video-pinned
//!    crawl rate the blocks run back-to-back, so this gap lands after the
//!    LAST crawl, over the held villager tableau (where the retail capture
//!    shows the caption);
//! 4. it is bounded to a retail-like beat: after the hold it fades back OUT
//!    to hidden, so the post-crawl hold never leaves the caption frozen on
//!    screen.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

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
fn opdeene_caption_is_a_bounded_beat_in_the_crawl_gap() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene(cutscene, 0).expect("enter opdeene");

    // 1. The caption image decoded from PROT 0749, and starts hidden.
    let cap = host
        .world
        .cutscene_caption
        .as_ref()
        .expect("opdeene decodes the 'It was the Seru.' caption image");
    assert_eq!((cap.width, cap.height), (112, 32), "112x32 caption strip");
    assert_eq!(
        cap.rgba.len(),
        (112 * 32 * 4) as usize,
        "RGBA8 buffer matches the dimensions"
    );
    let transparent = cap.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
    let opaque_bright = cap
        .rgba
        .chunks_exact(4)
        .filter(|p| p[3] == 255 && p[0] > 170)
        .count();
    assert!(
        transparent > 0 && opaque_bright > 0,
        "the caption has a transparent background ({transparent} px) and opaque glyphs ({opaque_bright} px)"
    );
    assert_eq!(
        host.world.cutscene_caption_alpha, 0.0,
        "the caption starts hidden"
    );

    // 2. Tick to the first crawl block. While it is on screen the caption is
    //    hidden (alpha stays 0 - it must not overlap the scrolling text).
    let mut ticked = 0u32;
    while !host.world.cutscene_narration_active() && ticked < 600 {
        let _ = host.world.tick();
        assert_eq!(
            host.world.cutscene_caption_alpha, 0.0,
            "caption hidden before any crawl opens (tick {ticked})"
        );
        ticked += 1;
    }
    assert!(
        host.world.cutscene_narration_active(),
        "the timeline reaches crawl block 1 within {ticked} ticks"
    );
    assert_eq!(host.world.cutscene_narration_seq, 1, "block 1 is the first");
    // Tick a chunk while block 1 is still scrolling; the caption stays hidden.
    for _ in 0..60 {
        if !host.world.cutscene_narration_active() {
            break;
        }
        let _ = host.world.tick();
        assert_eq!(
            host.world.cutscene_caption_alpha, 0.0,
            "caption stays hidden while crawl block 1 is on screen"
        );
    }

    // 3. The caption fades IN to full in the first post-block gap. While any
    // crawl is on screen the alpha stays 0 (asserted inside the loop). The
    // budget covers the video-pinned crawl rate (1 px per 6 retail frames)
    // across every block's pages.
    let mut peak_alpha = 0.0f32;
    let mut faded_in = false;
    for _ in 0..40000 {
        let _ = host.world.tick();
        if host.world.cutscene_narration_active() {
            // A block can open during a brief gap's fade-in tail; the gate
            // then targets 0 again, so the caption never reaches full while
            // a crawl is on screen.
            assert!(
                host.world.cutscene_caption_alpha < 0.99,
                "caption never fully shown while a crawl block is on screen"
            );
        }
        peak_alpha = peak_alpha.max(host.world.cutscene_caption_alpha);
        if host.world.cutscene_caption_alpha >= 0.99 {
            faded_in = true;
            break;
        }
    }
    assert!(
        faded_in,
        "the caption fades in to full in the first post-block gap (peak {peak_alpha})"
    );
    assert!(
        !host.world.cutscene_narration_active(),
        "the caption shows only in a gap (no crawl on screen)"
    );

    // 4. Bounded beat: after the ~2 s hold it fades back OUT to hidden (the
    //    post-crawl hold must not leave the caption frozen on screen).
    let mut faded_out = false;
    for _ in 0..40000 {
        let _ = host.world.tick();
        if host.world.cutscene_caption_alpha <= 0.0 {
            faded_out = true;
            break;
        }
    }
    assert!(
        faded_out,
        "the caption is bounded to a beat and fades back out within the gap"
    );
    eprintln!(
        "[opdeene] caption faded in (peak {peak_alpha}) and back out within the post-block gap"
    );
}
