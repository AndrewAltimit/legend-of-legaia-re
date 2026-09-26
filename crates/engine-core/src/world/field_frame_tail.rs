//! The **field frame tail**: the per-sim-tick work that follows the scene tick
//! on every play host, held here so both hosts run one body instead of two
//! copies that drift.
//!
//! Each play host (the native `play-window` and the browser play page) runs
//! the scene tick and then a tail of world-side steps. Those tails were
//! written twice and had drifted in three ways that each changed what the
//! player sees:
//!
//! - The native window's opening-narration arm returned straight after the
//!   scene tick, so during the crawl and the title card it skipped the effect
//!   scene-graphs, the scripted CLUT / VRAM effects and the ANIMATE cue drain
//!   the page ran every tick.
//! - The native window drained ANIMATE cues (op `0x4B` for NPCs, the
//!   `A2 F8` ExecMove pokes for the player) only while the world was in
//!   `SceneMode::Field`, inside its draw pass; the page drained them every
//!   tick. A cue raised off the field sat in the queue and fired on a later
//!   field frame.
//! - The native CLUT-effect step skipped every call when four effect lists
//!   were empty, and its emptiness test left out the `4C DB` blend fades, so a
//!   scene whose only live effect was a blend fade never stepped it; the
//!   skipped calls also left each list's tick backlog undrained, so a fade
//!   spawned later started that many ticks in.
//!
//! The kernels below are what a host calls; what stays host-side is only what
//! the host owns (GPU uploads, the NPC clip players it draws from).

use legaia_asset::player_anm::PlayerAnmBundle;

use super::{SceneMode, World};
use crate::field_anim::FieldClipPlayer;

/// The anim-speed step every host ticks the effect scene-graphs at - the
/// per-part wait-timer drain `0x0400` both hosts already passed by hand.
pub const EFFECT_SCENE_GRAPH_STEP: u16 = 0x0400;

/// One NPC whose clip an op-`0x4B` ANIMATE cue re-targeted this tick. The host
/// owns the clip players (it draws from them), so the kernel hands back the
/// resolved player and the host swaps it in.
pub struct NpcClipRetarget {
    /// The placement slot the cue names.
    pub slot: u8,
    /// The clip the cue's anim id resolves to, rewound to frame 0.
    pub player: FieldClipPlayer,
}

impl World {
    /// Advance the three move-VM effect scene-graphs one tick: the active
    /// Seru-magic summon, the battle move-FX, and the field op-`0x34` sub-3
    /// effects. Each self-gates to a no-op when nothing is live.
    pub fn tick_effect_scene_graphs(&mut self) {
        self.tick_summon(EFFECT_SCENE_GRAPH_STEP);
        self.tick_move_fx(EFFECT_SCENE_GRAPH_STEP);
        self.tick_field_fx(EFFECT_SCENE_GRAPH_STEP);
    }

    /// Drain every scripted VRAM effect into the scene VRAM: the op-`0x43`
    /// `MoveImage` stamps, the op-`0x43` sub-`0x12` rect copies, the ambient
    /// move-VM effect tree, and the CLUT-cell one-shots and blend fades.
    /// Returns `true` when any VRAM word changed (the host re-uploads).
    ///
    /// Every step runs every tick, empty or not: each one drains its own tick
    /// backlog and self-gates, so skipping them while their lists are empty
    /// is not free - it banks ticks a later effect then consumes at once.
    ///
    /// `back_buffer` is forwarded to [`World::apply_vram_rect_copies`]; both
    /// hosts present one framebuffer page and pass `false`.
    ///
    /// Off in battle, where the host's VRAM image is the battle one and a
    /// field write would clobber it.
    pub fn step_field_vram_effects(
        &mut self,
        vram: &mut legaia_tim::Vram,
        back_buffer: bool,
    ) -> bool {
        if self.mode == SceneMode::Battle {
            return false;
        }
        let moved = self.apply_script_vram_moves(vram);
        let copied = self.apply_vram_rect_copies(vram, back_buffer);
        let ambient = self.step_ambient_fx(vram);
        let clut = self.step_clut_fx(vram);
        moved | copied | ambient | clut
    }

