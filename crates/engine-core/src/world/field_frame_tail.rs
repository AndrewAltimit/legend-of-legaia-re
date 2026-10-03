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
use crate::field_env::{ANIM_CLAMP, PropAnim};

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

/// What [`World::step_world_frame_tail`] hands back: the half of each duty a
/// host that draws or sounds the world still owns. A headless driver drops
/// all of it - the world side already ran.
#[derive(Default)]
pub struct WorldFrameTail {
    /// The sound cue of the move-FX this tick spawned, for the host's SFX
    /// scheduler (retail's dispatch decode `classify_cue`).
    pub move_fx_cue: Option<u8>,
    /// The battle effect-script spawns routed this tick, for a host's log.
    pub routed_effect_spawns: Vec<super::RoutedEffectSpawn>,
    /// The NPC clip re-targets this tick's ANIMATE cues resolved.
    pub npc_retargets: Vec<NpcClipRetarget>,
}

impl World {
    /// The world-side half of the per-sim-tick tail both play hosts run after
    /// the scene tick, in the native window's order: seat the pending move-FX (`spawn_move_fx`), advance the three effect
    /// scene-graphs ([`Self::tick_effect_scene_graphs`]), route the battle
    /// effect-script spawns ([`Self::route_battle_effect_spawns`]) and drain
    /// the ANIMATE cues ([`Self::drain_field_anim_cues`]).
    ///
    /// Each of those has a world side a script or the battle action state
    /// machine waits on - a routed effect is only retired by the scene-graph
    /// tick, a latched `+0x5E` clip is what the NPC motion reads - so a driver
    /// that ticks the scene without this tail measures a different game from
    /// the one the play hosts run. `engine-shell`'s `BootSession::tick` calls
    /// it for every headless driver; the play hosts interleave their render
    /// work between the same steps and call the steps themselves.
    ///
    /// The summon-spawn request (`World::take_pending_summon_spawn`) is not
    /// taken here: its only consumer is a host seating the namesake creature's
    /// mesh, a render duty with no world side, and a headless caller that
    /// observes the request reads it off the world after the tick.
    pub fn step_world_frame_tail(
        &mut self,
        scene_bundle: Option<&PlayerAnmBundle>,
        locomotion_bundle: Option<&PlayerAnmBundle>,
        npc_bundle: impl Fn(u8) -> Option<bool>,
    ) -> WorldFrameTail {
        let mut move_fx_cue = None;
        if let Some((move_id, origin)) = self.take_pending_move_fx_spawn()
            && self.spawn_move_fx(move_id, origin)
        {
            move_fx_cue = self.take_pending_move_fx_cue();
        }
        self.tick_effect_scene_graphs();
        let routed_effect_spawns = self.route_battle_effect_spawns();
        let npc_retargets = self.drain_field_anim_cues(scene_bundle, locomotion_bundle, npc_bundle);
        WorldFrameTail {
            move_fx_cue,
            routed_effect_spawns,
            npc_retargets,
        }
    }

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

