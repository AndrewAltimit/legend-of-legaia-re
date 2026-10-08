//! Noa's dance rhythm minigame. The rules engine (beat clock, judge, groove
//! gauge, the floor cast and sprite-part pools, the camera track) lives in
//! `legaia_engine_minigames::dance` and is re-exported here; this file keeps
//! the one step that reads the disc through a [`crate::scene::ProtIndex`]:
//! staging the HUD art into a scene's VRAM.

pub use legaia_engine_minigames::dance::*;

/// Upload the dance HUD's own texture page and CLUT row into `vram`, leaving
/// every other rect of the resident scene alone. Returns how many TIMs were
/// uploaded.
///
/// `rects` comes from [`DanceGame::hud_vram_rects`], i.e. from the run's own
/// parsed widget table, so the pages staged are the pages the frame's quads
/// actually name. A member of the pack is uploaded when its image origin is
/// one of those pages; its CLUT block rides along, because a widget id names
/// a palette **column** of a strip that one member owns whole.
///
/// Why not the whole pack: the eleven other 256x256 members target `(576, 0)`
/// through `(768, 256)` - the columns a field scene's own texture pack
/// occupies - so uploading them would repaint the suspended town the port is
/// still drawing behind the HUD. That is a divergence retail cannot exhibit
/// and the port can, which is exactly the kind of difference worth spending a
/// rect filter on. Soft-fails to `0` when the entry is absent or carries no
/// member at a named page, which leaves a host on its placeholder text.
pub fn stage_dance_hud_vram(
    index: &crate::scene::ProtIndex,
    rects: &[DanceHudRect],
    vram: &mut legaia_tim::Vram,
) -> usize {
    if rects.is_empty() {
        return 0;
    }
    let Ok(raw) = index.entry_bytes_extended(DANCE_HUD_ART_PROT_ENTRY) else {
        return 0;
    };
    let mut uploaded = 0;
    for member in legaia_prot::timpack::unpack(&raw) {
        let Ok(tim) = legaia_tim::parse(&member) else {
            continue;
        };
        let origin = (tim.image.fb_x, tim.image.fb_y);
        if rects.iter().any(|&(page, _)| page == origin) {
            vram.upload_tim_partial(&tim, true, true);
            uploaded += 1;
        }
    }
    uploaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dance_view_window_is_centred_where_the_fields_is_offset() {
        let e = dance_scene_entry();
        let (x0, z0, x1, z1) = e.view_window;
        // Symmetric about the camera on both axes.
        assert_eq!(x1, -x0);
        assert_eq!(z1, -z0);
        assert_eq!(i32::from(x1 - x0), 16);
        assert_eq!(i32::from(z1 - z0), 20);

        // The field's default is offset instead - further ahead than behind
        // and further left than right - so the two are not the same box even
        // though both are deeper than wide.
        let f = crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
        assert_ne!(f.2, -f.0);
        assert_ne!(f.3, -f.1);
        assert_ne!(e.view_window, f);
        // And the dance floor is the larger box on both axes.
        assert!(x1 - x0 > f.2 - f.0);
        assert!(z1 - z0 > f.3 - f.1);

        // Y is above the origin: the dancer's spawn height is negative.
        assert!(e.dancer_spawn.1 < 0);
    }
}
