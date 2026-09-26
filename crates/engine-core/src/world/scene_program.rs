//! The per-frame driver of the field overlay's **scripted-scene actor**
//! (`FUN_801D4A60`, ported as [`crate::field_actor_program`]): the travel-art
//! choreography programs the MAN loader resumes after a scene change.
//!
//! The kernel is a pure function of the two actors it touches and a handful of
//! globals ([`ProgramEnv`]); this module is the `jalr node[+0x0C]` arm that
//! feeds it from `World` and applies what it returns.
//!
//! | Kernel input | Engine source |
//! |---|---|
//! | `DAT_1F800393` | the actor-tick cadence [`World::tick_handler_actors`] runs on |
//! | `_DAT_8007BABC` / `_DAT_8007BAA0` | [`crate::world::AudioState::sound_stream`] - the side-band bank request / acknowledge pair, **not** a BGM track (see below) |
//! | `_DAT_8007B868` | [`crate::world::AudioState::dual_mode_gate`] |
//! | `_DAT_1F800394` | [`crate::world::StoryFlagState::story_flags`] |
//! | flag `0x18` | [`World::system_flag_test`] |
//! | player `+0x14..+0x18` / `+0x24..+0x28` / `+0x10` / `+0x72` | the player actor's move state |
//! | player `+0x8E` | [`crate::world::FieldLocomotion::eased_mirror_y`], the halfword the height arm's mirror branch reads |
//!
//! The request word the programs write (`0x7F3`) is the **side-band sound-bank**
//! request `FUN_800243F0` installs into VAB slot 3, the same pair the field
//! VM's op `0x36` subs `1` / `2` drive: the guard state `0x02` runs is the one
//! `scus_leaf_kernels::SoundStreamRequest` documents as inlined at
//! `0x801D4B58..0x801D4B90`. The kernel's `bgm_*` field names predate that
//! reading. The engine's bank loads are synchronous, so a request settles in
//! the same call, exactly as op `0x36` sub `1` does.
//!
//! What the engine does not yet render: the part stages
//! ([`ProgramEffect::StagePart`]) name move-VM effect records resident in the
//! field overlay's data segment (`0x801F22F8..0x801F2658`), which no engine
//! loader reads, so they are only counted into [`SceneProgramFrame`].
//!
//! The CD-XA voice legs are played. [`ProgramEffect::XaCue`] is
//! `FUN_8003D53C(clip, chan, dur)` and lands on the field XA queue both hosts
//! drain ([`World::push_field_xa_cue`]); the drive stays busy for its span,
//! which is what the voice state's `xa_busy` wait reads. [`ProgramEffect::XaStream`]
//! is `FUN_80019794(clip)`, whose body issues `CdlSetloc` + `CdlSeekL` and
//! no read - a seek-ahead to the file the following one-shot plays from, with
//! nothing to render in an engine that has no drive (see
//! [`crate::world::field_xa`]'s module docs). Both are still reported in
//! [`SceneProgramFrame::xa`].
//!
//! REF: FUN_8002519C (the frame walker whose `jalr` this is)

use super::*;

use crate::actor_handler::ActorHandler;
use crate::field_actor_kernels::ACTOR_FLAG_YIELD;
use crate::field_actor_program::{
    PLAYER_LIFTING, ProgramEffect, ProgramEnv, ProgramPlayer, step_scene_program,
};

/// Flag id the closers test before releasing the player
/// (`FUN_8003CE64(0x18)`).
const RELEASE_GUARD_FLAG: u16 = crate::field_actor_program::FLAG_RELEASE_GUARD as u16;

/// What one [`World::tick_scene_programs`] pass did, summed over every live
/// program actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneProgramFrame {
    /// Program actors stepped this pass.
    pub stepped: usize,
    /// Part stages the programs asked for (not rendered - see module docs).
    pub part_stages: usize,
    /// CD-XA legs the programs asked for, `(clip, channel)`; `channel` is
    /// `None` for the whole-clip stream.
    pub xa: Vec<(u8, Option<u8>)>,
    /// Programs that retired themselves this pass.
    pub retired: usize,
}

