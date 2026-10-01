//! The floor's **bodies**: every 3D actor the dance overlay's spawner
//! (`FUN_801d0190`) puts on the floor, with the clip each one shows this
//! frame - the rules-side half of the dancer render. The mesh + pose half is
//! [`crate::dance_cast_scene`], which every host draws through.
//!
//! Two kinds of actor come out of the spawner
//! (`see ghidra/scripts/funcs/overlay_dance_801d0190.txt`):
//!
//! * the **dancers** - one per record of the mode's spawn table, `+0x48` the
//!   kind descriptor, `+0x5C` / `+0x6A` its idle clip + rate. Kind 0 (the
//!   human, Noa) is the one record whose model id skips the scene TMD base
//!   (`0x801D0280`), and it alone gets `+0x72 = 0x1400` (`0x801D02B8`) - the
//!   actor render scale, so her field mesh draws at `1.25x` beside the hall's
//!   dancer NPCs;
//! * in the how-to mode only (`DAT_801d514c == 2`, `0x801D0338..0x801D0390`),
//!   the **Disco King** from the second template at `0x801D4344`, whose tick
//!   word is the tutorial script `FUN_801d0750`: scene model `base + 0x3F`,
//!   clip `0x3B` at rate `8`, standing at `(0x1800, 0, 0x3200)`. He is not a
//!   dancer - he has no slot in the per-dancer arrays, so nothing judges or
//!   scores him - and his script never writes `+0x5C`, so the one clip plays
//!   for the whole lesson.

use super::*;

/// Scene-pool model the how-to spawner seats the Disco King on
/// (`addiu v0,v0,0x3f` at `0x801D0360`, added to the scene TMD base).
pub const DEMO_MODEL: u16 = 0x3F;
/// The Disco King's clip id (`li v0,0x3b` at `0x801D0370` into `+0x5C`).
pub const DEMO_CLIP: u16 = 0x3B;
/// His clip rate (`li v0,0x8` at `0x801D0378` into `+0x6A`).
pub const DEMO_CLIP_RATE: u16 = 8;
/// His floor position (`0x801D0380..0x801D0390` into `+0x14` / `+0x16` /
/// `+0x18`).
pub const DEMO_POS: [i16; 3] = [0x1800, 0, 0x3200];
/// The human dancer's actor render scale (`li v0,0x1400` / `sh v0,0x72(a1)`
/// at `0x801D02B8`; `0x1000` = 1.0).
pub const HUMAN_RENDER_SCALE: u16 = 0x1400;
/// Every other body's render scale.
pub const UNIT_RENDER_SCALE: u16 = 0x1000;

/// One clip reference on a body's display track: the placement-space anim id
/// (`record = id - 1`) and the cursor step in 1/16-frame units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DanceBodyClip {
    /// Anim id, `0` = nothing bound.
    pub id: u16,
    /// Cursor step per tick (`+0x6A`).
    pub rate: u16,
}

impl DanceBodyClip {
    /// The track entry for a kind-descriptor clip.
    pub fn of(clip: &legaia_asset::dance_cast::DanceClip) -> Self {
        Self {
            id: clip.anim_id & 0x1FF,
            rate: clip.rate,
        }
    }

    /// ANM bundle record index (`None` when nothing is bound).
    pub fn record(self) -> Option<usize> {
        (self.id > 0).then(|| usize::from(self.id) - 1)
    }
}

/// Which mesh pool a body's model id indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DanceBodyModel {
    /// The resident global pool (kind 0's id, written without the scene TMD
    /// base): slot 1 = Noa's field mesh, PROT 0874 §0 slot 1.
    Resident(u16),
    /// The dance-hall scene's TMD pool (MAN model-byte space).
    Scene(u16),
}

/// One body on the floor this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceBodyFrame {
    /// Which mesh to draw.
    pub model: DanceBodyModel,
    /// The dancer kind (`None` for the Disco King, who is not a dancer).
    pub kind: Option<usize>,
    /// World position (`+0x14` / `+0x16` / `+0x18`).
    pub pos: [i16; 3],
    /// Yaw (`+0x26`) - non-zero only through the groovy-move spin.
    pub yaw: i16,
    /// Render scale (`+0x72`, `0x1000` = 1.0).
    pub scale: u16,
    /// The standing loop (idle before the song, the dance-groove loop in it).
    pub loop_clip: DanceBodyClip,
    /// The judge-returned move last bound over the loop, if any. It plays
    /// once from its first frame; once it has run its length the loop
    /// resumes from its own first frame, which is `FUN_801d1358`'s rebind on
    /// the clip driver's end flag (`+0x62 & 0x100`, `0x801D14C8`).
    pub move_clip: Option<DanceBodyClip>,
    /// Ticks since that track last restarted.
    pub ticks: u32,
}

/// The how-to mode's Disco King actor.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DemoDancer {
    pub(super) ticks: u32,
}

impl DanceGame {
    /// Every body on the floor this frame: the dancers in slot order, then
    /// the how-to mode's Disco King.
    ///
    /// Empty for a chart-only run (no kind descriptors, so no model ids).
    pub fn body_frames(&self) -> Vec<DanceBodyFrame> {
        let mut out = Vec::with_capacity(self.dancers.len() + 1);
        for d in &self.dancers {
            let Some(k) = self.kinds.get(d.kind) else {
                continue;
            };
            let human = d.kind == 0;
            out.push(DanceBodyFrame {
                model: if human {
                    DanceBodyModel::Resident(k.model)
                } else {
                    DanceBodyModel::Scene(k.model)
                },
                kind: Some(d.kind),
                pos: d.home,
                yaw: (d.spin_acc % SPIN_TURN_UNITS) as i16,
                scale: if human {
                    HUMAN_RENDER_SCALE
                } else {
                    UNIT_RENDER_SCALE
                },
                loop_clip: d.show_loop,
                move_clip: d.show_move,
                ticks: d.show_ticks,
            });
        }
        if let Some(demo) = self.demo.as_ref() {
            out.push(DanceBodyFrame {
                model: DanceBodyModel::Scene(DEMO_MODEL),
                kind: None,
                pos: DEMO_POS,
                yaw: 0,
                scale: UNIT_RENDER_SCALE,
                loop_clip: DanceBodyClip {
                    id: DEMO_CLIP,
                    rate: DEMO_CLIP_RATE,
                },
                move_clip: None,
                ticks: demo.ticks,
            });
        }
        out
    }

    /// Advance every body's display track `frame_delta` ticks. Runs every
    /// frame of the run - the count-in included, where the dancers stand in
    /// their idle clip - because the clip driver ticks each actor
    /// independently of the dance states.
    pub fn advance_body_clips(&mut self, frame_delta: u32) {
        for d in &mut self.dancers {
            d.show_ticks = d.show_ticks.saturating_add(frame_delta);
        }
        if let Some(demo) = self.demo.as_mut() {
            demo.ticks = demo.ticks.saturating_add(frame_delta);
        }
    }
}
