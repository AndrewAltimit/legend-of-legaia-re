//! The anim commit's two voice sites for a party seat: the SpecialStarter's
//! fanfare and the Tactical-Arts shout. Kernels in
//! [`legaia_engine_battle::arts_voice_cue`]; this is where the commit body
//! (`World::commit_staged_battle_anim_at_boundary`) hands them the world's
//! queue, cursor, latches and `rand()` stream, and where their outputs land:
//! a fanfare id goes through the dispatcher (`World::dispatch_battle_cue`),
//! a shout clip onto the CD-XA cue queue both hosts play, holding the drive
//! busy for its read span as the starter does.

use super::*;

use legaia_engine_battle::arts_voice_cue as avc;

impl World {
    /// The roster id `DAT_8007BD10[seat]` for a party seat (1 Vahn, 2 Noa,
    /// 3 Gala).
    fn arts_voice_char_id(&self, i: usize) -> u8 {
        (self.party_roster_slot(i) as u8).wrapping_add(1)
    }

    /// The SpecialStarter (`0x1A`) commit's voice: the Miracle seat's and the
    /// starter mark's generic line, or the Hyper Art fanfare
    /// ([`avc::starter_commit_voice_ids`]), each through the dispatcher's
    /// CD-XA arm. `stage` is the banner stage the commit found.
    ///
    /// REF: FUN_8004AD80 (`0x8004B74C..0x8004BB44`)
    pub(in crate::world) fn emit_starter_commit_voice(&mut self, i: usize, stage: u8) {
        let char_id = self.arts_voice_char_id(i);
        let seat = self
            .battle_ctx
            .arts_banner_seat_flags
            .get(i)
            .copied()
            .unwrap_or(0)
            != 0;
        let b = &self.actors[i].battle;
        let cursor = usize::from(b.strike_index);
        let mark = cursor
            .checked_sub(1)
            .and_then(|k| b.starter_marks.and_then(|m| m.get(k).copied()));
        let next = b.params.get(cursor).copied().unwrap_or(0);
        let (ids, _) = avc::starter_commit_voice_ids(char_id, seat, mark, stage, next, &mut || {
            self.next_rand()
        });
        for id in ids {
            log::debug!("battle: SpecialStarter voice cue {id:#05x} (seat {i})");
            self.dispatch_battle_cue(id);
        }
    }

    /// The shout site: on a party commit of a staged id `>= 0x10`, call the
    /// arts-voice selector when the queue says so
    /// ([`avc::shout_call_args`]). The call is recorded on
    /// [`crate::world::AudioState::battle_shout_cues`]; with the selector
    /// tables staged it is resolved here too
    /// ([`avc::select_arts_voice`]) - the clip onto the CD-XA cue queue, a
    /// second-half sound cue through the dispatcher or the funnel.
    ///
    /// `solo_latch` is `ctx[+0x243]` before this commit's own store.
    ///
    /// REF: FUN_8004AD80 (`0x8004BCF0..0x8004BD6C`), FUN_8004C140
    pub(in crate::world) fn emit_commit_arts_shout(&mut self, i: usize, solo_latch: u8) {
        let b = &self.actors[i].battle;
        let Some((action, flag)) =
            avc::shout_call_args(&b.params, b.strike_index, b.action_category, solo_latch)
        else {
            return;
        };
        let char_id = self.arts_voice_char_id(i);
        let Some(cslot) = char_id.checked_sub(1).filter(|&c| c < 3) else {
            return;
        };
        let tables = self.audio.arts_voice_tables.clone();
        let resolved = tables.is_some();
        self.audio
            .battle_shout_cues
            .push(legaia_engine_battle::battle_events::BattleShoutCue {
                cslot,
                action,
                resolved,
            });
        let Some(tables) = tables else {
            return;
        };
        let call = avc::ArtsVoiceCall {
            char_id,
            action,
            flag,
            solo_latch,
            first_enemy: self.formation_slot0_monster_id(),
            caller_seat: self.retail_actor_category(i as u8),
        };
        let mut last = self.audio.arts_voice_last_channel;
        let emits = avc::select_arts_voice(&tables, &call, &mut last, &mut || self.next_rand());
        self.audio.arts_voice_last_channel = last;
        for emit in emits {
            log::debug!(
                "battle: arts shout char {char_id} action {action:#04x} flag {flag:#04x} -> {emit:?}"
            );
            match emit {
                avc::ArtsVoiceEmit::Clip { slot, channel, dur } => {
                    self.push_battle_xa_cue(legaia_engine_system::sound_state::XaVoiceClip {
                        clip: u32::from(slot),
                        channel: u32::from(channel),
                        duration_sectors: dur,
                    });
                }
                avc::ArtsVoiceEmit::Dispatch(id) => self.dispatch_battle_cue(id),
                avc::ArtsVoiceEmit::Route(id) => {
                    let cat = self.retail_actor_category(self.battle_ctx.active_actor);
                    self.route_battle_cue(id, cat);
                }
            }
        }
    }
}
