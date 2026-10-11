//! `ScreenFxHost`: the widget pool a play host ticks, and the PROT 0900 overlay addresses the widgets were read from.

use super::*;

/// Runtime host for the four widget kinds - the engine face of retail's
/// PROT-0900 actor pool, driven by the field-VM op `0x43` sub-ops
/// `0x10`/`0x11`/`0x13`/`0x14`/`0x15` (the ending-scene widget family).
/// The field host routes each sub-op here; [`Self::tick`] advances every
/// live widget one frame and returns the draw list.
#[derive(Debug, Default, Clone)]
pub struct ScreenFxHost {
    /// Screen geometry the widgets draw against.
    pub screen: Screen,
    /// The screen-mask (iris) widget; spawned on the first sub-0x11.
    pub mask: Option<MaskWidget>,
    /// Live scripted sprite widgets, each with its spawn-record bytes
    /// (the widget script cursor indexes into them).
    pub sprites: Vec<(SpriteWidget, Vec<u8>)>,
    /// The image-panel widget (sub-0x13 spawn / sub-0x14 move-scale).
    pub panel: Option<PanelWidget>,
    /// The letterbox widget (sub-0x15 config).
    pub letterbox: Option<Letterbox>,
}

impl ScreenFxHost {
    /// `true` when any widget is live (a renderer can skip the pass
    /// entirely otherwise).
    pub fn is_active(&self) -> bool {
        self.mask.is_some()
            || self.panel.is_some()
            || self.letterbox.is_some()
            || !self.sprites.is_empty()
    }

    /// Drop every widget (scene teardown).
    pub fn clear(&mut self) {
        self.mask = None;
        self.sprites.clear();
        self.panel = None;
        self.letterbox = None;
    }

    /// Op `0x43` sub-0x10: spawn a scripted sprite widget from its inline
    /// 19-byte record (`FUN_801F8004`). `payload` is the record **and the
    /// field-script bytes after it**: the spawner seeds the widget cursor at
    /// record `+0x13`, so the widget script is the `0x40` sub-op run that
    /// follows the spawn instruction (the ending credits' fade-in, flag
    /// wait, fade-out and kill). A bare 19-byte record parks the widget on
    /// its static draw at the record's initial colour.
    pub fn sprite_spawn(&mut self, payload: &[u8]) {
        if let Some(rec) = SpriteRecord::parse(payload) {
            self.sprites
                .push((SpriteWidget::spawn(rec), payload.to_vec()));
        }
    }

    /// Op `0x43` sub-0x11: mask rect tween (`FUN_801F8D4C`). Spawns the
    /// mask fully open on first use; `-1` edges take their full-open
    /// defaults.
    pub fn mask_rect(&mut self, words: [u16; 5]) {
        let screen = self.screen;
        let m = self.mask.get_or_insert_with(|| MaskWidget::spawn(&screen));
        m.set_rect(
            words[0] as i16,
            words[1] as i16,
            words[2] as i16,
            words[3] as i16,
            words[4] as i16,
            &screen,
        );
    }

    /// Op `0x43` sub-0x13: image-panel spawn (`FUN_801F88FC`); `payload`
    /// starts at the sub-op byte, matching the retail reader.
    pub fn panel_spawn(&mut self, payload: &[u8]) {
        self.panel = PanelWidget::spawn(payload);
    }

    /// Op `0x43` sub-0x14: panel move/scale (`FUN_801F8E6C`); no-op
    /// without a spawned panel, like retail's empty actor slot.
    pub fn panel_move(&mut self, words: [i16; 4]) {
        if let Some(p) = &mut self.panel {
            p.move_scale(words[0], words[1], words[2] as i32 & 0xFFFF, words[3]);
        }
    }

    /// Op `0x43` sub-0x15: letterbox config (`FUN_801F8F28`).
    pub fn letterbox_config(&mut self, payload: &[u8]) {
        self.letterbox = Letterbox::from_config(payload);
    }

    /// Advance every live widget one frame and collect the draw list.
    /// `frame_delta` is retail's per-frame byte (`DAT_1F800393`);
    /// `flag_test` is the story-flag probe the sprite scripts wait on
    /// (`FUN_8003CE64`, the `0x80085758` bank).
    pub fn tick(
        &mut self,
        frame_delta: u8,
        mut flag_test: impl FnMut(u16) -> bool,
    ) -> ScreenFxFrame {
        let mut frame = ScreenFxFrame::default();
        let screen = self.screen;
        if let Some(m) = &mut self.mask {
            frame.solid_quads.extend(m.tick(frame_delta, &screen));
        }
        for (w, rec) in &mut self.sprites {
            if let Some(draw) = w.tick(rec, frame_delta, &mut flag_test) {
                frame.sprites.push(draw);
            }
        }
        // Killed sprites never draw again; free their slots.
        self.sprites.retain(|(w, _)| !w.killed);
        if let Some(p) = &mut self.panel {
            let (q0, q1) = p.tick(frame_delta, &screen);
            frame.panels.push(q0);
            if let Some(q1) = q1 {
                frame.panels.push(q1);
            }
        }
        if let Some(lb) = &self.letterbox {
            // Not `solid_quads`: the bands link at OT `+0x4`, the mask's
            // borders at `+0x1c`. See [`ScreenFxFrame::band_quads`].
            frame.band_quads.extend(lb.solid_bands(&screen));
            frame.gradient_quads.extend(lb.gradient_bands());
        }
        frame
    }
}

// ---------------------------------------------------------------------------
// PROT-0900 layout constants (for disc-gated validation + hosting)
// ---------------------------------------------------------------------------

/// Extraction PROT entry of the screen-effect / top-view render overlay.
pub const SCREEN_FX_OVERLAY_PROT: usize = 900;

/// Slot-B link base the overlay loads at.
pub const SCREEN_FX_LINK_BASE: u32 = 0x801F_69D8;

/// Per-frame handler entry points, in handler-table order
/// (sprite, mask, panel, letterbox).
pub const SCREEN_FX_HANDLER_VAS: [u32; 4] = [0x801F_7A9C, 0x801F_811C, 0x801F_849C, 0x801F_8A34];

/// The four 0x18-byte handler-binding descriptors (`FUN_80020DE0` input),
/// in the same order as [`SCREEN_FX_HANDLER_VAS`].
pub const SCREEN_FX_DESCRIPTOR_VAS: [u32; 4] = [0x801F_8FE4, 0x801F_8FFC, 0x801F_9014, 0x801F_902C];

/// The 5-entry sprite-script sub-op dispatch table at the overlay head
/// (`jr *(0x801F69D8 + sub*4)`): kill, wait-set, wait-clear, tween-pos+colour,
/// tween-colour.
pub const SPRITE_SUBOP_ENTRY_VAS: [u32; 5] = [
    0x801F_7B14,
    0x801F_7B28,
    0x801F_7B54,
    0x801F_7B8C,
    0x801F_7D90,
];
