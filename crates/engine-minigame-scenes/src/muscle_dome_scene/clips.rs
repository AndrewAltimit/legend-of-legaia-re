//! Body clips: the pose tags, per-body clip sets, the playing act and the clip pickers.

use legaia_actor_formats::battle_char_assembly as bca;
use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_tmd::mesh::VramMesh;

/// Player battle-form clip slots the dome plays.
pub(super) const P_IDLE: u32 = 0;
pub(super) const P_HIT: u32 = 2;
pub(super) const P_KO: u32 = 4;
/// The walk / approach cycle - action tag `1` in both the player and the
/// monster files (`docs/formats/monster-animation.md`).
pub(super) const WALK_TAG: u8 = 1;
pub(super) const P_SWINGS: [u32; 4] = [0xC, 0xD, 0xE, 0xF];

/// One decoded clip: per (frame, part) `[tx, ty, tz, rx, ry, rz]`.
#[derive(Debug, Clone)]
pub struct DomeClip {
    pub(super) frames: Vec<Vec<[i32; 6]>>,
    /// Keyframes advanced per 16 ticks (the retail `rate` byte, doubled).
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
            rate: u32::from(anim.rate.max(1)) * 2,
        })
    }

    /// Keyframe count.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// One body's clip set.
#[derive(Debug, Clone)]
pub(super) struct BodyClips {
    pub(super) idle: DomeClip,
    /// The walk / approach cycle (record tag `1`), idle when absent.
    pub(super) walk: DomeClip,
    pub(super) hit: DomeClip,
    pub(super) ko: DomeClip,
    /// The player's per-command swings (`0xC..=0xF`, index `cmd - 0xC`), or
    /// the monster's one attack clip in slot 0.
    pub(super) swings: [Option<DomeClip>; 4],
}

impl BodyClips {
    pub(super) fn swing(&self, cmd: u8) -> &DomeClip {
        let i = usize::from(cmd.wrapping_sub(0xC));
        self.swings
            .get(i)
            .and_then(Option::as_ref)
            .or(self.swings[0].as_ref())
            .unwrap_or(&self.idle)
    }
}

/// One posable body: its rest mesh, per-vertex object ids and clips.
#[derive(Debug, Clone)]
pub(super) struct DomeBody {
    pub(super) mesh: VramMesh,
    pub(super) oids: Vec<u32>,
    pub(super) clips: BodyClips,
}

/// Which clip a body plays and since when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClipKind {
    Idle,
    Walk,
    Hit,
    Ko,
    Swing(u8),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Act {
    pub(super) kind: ClipKind,
    pub(super) start: u32,
    pub(super) looped: bool,
    pub(super) hold: bool,
}

impl Act {
    pub(super) fn idle(start: u32) -> Self {
        Self {
            kind: ClipKind::Idle,
            start,
            looped: true,
            hold: false,
        }
    }
}

/// One battle-form clip by runtime action slot, expanded per assembled
/// object: slot `0` the record-0 idle loop, `0xC..=0xF` the per-command
/// swing records, any other slot the record-0 action table by tag (the
/// party hit-reaction family `[2, 3, 4, 5, 0xB]`).
pub fn fighter_clip(
    raw: &[u8],
    asm: &bca::AssembledCharacter,
    slot: u32,
) -> Option<MonsterAnimation> {
    let anim = if (0xC..=0xF).contains(&slot) {
        let pack = legaia_actor_formats::battle_data_pack::parse(raw).ok()?;
        bca::swing_battle_animations(raw, &pack, &[0u8; 5])
            .ok()?
            .into_iter()
            .find(|s| u32::from(s.slot) == slot)?
            .anim
    } else if slot == 0 {
        bca::idle_battle_animation(raw).ok()??
    } else {
        bca::battle_animations(raw)
            .ok()?
            .into_iter()
            .find(|a| u32::from(a.action_id) == slot)?
    };
    Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones))
}

/// The monster clip indices the dome plays: idle (tag 0), the attack (tag
/// `0x21`, else `0x20`, else the first `>= 0x20`, else entry 1), the hit
/// (tag 2, else 3) and the knockdown (tag 4, else the hit).
pub fn pick_monster_clips(anims: &[MonsterAnimation]) -> [usize; 4] {
    let by_tag = |t: u8| anims.iter().position(|a| a.action_id == t);
    let idle = by_tag(0).unwrap_or(0);
    let attack = by_tag(0x21)
        .or_else(|| by_tag(0x20))
        .or_else(|| anims.iter().position(|a| a.action_id >= 0x20))
        .unwrap_or(if anims.len() > 1 { 1 } else { idle });
    let hit = by_tag(2).or_else(|| by_tag(3));
    let ko = by_tag(4).or(hit).unwrap_or(idle);
    [idle, attack, hit.unwrap_or(idle), ko]
}

pub(super) fn clip_for(c: &BodyClips, kind: ClipKind) -> &DomeClip {
    match kind {
        ClipKind::Idle => &c.idle,
        ClipKind::Walk => &c.walk,
        ClipKind::Hit => &c.hit,
        ClipKind::Ko => &c.ko,
        ClipKind::Swing(cmd) => c.swing(cmd),
    }
}