    /// Whether a host advances its field-NPC clip players this tick.
    ///
    /// The clip players are host-side (each host keys its own pose cache or
    /// JSON on them), but *when* they run is a world decision: only while the
    /// field owns the frame. Field actors are not ticked under a battle, a
    /// minigame, the world map or a movie - the field overlay that steps them
    /// is not the one running - so an NPC walks back into the field on the
    /// clip frame it left. The native window already held its players still
    /// off the field while the page ran them in every mode, so an NPC came
    /// back from a fight on a different frame per host.
    pub fn field_npc_clips_advance(&self) -> bool {
        self.mode == SceneMode::Field
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
    /// The **NPC** half (op `0x4B`, a cross-context ExecMove, a `4C 51` run's
    /// move id) is resolved and handed back: `npc_bundle(slot)` answers
    /// whether the host animates that slot and, if so, which bundle it spawned
    /// posing from - the party locomotion bundle (`Some(true)`) or the scene's
    /// own (`Some(false)`). A cue's anim id names record `id - 1` of the bank
    /// the actor's **live** party-bank bit selects
    /// ([`Self::npc_party_bank`]), as `FUN_800204F8` binds it - the host's
    /// spawn answer stands in only for a slot no channel carries. A party
    /// member's story beats drop that bit to play a scene-bank gesture and
    /// raise it again after, so binding by spawn class played the gesture's
    /// id out of the locomotion bundle: the wrong clip, no clip, or a
    /// two-bone save-crystal record on a ten-bone body.
    ///
    /// A clip whose bone count differs from the one the slot was bound with
    /// is refused: the hosts cut the actor's mesh to that count, and a pose
    /// for another skeleton tears it apart.
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
                if let Some(anim) = self.locomotion.player_anim.as_mut()
                    && let Some(clip) = FieldClipPlayer::from_record(bundle, id as usize - 1)
                        .filter(|c| c.bone_count() == anim.idle.bone_count())
                {
                    anim.push_scripted(clip);
                }
            }
        }
        let drained = std::mem::take(&mut self.npcs.anim_cues);
        let mut cues: Vec<(u8, u8)> = Vec::with_capacity(drained.len());
        for (slot, (count, base_id, frames)) in drained {
            // Retail's `+0x5E` latch: a single-move cue is a `+0x5C` write,
            // and the consumer latches it as the playing move; a sequence
            // cue is not one move, so nothing is latched for the slot.
            if count == 1 && frames.is_empty() {
                self.npcs.clip_current.insert(slot, base_id);
            } else {
                self.npcs.clip_current.remove(&slot);
            }
            cues.push((slot, base_id));
        }
        // The cue map is a hash map; apply in slot order so both hosts see
        // the same sequence.
        cues.sort_unstable();
        cues.into_iter()
            .filter_map(|(slot, base_id)| {
                let spawn_party = npc_bundle(slot)?;
                let bundle = if self.npc_clip_party_bank(slot, spawn_party) {
                    locomotion_bundle
                } else {
                    scene_bundle
                }?;
                let record = (base_id as usize).checked_sub(1)?;
                FieldClipPlayer::from_record(bundle, record)
                    .filter(|p| {
                        self.npcs
                            .clip_bones
                            .get(&slot)
                            .is_none_or(|&b| p.bone_count() == b)
                    })
                    .map(|player| (slot, base_id, NpcClipRetarget { slot, player }))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(slot, base_id, r)| {
                self.bind_npc_clip_cursor(slot, base_id, &r.player);
                r
            })
            .collect()
    }

    /// Bind NPC `slot`'s world-owned clip cursor to `player`'s clip (anim id
    /// `anim_id`) - the binder half of `FUN_800204F8`: the cursor restarts at
    /// frame 0 and the actor's `+0x62` word is left alone, and re-binding the
    /// id already playing is a no-op (`+0x5C == +0x5E`). Hosts call this when
    /// they build a slot's clip player at scene entry;
    /// [`Self::drain_field_anim_cues`] calls it for every re-target.
    ///
    /// PORT: FUN_800204F8 (binder half)
    pub fn bind_npc_clip_cursor(&mut self, slot: u8, anim_id: u8, player: &FieldClipPlayer) {
        self.npcs
            .clip_bones
            .entry(slot)
            .or_insert_with(|| player.bone_count());
        let frames = u16::try_from(player.frame_count())
            .unwrap_or(u16::MAX)
            .max(1);
        let rate = i16::try_from(player.step()).unwrap_or(i16::MAX).max(1);
        if let Some(c) = self.npcs.clip_cursors.get_mut(&slot)
            && c.anim_id == anim_id
            && c.frames == frames
        {
            return;
        }
        // The step is the player's own (`clip_step` of the record header);
        // the ungated arm of the tick's step select passes it through as-is.
        let mut cursor = PropAnim::cross_context(anim_id.max(1), frames);
        cursor.rate = rate;
        if let Some(flags) = self.npc_channel_local_flags(slot) {
            cursor.flags = flags;
        }
        self.npcs.clip_cursors.insert(slot, cursor);
    }

    /// Whether NPC `slot`'s actor carries the party-bank bit `0x01000000` -
    /// the bit `FUN_800204F8` routes its clip ids through the party
    /// locomotion bank on - read off its placement channel's live flag word.
    /// `None` when no placement channel carries the slot.
    ///
    /// REF: FUN_800204F8 (`0x8002053C`), FUN_8003A1E4 (the spawn seed)
    pub fn npc_party_bank(&self, slot: u8) -> Option<bool> {
        self.channel_view()
            .iter()
            .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
            .map(|c| c.ctx.flags & legaia_engine_vm::field_player_clip::PARTY_BANK_FLAG != 0)
    }

    /// Whether NPC `slot`'s clip ids name records of the party locomotion
    /// bank (`true`) or of the scene's own bank (`false`): the live
    /// party-bank bit ([`Self::npc_party_bank`]), and the spawn class
    /// (`spawn_party`, a global-pool party model) only for a slot no
    /// placement channel carries.
    ///
    /// Both the clip a host binds when it uploads the scene's NPCs and every
    /// later re-target resolve through this. The spawn binding used to pick
    /// by spawn class alone, so a party model whose spawn prologue had
    /// already dropped the bit for a scene-bank gesture (`B2 <id> 18`, then
    /// `A2 <id> <move>`) bound the id out of the locomotion bank - a different
    /// record, or none when the locomotion bank is shorter than the id.
    ///
    /// REF: FUN_800204F8 (`0x8002053C`)
    pub fn npc_clip_party_bank(&self, slot: u8, spawn_party: bool) -> bool {
        self.npc_party_bank(slot).unwrap_or(spawn_party)
    }

    /// The `+0x62` anim-control word of NPC `slot`'s spawned context, if a
    /// placement channel carries it.
    fn npc_channel_local_flags(&self, slot: u8) -> Option<u16> {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
            .map(|c| c.ctx.local_flags)
    }

    /// One field frame of the per-actor anim tick over every bound NPC clip
    /// cursor: the slot's channel `+0x62` goes in, the tick consumes a restart
    /// request, steps unless held, wraps or clamps, latches the end bit, and
    /// the word goes back to the channel - where the actor's own script (or a
    /// talk running on it) reads the latch. An actor whose word is the
    /// template's looping state plays exactly as a free-running clip.
    ///
    /// PORT: FUN_800204F8 (advance half, NPC actors)
    pub fn tick_npc_clips(&mut self) {
        if !self.field_npc_clips_advance() {
            return;
        }
        let slots: Vec<u8> = self.npcs.clip_cursors.keys().copied().collect();
        for slot in slots {
            let flags = self.npc_channel_local_flags(slot);
            let Some(c) = self.npcs.clip_cursors.get_mut(&slot) else {
                continue;
            };
            if let Some(f) = flags {
                c.flags = f;
            }
            c.tick();
            let out = c.flags;
            if flags.is_some()
                && let Some(ch) = self
                    .field_vm
                    .channels
                    .iter_mut()
                    .find(|ch| !ch.object_bind && ch.placement_index == usize::from(slot))
            {
                ch.ctx.local_flags = out;
            }
        }
    }

    /// The running clip of the field NPC a cross-context op's `target` byte
    /// names (the channel whose script id is `target`, retail's
    /// `FUN_8003C83C` resolution): `Some(Some(cursor))` when the world owns a
    /// cursor for that placement, `Some(None)` for an NPC without one, `None`
    /// when `target` is no live NPC channel.
    pub(crate) fn npc_live_clip_for_target(
        &self,
        target: u8,
    ) -> Option<Option<crate::field_env::PropAnim>> {
        let chans = if self.field_vm.channels.is_empty() {
            &self.field_vm.stepping_view
        } else {
            &self.field_vm.channels
        };
        let ch = chans
            .iter()
            .find(|c| !c.object_bind && c.ctx.script_id == u16::from(target))?;
        let slot = u8::try_from(ch.placement_index).ok()?;
        Some(self.npcs.clip_cursors.get(&slot).copied())
    }

    /// Pose `player` from NPC `slot`'s world-owned cursor. `false` when the
    /// world has no cursor for the slot - the host then free-runs the player
    /// itself.
    pub fn sync_npc_clip(&self, slot: u8, player: &mut FieldClipPlayer) -> bool {
        let Some(c) = self.npcs.clip_cursors.get(&slot) else {
            return false;
        };
        player.set_cursor(c.cursor.max(0) as u32, c.flags & ANIM_CLAMP != 0);
        true
    }

    /// Whether NPC `slot` has a world-owned clip cursor - i.e. something
    /// will latch its `+0x62` end bit.
    pub fn npc_clip_cursor_bound(&self, slot: u8) -> bool {
        self.npcs.clip_cursors.contains_key(&slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm as vm;

    #[test]
    fn npc_clips_run_on_the_field_only() {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        assert!(w.field_npc_clips_advance());
        for mode in [
            SceneMode::Battle,
            SceneMode::WorldMap,
            SceneMode::Cutscene,
            SceneMode::Dance,
            SceneMode::Title,
        ] {
            w.mode = mode;
            assert!(!w.field_npc_clips_advance(), "{mode:?}");
        }
    }

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
    fn the_world_tail_consumes_every_queue_a_headless_tick_used_to_drop() {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.locomotion.player_move_cues.push(7);
        w.npcs.anim_cues.insert(3, (1, 5, Vec::new()));
        let tail = w.step_world_frame_tail(None, None, |_| None);
        assert!(w.locomotion.player_move_cues.is_empty());
        assert!(w.npcs.anim_cues.is_empty());
        // The single-move cue's `+0x5E` latch - the world side a host-less
        // drain used to leave unset.
        assert_eq!(w.npcs.clip_current.get(&3), Some(&5));
        assert!(tail.npc_retargets.is_empty());
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

    fn party_channel(slot: usize, flags: u32) -> crate::field_channels::FieldChannel {
        crate::field_channels::FieldChannel {
            placement_index: slot,
            ctx: legaia_engine_vm::field::FieldCtx {
                flags,
                ..Default::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        }
    }

    /// A party member's gesture binds from the bank its **live** party-bank
    /// bit names, not the one its spawn model implied: the story scripts drop
    /// the bit (`B2 <id> 18`) to play a scene-bank gesture.
    #[test]
    fn an_npc_cue_binds_from_the_bank_the_live_party_bit_names() {
        // Locomotion record 4 is a 10-bone clip, scene record 4 a 10-bone
        // clip of another length - the two banks are told apart by frames.
        let loco = crate::field_anim::synth_anm_bundle(&[(10, 2); 5]);
        let mut scene_recs = vec![(10, 2); 4];
        scene_recs.push((10, 7));
        let scene = crate::field_anim::synth_anm_bundle(&scene_recs);
        let mut w = World::new();
        w.mode = SceneMode::Field;
        let party = vm::field_player_clip::PARTY_BANK_FLAG;
        w.field_vm.channels.push(party_channel(3, party));

        w.npcs.anim_cues.insert(3, (1, 5, Vec::new()));
        let out = w.drain_field_anim_cues(Some(&scene), Some(&loco), |_| Some(true));
        assert_eq!(
            out[0].player.frame_count(),
            2,
            "bit up: the locomotion bank"
        );

        w.field_vm.channels[0].ctx.flags &= !party;
        w.npcs.anim_cues.insert(3, (1, 5, Vec::new()));
        let out = w.drain_field_anim_cues(Some(&scene), Some(&loco), |_| Some(true));
        assert_eq!(out[0].player.frame_count(), 7, "bit down: the scene bank");
    }

    /// A clip for another skeleton never replaces a bound one: the hosts cut
    /// the actor's mesh to the bound clip's bone count.
    #[test]
    fn an_npc_cue_for_another_skeleton_is_refused() {
        let loco = crate::field_anim::synth_anm_bundle(&[(10, 2), (2, 3)]);
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.field_vm
            .channels
            .push(party_channel(3, vm::field_player_clip::PARTY_BANK_FLAG));
        let ten = FieldClipPlayer::from_record(&loco, 0).unwrap();
        w.bind_npc_clip_cursor(3, 1, &ten);
        w.npcs.anim_cues.insert(3, (1, 2, Vec::new()));
        let out = w.drain_field_anim_cues(None, Some(&loco), |_| Some(true));
        assert!(out.is_empty(), "the two-bone record is not a hero pose");
        w.npcs.anim_cues.insert(3, (1, 1, Vec::new()));
        let out = w.drain_field_anim_cues(None, Some(&loco), |_| Some(true));
        assert_eq!(out.len(), 1);
    }
}
