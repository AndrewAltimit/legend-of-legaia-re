//! The surface's idle clips: what a body poses from when its session carries
//! no play-out choreography (`muscle_dome_session::choreo`).

use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_tmd::mesh::VramMesh;

/// One decoded clip: per (frame, part) `[tx, ty, tz, rx, ry, rz]`.
#[derive(Debug, Clone)]
pub struct DomeClip {
    pub(super) frames: Vec<Vec<[i32; 6]>>,
    /// The entry's rate byte `+0x78`.
    pub(super) rate: u32,
}

impl DomeClip {
    pub(super) fn from_anim(anim: &MonsterAnimation) -> Option<Self> {
        if anim.frame_count == 0 || anim.frames.is_empty() {
            return None;
        }
        let frames = anim
            .frames
            .iter()
            .map(|f| {
                f.iter()
                    .map(|t| {
                        [
                            i32::from(t.tx),
                            i32::from(t.ty),
                            i32::from(t.tz),
                            i32::from(t.rx),
                            i32::from(t.ry),
                            i32::from(t.rz),
                        ]
                    })
                    .collect()
            })
            .collect();
        Some(Self {
            frames,
            rate: u32::from(anim.rate.max(1)),
        })
    }

    /// Keyframe count.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The keyframe `ticks` into a loop at the battle rate law's normal
    /// advance (`FUN_80047430`: `rate / 4` keyframes a vsync).
    pub(super) fn frame_at(&self, ticks: u32) -> usize {
        (ticks * self.rate / 4) as usize % self.frame_count().max(1)
    }
}

/// One posable body: its rest mesh, per-vertex object ids and idle loop.
#[derive(Debug, Clone)]
pub(super) struct DomeBody {
    pub(super) mesh: VramMesh,
    pub(super) oids: Vec<u32>,
    pub(super) idle: DomeClip,
}
