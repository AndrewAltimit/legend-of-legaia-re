//! The field-to-minigame round trip shared by every minigame: the mode set, the
//! escape poll, the entry and return warps and the SFX cue drain.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Drain the minigame SFX cue ids queued this frame (the fishing hub's
    /// and point exchange's blips; the dance stores its cues straight into
    /// the SFX ring instead, as retail does). Cosmetic - a
    /// host with no audio drops them, exactly as an unheard retail cue would
    /// be. Both hosts drain this queue, which is what keeps the two from
    /// growing separate cue paths.
    pub fn drain_minigame_sfx_cues(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.minigames.pending_sfx)
    }

    /// The five mode-24 minigame scene modes, in `sub_id` order.
    pub const MINIGAME_MODES: [SceneMode; 5] = [
        SceneMode::Fishing,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
        SceneMode::Dance,
    ];

    /// Whether the world is inside one of the five mode-24 minigames.
    pub fn in_minigame(&self) -> bool {
        Self::MINIGAME_MODES.contains(&self.mode)
    }

    /// **Engine affordance, not a retail port: Start leaves any minigame.**
    ///
    /// Retail's five minigames each quit through their own overlay's SM - the
    /// slot cabinet's exit menu row, the duel's decided-match confirm, the
    /// arena's give-up arm - and every one of those is a *different* control
    /// in a *different* overlay. Some are ported (the cabinet's cash-out quit
    /// row through `SlotMachine::cash_out_input`, the duel cabinet's PAY OUT
    /// choice), but each is reachable only from its own game's screens and
    /// not every game's exit is, so without one shared exit an entered
    /// minigame can still strand the player in a mode with the BGM running.
    ///
    /// That is the invariant [`legaia_engine_scene_host::scene_host::SceneHost::drain_minigame_warp`]
    /// already states for its *failure* arms - "a script that armed a warp must
    /// never be left in a mode with no exit" - and it has to hold for the
    /// successful ones too, on every host, or a reachable minigame is a
    /// softlock. Each game's own `exit_*` runs, so the bookkeeping (cash-out,
    /// leg report, point bank) is the same as the deliberate exit; a door-warp
    /// entry additionally closes its round trip through
    /// [`Self::minigame_return_warp`].
    pub(in crate::world::frame_tick) fn poll_minigame_escape(&mut self) {
        if !self.in_minigame() || !self.input.just_pressed(input::PadButton::Start) {
            return;
        }
        match self.mode {
            SceneMode::Fishing => {
                self.exit_fishing();
            }
            SceneMode::SlotMachine => {
                self.exit_slot_machine();
            }
            // The attract card reads Start itself (its `0x844` edge is
            // Start, Cross or L1), so there Start begins rather than quits;
            // from the player select on it quits as everywhere else.
            SceneMode::BakaFighter
                if self.minigames.baka_fighter.as_ref().is_some_and(|f| {
                    f.cabinet().state() == legaia_engine_minigames::baka_cabinet::ST_ATTRACT
                }) =>
            {
                return;
            }
            SceneMode::BakaFighter => {
                self.exit_baka_fighter();
            }
            SceneMode::MuscleDome => {
                self.leave_muscle_dome();
            }
            SceneMode::Dance => {
                self.exit_dance();
            }
            _ => unreachable!("guarded by in_minigame"),
        }
        self.close_minigame_round_trip();
    }

    /// Close the mode-24 round trip after a minigame exit, when the entry
    /// came through the door warp (a backed-up scene name is armed):
    /// [`Self::minigame_return_warp`] restores the departure label, banks the
    /// session winnings and drops back to the field. A no-op for a session a
    /// debug launcher opened (nothing armed), and after `exit_baka_fighter`,
    /// which runs its own return warp.
    ///
    /// Every exit path calls it after its `exit_*`: the Start escape here,
    /// the native window's minigame hotkeys and the browser page's fishing
    /// button. The hotkeys and the button used to call the bare `exit_*`, so
    /// leaving a door-entered session that way kept the scene backup armed
    /// and never banked the winnings - only Start closed the trip.
    pub fn close_minigame_round_trip(&mut self) {
        if self.minigames.scene_backup.is_some() {
            self.minigame_return_warp();
        }
    }

    /// Arm the mode-24 minigame door-warp: back up the active scene name and
    /// zero the session-winnings accumulator, so [`Self::minigame_return_warp`]
    /// can round-trip back to the departure scene.
    ///
    /// Mirrors the two retail halves of the entry: the field-VM `0x3E` warp
    /// arm zeroes the winnings accumulator `_DAT_80084440`, and the mode-24
    /// OTHER-INIT entry `FUN_80025980` copies the active scene name
    /// `0x80084548` into the backup at `0x8007BAE8` before the minigame
    /// overlay clobbers the field.
    // REF: FUN_80025980 (scene-name backup half), FUN_801DE840 case 0x3E
    //      (winnings-accumulator zero half)
    pub fn arm_minigame_warp(&mut self) {
        self.minigames.scene_backup = Some(self.active_scene_label.clone());
        self.minigames.winnings = 0;
    }

    /// Request the mode-24 door-warp into `sub_id` from outside a script -
    /// the developer launchers (the native window's minigame hotkeys, the
    /// browser page's `play_mg_debug_warp`). Arms the round trip exactly as
    /// the op-`0x3E` arm does and leaves the `sub_id` for the scene host's
    /// next tick to drain (`SceneHost::drain_minigame_warp`), so a launcher
    /// enters the same session, with the same BGM swap and the same return
    /// warp, as walking through the casino door.
    ///
    /// A launcher into the Muscle Dome also lists the hub's announcer lines
    /// for the prestage drain here, at the request: a host that drains the
    /// list before its next tick (the browser page's launcher does) gets the
    /// lead the door scene's own list gives a walked door
    /// ([`crate::world::field_xa::scene_minigame_door_xa_prestage`]).
    pub fn request_minigame_warp(&mut self, sub_id: u8) {
        self.arm_minigame_warp();
        self.minigames.pending_warp = Some(sub_id);
        if legaia_engine_mode::minigame_entry::MinigameSubId::from_sub_id(sub_id)
            == Some(legaia_engine_mode::minigame_entry::MinigameSubId::MuscleDome)
        {
            self.queue_xa_prestage(legaia_engine_minigames::muscle_ringside::hub_xa_prestage());
        }
    }

    /// Mode-24 minigame exit / return-warp: restore the backed-up scene name
    /// into [`Self::active_scene_label`], commit the session winnings into
    /// the casino coin bank (`casino_coins += minigame_winnings`, saturating
    /// at the retail `9_999_999` cap), and drop back to [`SceneMode::Field`]
    /// (retail latches `_DAT_8007B83C = 2`, mode 2 MAIN INIT, whose
    /// per-scene initializer reloads the restored scene; the engine keeps
    /// the field state resident underneath its minigame sessions, so
    /// restoring the label + mode completes the same round trip without a
    /// reload).
    ///
    /// Distinct from the slot overlay's cash-out ([`Self::exit_slot_machine`],
    /// an *assignment* into the bank): this commit is a delta-add of the
    /// accumulator (`_DAT_800845A4 += _DAT_80084440`).
    ///
    /// The winnings commit runs even when no warp is armed (retail's add is
    /// unconditional); only the name restore needs the backup.
    // PORT: FUN_80026018
    pub fn minigame_return_warp(&mut self) {
        self.minigames.casino_coins = self
            .minigames
            .casino_coins
            .saturating_add(self.minigames.winnings)
            .min(9_999_999);
        if let Some(name) = self.minigames.scene_backup.take() {
            self.active_scene_label = name;
        }
        // Give the departure scene its own track back when the minigame's
        // overlay init took the score over (dance / Baka / dome). No-op for
        // the two slots that never displaced it.
        self.restore_minigame_bgm();
        self.mode = SceneMode::Field;
        // The field entry it lands in is MAIN INIT's warp arm, which takes
        // twenty seconds off a running escape clock (`0x8003B62C`).
        self.battle.escape_timer.field_return_penalty();
    }
}
