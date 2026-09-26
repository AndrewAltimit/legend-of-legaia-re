//! The field's **timed kind-0 warp** - the World half of the walk-on
//! dispatcher `FUN_801D1EC4`'s warp arms (the scene host owns the tile
//! compare and the trigger tables; this owns the timer, the fades, the
//! landing and the clip state the dispatcher and scene entry reset).
//!
//! Retail does not teleport on the crossing. The kind-0 arm arms a `0x26`-frame
//! timer and two fades, the player tick keeps the pad controller off while it
//! runs, and the player is seated at the destination on the frame the timer
//! runs out - the bottom of the fade to black - followed by a `0x28`-frame
//! hold before the pad comes back. See
//! [`legaia_engine_vm::field_warp_tile`] for the byte-level reading.

use super::*;
use vm::field_warp_tile as warp_tile;

/// What one frame of the warp timer did, for the scene host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldWarpTick {
    /// No warp in flight: the host runs its tile compare.
    Idle,
    /// A warp is counting down: the host skips its tile compare, as retail's
    /// timer-running half returns before it.
    Running,
    /// The warp landed this frame. The player is seated; the host restamps
    /// its crossing tile and runs the kind-1 record at `query_tile`.
    Landed {
        /// The tile the landing queries its kind-1 record at
        /// ([`warp_tile::landing_query_tile`]).
        query_tile: (u8, u8),
        /// The tile the player now stands on (`world >> 7`), which retail
        /// stores as the crossing tile (`0x801D1FB4..0x801D1FE4`).
        landing_tile: (u8, u8),
    },
}

/// A `FUN_801D58F0` fade as the template `FUN_80024E80` loads. The trailing
/// id word is left `0` for [`crate::fade::spawn_fade`] to stamp.
fn warp_fade_template(f: &warp_tile::WarpFade, delay: i16) -> crate::fade::FadeTemplate {
    crate::fade::FadeTemplate {
        kind: f.kind,
        duration: f.duration,
        start_rgb: warp_tile::WarpFade::rgb(f.from_rgb),
        end_rgb: warp_tile::WarpFade::rgb(f.to_rgb),
        mode: [delay, f.hold, 0],
    }
}

impl World {
    /// The kind-0 arm of `FUN_801D1EC4`: store the destination (half-tiles),
    /// start the warp timer, put up the fade to black and schedule the fade
    /// back in. Also clears the player's movement lock, as the arm does
    /// after its two fades (`0x801D2254..0x801D2264`).
    ///
    /// The port has one fade slot where retail spawns two fade actors, so
    /// the fade-in is held back until its `0x29`-frame start delay has run
    /// and then replaces the fade-out, which by then is holding black.
    ///
    /// REF: FUN_801D1EC4 (ported as [`warp_tile::arm_warp`]), FUN_801D58F0
    pub fn arm_field_warp(&mut self, dest: (u8, u8)) {
        let [out, fade_in] = warp_tile::arm_warp(&mut self.locomotion.warp, dest);
        crate::fade::spawn_fade(
            &mut self.presentation.fade,
            &warp_fade_template(&out, out.delay),
            0,
        );
        self.locomotion.warp_fade_in_in = Some(i32::from(fade_in.delay));
        if let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get_mut(slot as usize)
        {
            actor.move_state.flags &= !warp_tile::MOVEMENT_LOCK;
        }
    }

    /// `true` while a kind-0 warp is counting down.
    pub fn field_warp_in_flight(&self) -> bool {
        self.locomotion.warp.timer > 0
    }

    /// One frame of the warp: run the held-back fade-in's delay, count the
    /// timer down, and on the landing frame apply the landing - the
    /// encounter step counter re-rolled when it is spent (`0x801D1F64` /
    /// `0x801D1F6C`), the movement lock cleared, the player seated at
    /// `(dest_x * 64 + 64, (dest_z + 1) * 64)` on the floor there, and a
    /// player `MoveTo` event so the hosts' camera re-pins the way retail's
    /// `FUN_801DAA50` call does.
    ///
    /// Retail also tags the pool actor running `0x801DA7F0` for tear-down
    /// on every running frame; the port has no such actor to tag.
    ///
    /// Every frame ends with the system channel's clear of the `-1000`
    /// landed sentinel ([`warp_tile::clear_landed_sentinel`]), which retail
    /// runs later in the same tick from `FUN_801DA51C`: the timer reads
    /// `-1000` only between the landing and the end of its own tick. The
    /// channel's dialogue / lock gates are not modelled - the landing frame
    /// has neither up.
    ///
    /// REF: FUN_801D1EC4 (ported as [`warp_tile::tick_warp_timer`]),
    /// FUN_801DDF48, FUN_80019278, FUN_801DA51C
    pub fn tick_field_warp(&mut self) -> FieldWarpTick {
        let tick = self.tick_field_warp_timer();
        warp_tile::clear_landed_sentinel(&mut self.locomotion.warp);
        tick
    }