impl World {
    /// Step every live scripted-scene program actor once.
    ///
    /// PORT driver for FUN_801D4A60 (the kernel is
    /// [`crate::field_actor_program::step_scene_program`])
    ///
    /// Run from [`Self::tick_handler_actors`] with the actor-tick cadence, the
    /// retail `DAT_1F800393` every accumulate state adds. A program whose
    /// player seat is gone steps nothing: every arm reads the player.
    pub fn tick_scene_programs(&mut self, frame_delta: u8) -> SceneProgramFrame {
        let mut out = SceneProgramFrame::default();
        let Some(player_slot) = self.player_actor_slot.map(usize::from) else {
            return out;
        };
        if self.actors.get(player_slot).is_none_or(|a| !a.active) {
            return out;
        }
        let live: Vec<usize> = self
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                a.active && a.handler == ActorHandler::ScriptedScene && a.scene_program.is_some()
            })
            .map(|(slot, _)| slot)
            .collect();
        for slot in live {
            let Some(mut program) = self.actors[slot].scene_program else {
                continue;
            };
            program.flags = self.actors[slot].physics.status_flags;
            let ms = &self.actors[player_slot].move_state;
            let player = ProgramPlayer {
                pos: (ms.world_x, ms.world_y, ms.world_z),
                rot: (ms.render_24, ms.render_26, ms.render_28),
                flags: ms.flags,
                speed: ms.field_72,
                lift: self.locomotion.eased_mirror_y.unwrap_or(0),
            };
            let env = ProgramEnv {
                frame_delta,
                bgm_request: self.audio.sound_stream.requested,
                bgm_current: self.audio.sound_stream.acked,
                dev_flags: self.audio.dual_mode_gate as u32,
                // `_DAT_8007BC20 != 0`: a field one-shot still in its read
                // span (the voice state `0x19` waits it out).
                xa_busy: u32::from(self.field_xa_busy()),
                story_flags: self.flags.story_flags,
                release_guard_set: self.system_flag_test(RELEASE_GUARD_FLAG),
            };
            let was_lifting = player.flags & PLAYER_LIFTING != 0;
            let step = step_scene_program(program, player, env);

            // The program actor.
            let a = &mut self.actors[slot];
            a.scene_program = Some(step.actor);
            a.state_50 = step.actor.program;
            a.state_54 = step.actor.state;
            a.physics.status_flags = step.actor.flags;
            if step.retired {
                a.physics.status_flags |= ACTOR_FLAG_YIELD;
                out.retired += 1;
            }

            // The player.
            let p = step.player;
            let ms = &mut self.actors[player_slot].move_state;
            ms.world_x = p.pos.0;
            ms.world_y = p.pos.1;
            ms.world_z = p.pos.2;
            ms.flags = p.flags;
            ms.field_72 = p.speed;
            // The lift is `+0x8E` under the `0x20000000` mirror bit: publish
            // it where the height arm reads it while the bit is up, and take
            // it back down with the bit.
            if p.flags & PLAYER_LIFTING != 0 {
                self.locomotion.eased_mirror_y = Some(p.lift);
            } else if was_lifting {
                self.locomotion.eased_mirror_y = None;
            }
            self.flags.story_flags = step.story_flags;

            for fx in step.effects {
                match fx {
                    ProgramEffect::SetFlag(id) => self.system_flag_set(u16::from(id)),
                    ProgramEffect::ClearFlag(id) => self.system_flag_clear(u16::from(id)),
                    ProgramEffect::Sfx(id) => self.push_sfx_cue(id as i16),
                    ProgramEffect::RequestBgm(bank) => {
                        if self.audio.sound_stream.request(bank) {
                            self.audio.sound_stream.settle();
                        }
                    }
                    ProgramEffect::StagePart { .. } => out.part_stages += 1,
                    ProgramEffect::XaCue { clip, chan, dur } => {
                        self.push_field_xa_cue(clip, chan, dur);
                        out.xa.push((clip, Some(chan)));
                    }
                    // The seek-ahead: nothing to play (module docs).
                    ProgramEffect::XaStream { clip } => out.xa.push((clip, None)),
                }
            }
            out.stepped += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_actor_program::{
        FLAG_PLAYER_BUSY, FLAG_PROGRAM_1, FLAG_SCENE_ACTIVE, PLAYER_ENGAGED, PLAYER_MOTION_HELD,
        SFX_BEAT, STORY_FLAG_BIT, VOICE_CHANNEL, VOICE_CLIP, VOICE_DURATION,
    };

    /// A world with a live player in slot 0 carrying a real speed multiplier.
    fn world_with_player() -> World {
        let mut w = World::default();
        w.actors[0].active = true;
        w.actors[0].move_state.field_72 = 0x0C00;
        w.actors[0].move_state.world_y = 0x40;
        w.player_actor_slot = Some(0);
        w
    }

    #[test]
    fn a_resumed_closer_runs_to_its_release_through_the_handler_pass() {
        // Program 3 (the flag-`0x0C` closer): park the player, beat, release,
        // clear its own flag, retire. Without the handler-pass driver the
        // flag stays set and the actor sits on the pool forever.
        let mut w = world_with_player();
        w.system_flag_set(u16::from(FLAG_PROGRAM_1));
        assert_eq!(w.man_load_resume_programs(), vec![3]);
        let mut saw_engaged = false;
        let mut frames = 0;
        while w
            .find_actor_by_handler(ActorHandler::ScriptedScene)
            .is_some()
            && frames < 400
        {
            w.tick_handler_actors(2);
            saw_engaged |= w.actors[0].move_state.flags & PLAYER_ENGAGED != 0;
            frames += 1;
        }
        assert!(frames < 400, "the closer never retired");
        assert!(saw_engaged, "the closer engages the player while it runs");
        assert!(
            !w.system_flag_test(u16::from(FLAG_PROGRAM_1)),
            "flag cleared"
        );
        assert!(!w.system_flag_test(u16::from(FLAG_PLAYER_BUSY)));
        let p = &w.actors[0].move_state;
        assert_eq!(
            p.flags & (PLAYER_ENGAGED | PLAYER_MOTION_HELD),
            0,
            "released"
        );
        assert_eq!(p.field_72, 0x0C00, "the parked speed came back");
        assert!(
            w.audio
                .sfx_ring_ops
                .contains(&crate::world::SfxRingOp::Push(SFX_BEAT as i16)),
            "the beat reaches the host SFX ring"
        );
    }

    #[test]
    fn the_lift_closer_reads_the_live_side_band_pair_and_lifts_the_player() {
        // Program 2 (the flag-`0x17` closer) parks on `request == ack` before
        // its voice stream; the engine's pair is settled, so it proceeds, lifts
        // the player through the `+0x8E` mirror and lands the story bit.
        let mut w = world_with_player();
        w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
        assert_eq!(w.man_load_resume_programs(), vec![2]);
        let mut xa = Vec::new();
        let mut lifted = false;
        let mut frames = 0;
        while w
            .find_actor_by_handler(ActorHandler::ScriptedScene)
            .is_some()
            && frames < 800
        {
            // Drive the program directly so the frame reports are visible.
            let f = w.tick_scene_programs(2);
            xa.extend(f.xa);
            w.retire_yielded_actors();
            lifted |= w.locomotion.eased_mirror_y.is_some();
            // The voice one-shot holds the drive busy; one vsync per step
            // lets its span run out so the closing wait releases.
            w.tick_field_xa_busy();
            frames += 1;
        }
        assert!(frames < 800, "the closer never retired");
        assert!(
            xa.contains(&(VOICE_CLIP, None)),
            "the seek-ahead to the voice file"
        );
        assert!(
            xa.contains(&(VOICE_CLIP, Some(VOICE_CHANNEL))),
            "the voice one-shot"
        );
        assert!(lifted, "the lift published the +0x8E mirror");
        assert_eq!(w.locomotion.eased_mirror_y, None, "and took it back down");
        assert!(!w.system_flag_test(u16::from(FLAG_SCENE_ACTIVE)));
        assert_eq!(
            w.flags.story_flags & STORY_FLAG_BIT,
            0,
            "0x1A clears the bit"
        );
    }

    #[test]
    fn the_voice_leg_reaches_the_field_xa_queue_and_holds_the_wait() {
        // Program 2's voice one-shot lands on the queue both hosts drain, and
        // its read span is what state 0x19's `xa_busy` wait reads - the
        // program may not retire while the line is still sounding.
        let mut w = world_with_player();
        w.system_flag_set(u16::from(FLAG_SCENE_ACTIVE));
        assert_eq!(w.man_load_resume_programs(), vec![2]);
        let mut clips = Vec::new();
        let mut first_clip_frame = None;
        let mut retired_frame = None;
        for frame in 0..800 {
            if w.find_actor_by_handler(ActorHandler::ScriptedScene)
                .is_none()
            {
                retired_frame = Some(frame);
                break;
            }
            w.tick_scene_programs(2);
            w.retire_yielded_actors();
            let got = w.drain_field_xa_cues();
            if !got.is_empty() && first_clip_frame.is_none() {
                first_clip_frame = Some(frame);
            }
            clips.extend(got);
            // One world vsync per step, as `World::tick` runs it.
            w.tick_field_xa_busy();
        }
        assert_eq!(
            clips,
            vec![crate::sfx_cue::XaVoiceClip {
                clip: u32::from(VOICE_CLIP),
                channel: u32::from(VOICE_CHANNEL),
                duration_sectors: u32::from(VOICE_DURATION),
            }],
            "exactly one voice one-shot, and no clip for the seek-ahead"
        );
        let (start, end) = (first_clip_frame.unwrap(), retired_frame.unwrap());
        assert!(
            end - start >= usize::from(VOICE_DURATION),
            "the program waited out the {VOICE_DURATION}-vsync span ({start}..{end})"
        );
    }
}