    /// Drain this tick's ANIMATE cues, in every scene mode.
    ///
    /// The **player** half is applied here: a settle pick that binds from the
    /// scene bank resolves against `scene_bundle`, and each `A2 F8 <move_id>`
    /// ExecMove poke above the two locomotion moves queues scene record
    /// `move_id - 1` as a scripted one-shot over the idle / walk pair. The
    /// queue is emptied whether or not a bundle is present, as retail's poke
    /// is consumed whether or not it binds.
    ///
    /// The **NPC** half (op `0x4B`) is resolved and handed back:
    /// `npc_bundle(slot)` answers whether the host animates that slot and, if
    /// so, whether it poses from the party locomotion bundle (`Some(true)`) or
    /// the scene's own (`Some(false)`); a cue's anim id names record `id - 1`
    /// of that bundle, the same space the placement's own anim byte uses.
    pub fn drain_field_anim_cues(
        &mut self,
        scene_bundle: Option<&PlayerAnmBundle>,
        locomotion_bundle: Option<&PlayerAnmBundle>,
        npc_bundle: impl Fn(u8) -> Option<bool>,
    ) -> Vec<NpcClipRetarget> {
        if let (Some(bundle), Some(anim)) = (scene_bundle, self.locomotion.player_anim.as_mut()) {
            anim.resolve_scene_clip(bundle);
        }
        let moves = std::mem::take(&mut self.locomotion.player_move_cues);
        if let Some(bundle) = scene_bundle {
            for id in moves {
                // Moves 1 and 2 are the locomotion walk moves the movement
                // controller already animates.
                if id <= 2 {
                    continue;
                }
                if let Some(clip) = FieldClipPlayer::from_record(bundle, id as usize - 1)
                    && let Some(anim) = self.locomotion.player_anim.as_mut()
                {
                    anim.push_scripted(clip);
                }
            }
        }
        let mut cues: Vec<(u8, u8)> = self
            .npcs
            .anim_cues
            .drain()
            .map(|(slot, (_count, base_id, _frames))| (slot, base_id))
            .collect();
        // The cue map is a hash map; apply in slot order so both hosts see
        // the same sequence.
        cues.sort_unstable();
        cues.into_iter()
            .filter_map(|(slot, base_id)| {
                let special = npc_bundle(slot)?;
                let bundle = if special {
                    locomotion_bundle
                } else {
                    scene_bundle
                }?;
                let record = (base_id as usize).checked_sub(1)?;
                FieldClipPlayer::from_record(bundle, record)
                    .map(|player| NpcClipRetarget { slot, player })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_drain_empties_both_queues_off_the_field_too() {
        let mut w = World::new();
        w.mode = SceneMode::WorldMap;
        w.locomotion.player_move_cues.push(7);
        w.npcs.anim_cues.insert(3, (1, 5, Vec::new()));
        // No bundles: nothing resolves, but both queues drain - a cue left
        // queued off the field would fire on some later field frame.
        let out = w.drain_field_anim_cues(None, None, |_| Some(false));
        assert!(out.is_empty());
        assert!(w.locomotion.player_move_cues.is_empty());
        assert!(w.npcs.anim_cues.is_empty());
    }

    #[test]
    fn vram_effects_drain_their_backlogs_even_when_idle() {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.ambient.clut_pending_game_ticks = 9;
        w.ambient.pending_game_ticks = 9;
        let mut vram = legaia_tim::Vram::new();
        assert!(!w.step_field_vram_effects(&mut vram, false));
        assert_eq!(w.ambient.clut_pending_game_ticks, 0);
        assert_eq!(w.ambient.pending_game_ticks, 0);
    }

    #[test]
    fn vram_effects_are_off_in_battle() {
        let mut w = World::new();
        w.mode = SceneMode::Battle;
        w.ambient.clut_pending_game_ticks = 9;
        let mut vram = legaia_tim::Vram::new();
        assert!(!w.step_field_vram_effects(&mut vram, false));
        assert_eq!(w.ambient.clut_pending_game_ticks, 9);
    }
}