    fn tick_field_warp_timer(&mut self) -> FieldWarpTick {
        let delta = self.move_vm.ramp_ratio.max(1);
        if let Some(left) = self.locomotion.warp_fade_in_in.as_mut() {
            *left -= i32::from(delta);
            if *left <= 0 {
                self.locomotion.warp_fade_in_in = None;
                crate::fade::spawn_fade(
                    &mut self.presentation.fade,
                    &warp_fade_template(&warp_tile::WARP_FADE_IN, 0),
                    0,
                );
            }
        }
        let counter = self.encounters.step_counter;
        match warp_tile::tick_warp_timer(&mut self.locomotion.warp, delta, counter) {
            warp_tile::WarpStep::Idle => FieldWarpTick::Idle,
            warp_tile::WarpStep::Running => FieldWarpTick::Running,
            warp_tile::WarpStep::Landed {
                world,
                query_tile,
                reroll,
            } => {
                if reroll {
                    self.reroll_encounter_step_counter();
                }
                let y =
                    self.sample_field_floor_height(i32::from(world.0), i32::from(world.1)) as i16;
                if let Some(slot) = self.player_actor_slot
                    && let Some(actor) = self.actors.get_mut(slot as usize)
                {
                    let ms = &mut actor.move_state;
                    ms.flags &= !warp_tile::MOVEMENT_LOCK;
                    ms.world_x = world.0;
                    ms.world_z = world.1;
                    ms.world_y = y;
                }
                self.pending_field_events
                    .push(crate::field_events::FieldEvent::MoveTo {
                        world_x: world.0 as u16,
                        world_z: world.1 as u16,
                        is_player: true,
                    });
                let tile = |w: i16| (i32::from(w) >> 7).clamp(0, 0x7F) as u8;
                FieldWarpTick::Landed {
                    query_tile,
                    landing_tile: (tile(world.0), tile(world.1)),
                }
            }
        }
    }

    /// The walk-on arm's clip write under `_DAT_8007B6A8`: a kind-1 hit in a
    /// flagged scene resets the clip base to idle and raises the player's
    /// party-bank bit (`0x801D218C..0x801D21BC`). The touch post
    /// `FUN_801D5B5C` makes the same write (`0x801D5B90..0x801D5BAC`).
    ///
    /// REF: FUN_801D1EC4
    pub fn field_walk_on_clip_reset(&mut self) {
        if self.party.scene_save_allowed {
            self.locomotion.clip_base = vm::field_player_clip::BASE_IDLE;
            self.locomotion.player_party_bank = true;
        }
    }

    /// Scene entry's reset of the warp and clip globals: no warp carries
    /// across a scene change, and the clip base starts at idle (SCUS
    /// `0x8003B364`, `addiu v0, zero, 2; sw v0, _DAT_8007BDD8`).
    pub fn reset_field_warp_and_clip(&mut self) {
        self.locomotion.warp = vm::field_warp_tile::WarpTimer::default();
        self.locomotion.warp_fade_in_in = None;
        self.locomotion.clip_base = vm::field_player_clip::BASE_IDLE;
        self.locomotion.player_party_bank = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_player() -> World {
        let mut w = World::new();
        w.mode = crate::world::SceneMode::Field;
        w.install_field_player(0);
        w
    }

    #[test]
    fn a_warp_seats_the_player_only_when_the_timer_runs_out() {
        let mut w = world_with_player();
        let slot = w.player_actor_slot.unwrap() as usize;
        w.actors[slot].move_state.world_x = 100;
        w.actors[slot].move_state.world_z = 200;
        w.set_encounter_step_counter(300);
        w.arm_field_warp((21, 40));
        assert!(w.field_warp_in_flight());
        assert!(w.presentation.fade.is_some(), "the fade to black is up");
        let mut landed = None;
        for frame in 0..warp_tile::WARP_TIMER_FRAMES {
            match w.tick_field_warp() {
                FieldWarpTick::Running => {
                    assert_eq!(w.actors[slot].move_state.world_x, 100, "frame {frame}");
                }
                FieldWarpTick::Landed {
                    query_tile,
                    landing_tile,
                } => landed = Some((frame, query_tile, landing_tile)),
                FieldWarpTick::Idle => panic!("idle mid-warp"),
            }
        }
        let (frame, query, tile) = landed.expect("landed");
        assert_eq!(frame, warp_tile::WARP_TIMER_FRAMES - 1);
        assert_eq!(query, (10, 20));
        assert_eq!(tile, (11, 20), "(1408 >> 7, 2624 >> 7)");
        let ms = &w.actors[slot].move_state;
        assert_eq!((ms.world_x, ms.world_z), (21 * 64 + 64, 41 * 64));
        assert_eq!(w.encounter_step_counter(), 300, "a live counter carries");
        // The system channel clears the -1000 sentinel the same tick.
        assert_eq!(
            w.locomotion.warp.timer, 0,
            "sentinel cleared on the landing tick"
        );
        // The hold keeps the pad off after the landing.
        assert!(vm::field_warp_tile::pad_suppressed(&w.locomotion.warp));
    }

    #[test]
    fn a_spent_counter_is_rerolled_at_the_landing() {
        let mut w = world_with_player();
        w.set_encounter_step_counter(0);
        w.arm_field_warp((0, 0));
        while !matches!(w.tick_field_warp(), FieldWarpTick::Landed { .. }) {}
        let c = w.encounter_step_counter();
        assert!(
            (488..=1460).contains(&c),
            "rerolled into the triangular band, got {c}"
        );
    }

    #[test]
    fn the_fade_in_replaces_the_fade_out_after_its_delay() {
        let mut w = world_with_player();
        w.arm_field_warp((0, 0));
        let out_kind = w.presentation.fade.as_ref().unwrap().rgb();
        assert_eq!(out_kind, [0, 0, 0], "the fade-out starts at black");
        for _ in 0..warp_tile::WARP_FADE_IN.delay {
            w.tick_field_warp();
        }
        let f = w.presentation.fade.as_ref().expect("fade-in live");
        assert_eq!(f.rgb(), [0xFF; 3], "the fade-in starts from the held black");
        assert!(w.locomotion.warp_fade_in_in.is_none());
    }
}
