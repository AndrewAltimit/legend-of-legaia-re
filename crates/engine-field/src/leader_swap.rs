//! The field party-leader swap controller [`LeaderSwap`] (`FUN_801D27E0`).
//!
//! The `PORT` tag lives on the item that implements the address, so the
//! liveness audit sees one anchor per port site.

/// The story-flag index the swap controller requires before it will run at all
/// (`func_0x8003CE64(0xD)`). Clear retires the controller outright.
pub const LEADER_SWAP_ENABLE_FLAG: u16 = 0x0D;

/// Story flags `0x10`, `0x11` and `0x12` encode *which* of the three party
/// characters currently leads: the swap clears all three and sets
/// `LEADER_FLAG_BASE + new_leader`.
pub const LEADER_FLAG_BASE: u16 = 0x10;

/// Party slots the controller cycles through (`sltiu v0,s2,0x3`).
pub const LEADER_SLOTS: u8 = 3;

/// Frames each of the two fade halves runs for (`li s5,0x20`) - it is both the
/// fade template's duration and the `+0x9E` counter's bound.
pub const LEADER_SWAP_FADE_FRAMES: u16 = 0x20;

/// Bit the controller raises on the camera object's flag word `+0x10` while a
/// swap is in flight, and lowers again in the final state.
pub const CAMERA_BUSY_BIT: u32 = 0x0008_0000;

/// Pad-word bit (`_DAT_1F800394 & 0x400`) that, like the camera busy bit,
/// routes the arm test through the story flags rather than through
/// `_DAT_8007B874`.
pub const LEADER_SWAP_PAD_BIT: u32 = 0x0400;

/// Bit of `_DAT_8007B874` that requests a swap when neither the camera busy bit
/// nor [`LEADER_SWAP_PAD_BIT`] is set.
pub const LEADER_SWAP_REQUEST_BIT: u32 = 0x80;

/// The value written into the outgoing leader actor's `+0x14` and `+0x18` once
/// the camera has taken its pose (`li v1,0x3f80`).
pub const LEADER_ACTOR_POSE_SENTINEL: i16 = 0x3F80;

/// Shift from world units to the walkability grid's tile units, applied to the
/// camera's x and z before the field-grid calls (`sll 0x10; sra 0x17`, i.e. a
/// sign-extended `>> 7`). It is the same 128-unit tile
/// `docs/subsystems/field-locomotion.md` describes.
pub const WORLD_TO_TILE_SHIFT: u32 = 7;

/// One party actor's cached pose - the `0x10`-byte record state `0` writes for
/// each of the three slots into the table at `0x800845E4`. Each component is
/// stored as a **word**, sign-extended from the actor's halfword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartyActorPose {
    /// From actor `+0x14`.
    pub x: i32,
    /// From actor `+0x16`.
    pub y: i32,
    /// From actor `+0x18`.
    pub z: i32,
    /// From actor `+0x26`.
    pub facing: i32,
}

/// What the swap controller asks its host to do this frame, in the order retail
/// issues them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderSwapEffect {
    /// State `0`: cache all three party actors' poses into the `0x800845E4`
    /// table. Runs before any gate, on every state-0 frame.
    CachePartyPoses,
    /// State `0`: spawn the fade-to-white and keep the object it returns in
    /// `_DAT_801F3490`. The template is `legaia_engine_system::fade::FadeTemplate` kind `2`,
    /// `0x20` frames, black to white.
    SpawnFadeOut,
    /// State `2`: write the camera object's current pose back onto the
    /// **outgoing** leader's actor, so it stands where the camera left it.
    StoreOutgoingPose {
        /// The slot being left.
        slot: u8,
    },
    /// State `2`: the leader index changed. The host writes it to
    /// `_DAT_8007B8F8`, `DAT_80084597` and `DAT_80084598`, clears story flags
    /// `0x10..=0x12` and sets [`LEADER_FLAG_BASE`]` + slot`.
    CommitLeader {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: `FUN_801DE190()` - rebuild the party display.
    RefreshParty,
    /// State `2`: copy the incoming leader's pose onto the camera object
    /// (`+0x14`/`+0x1C`, `+0x16`/`+0x1E`, `+0x18`/`+0x20`, `+0x26`) and
    /// re-anchor the field grid on it - `func_0x80017EC8`, `FUN_801DE3E0`, the
    /// negated origin at `_DAT_80089118` / `_DAT_80089120`, then `FUN_801DB8EC`
    /// and `FUN_801DAA50`.
    RecentreCamera {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: stamp [`LEADER_ACTOR_POSE_SENTINEL`] into the incoming leader
    /// actor's `+0x14` and `+0x18`.
    ClearIncomingPose {
        /// The slot taking over.
        slot: u8,
    },
    /// State `2`: spawn the fade-back-in (white to black), then
    /// `func_0x8003BDE0(x, z, +0x72, 1)` with the tile-space camera position.
    SpawnFadeIn,
    /// State `3`: OR bit `3` into the spawned fade object's `+0x10`.
    ReleaseFadeObject,
    /// State `4`: clear [`CAMERA_BUSY_BIT`] from the camera object's `+0x10`.
    ClearCameraBusy,
    /// State `5`: clear bit `3` of the controller's own `+0x10` - it retires.
    RetireController,
}

/// The controller's `+0x54` phase and `+0x9E` counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaderSwap {
    /// `+0x54` - the six-way phase. The switch is bounded by `sltiu v1,0x6`, so
    /// anything higher runs no body at all.
    pub phase: i16,
    /// `+0x9E` - the fade counter, reset at each phase transition.
    pub counter: u16,
    /// `+0x50` - the story-flag base the presence tests are made against.
    pub flag_base: u16,
}

/// Everything the controller reads that it does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaderSwapWorld {
    /// `DAT_80084597` - the live leader slot.
    pub leader: u8,
    /// `_DAT_8007B6B4` - the first suppressor. The arm needs it zero.
    pub suppress_a: i32,
    /// `_DAT_8007B6B0` - the second suppressor. The arm needs it `<= 0`.
    pub suppress_b: i32,
    /// The camera object's flag word `+0x10`.
    pub camera_flags: u32,
    /// `_DAT_1F800394` - the pad word.
    pub pad: u32,
    /// `_DAT_8007B874`.
    pub request_byte: u32,
}

/// Result of one controller frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeaderSwapTick {
    /// Host calls in retail order.
    pub effects: Vec<LeaderSwapEffect>,
    /// The leader slot the swap settled on, when state `2` ran.
    pub new_leader: Option<u8>,
}

impl LeaderSwap {
    /// One frame of `FUN_801D27E0` - the field controller that switches which
    /// party member walks the map.
    ///
    /// `flag` tests the story-flag bank `DAT_80085758` (`func_0x8003CE64`). It
    /// is consulted for the enable flag, for each of the three
    /// `flag_base + n` presence flags, and again inside the search that picks
    /// the next leader.
    ///
    /// | state | body |
    /// |---|---|
    /// | `0` | Cache the three poses, run the arm gate, and on success spawn the fade-out and advance. |
    /// | `1` | Hold for [`LEADER_SWAP_FADE_FRAMES`], then advance. |
    /// | `2` | Perform the swap, spawn the fade-in, advance. |
    /// | `3` | Release the fade object, advance. |
    /// | `4` | Hold for [`LEADER_SWAP_FADE_FRAMES`], then clear the busy bit and return to state `0`. |
    /// | `5` | Retire the controller. |
    ///
    /// The arm gate has three parts, in order. **All three** presence flags set
    /// means there is nothing to swap to and the frame ends. Exactly two set
    /// additionally requires `flag_base + leader`. Then either the camera busy
    /// bit or the pad bit routes the final test through `flag_base + leader`,
    /// while their absence falls back to [`LEADER_SWAP_REQUEST_BIT`] of
    /// `_DAT_8007B874`.
    ///
    /// PORT: FUN_801D27E0
    /// REF: FUN_801DE190, FUN_801DE3E0, FUN_801DB8EC, FUN_801DAA50
    ///
    /// WIRED: `legaia_engine_core::World::tick_three_actor_talk` hosts this
    /// SM for the active three-actor-talk session (field-VM op `0x43` sub-2
    /// installs it; the session's `swap` field is the controller). The
    /// three-actor-talk participants' field-NPC records stand in for
    /// retail's three resident party actors -
    /// [`LeaderSwapEffect::StoreOutgoingPose`] /
    /// [`LeaderSwapEffect::ClearIncomingPose`] read and write
    /// `World::npcs.positions`, and the session's saved-pose table is
    /// the `0x800845E4` mirror.
    pub fn step(
        &mut self,
        world: &LeaderSwapWorld,
        frame_step: u8,
        mut flag: impl FnMut(u16) -> bool,
    ) -> LeaderSwapTick {
        let mut out = LeaderSwapTick::default();
        match self.phase {
            0 => {
                out.effects.push(LeaderSwapEffect::CachePartyPoses);
                if world.suppress_a != 0 || world.suppress_b > 0 {
                    return out;
                }
                if !flag(LEADER_SWAP_ENABLE_FLAG) {
                    self.phase = 5;
                    return out;
                }
                let present = (0..LEADER_SLOTS)
                    .filter(|n| flag(self.flag_base + u16::from(*n)))
                    .count();
                if present == 3 {
                    return out;
                }
                let leader_flag = self.flag_base + u16::from(world.leader);
                if present == 2 && !flag(leader_flag) {
                    return out;
                }
                let armed = if (world.camera_flags & CAMERA_BUSY_BIT) != 0
                    || (world.pad & LEADER_SWAP_PAD_BIT) != 0
                {
                    flag(leader_flag)
                } else {
                    (world.request_byte & LEADER_SWAP_REQUEST_BIT) != 0
                };
                if !armed {
                    return out;
                }
                out.effects.push(LeaderSwapEffect::SpawnFadeOut);
                self.counter = 0;
                self.phase += 1;
            }
            1 => {
                self.counter = self.counter.wrapping_add(u16::from(frame_step));
                if self.counter < LEADER_SWAP_FADE_FRAMES {
                    return out;
                }
                self.counter = 0;
                self.phase += 1;
            }
            2 => {
                out.effects
                    .push(LeaderSwapEffect::StoreOutgoingPose { slot: world.leader });
                // Step past the current slot, wrapping at three, until a
                // presence flag reads clear. The `addiu s2,s2,-1` at
                // `801d2ae4` undoes the increment that ran in the branch's
                // delay slot, which is why the loop settles *on* the clear
                // slot rather than one past it.
                let mut slot = world.leader;
                loop {
                    slot = if slot + 1 >= LEADER_SLOTS {
                        0
                    } else {
                        slot + 1
                    };
                    if !flag(self.flag_base + u16::from(slot)) {
                        break;
                    }
                }
                out.new_leader = Some(slot);
                out.effects.push(LeaderSwapEffect::CommitLeader { slot });
                out.effects.push(LeaderSwapEffect::RefreshParty);
                out.effects.push(LeaderSwapEffect::RecentreCamera { slot });
                out.effects
                    .push(LeaderSwapEffect::ClearIncomingPose { slot });
                out.effects.push(LeaderSwapEffect::SpawnFadeIn);
                self.counter = 0;
                self.phase += 1;
            }
            3 => {
                out.effects.push(LeaderSwapEffect::ReleaseFadeObject);
                self.counter = 0;
                self.phase += 1;
            }
            4 => {
                self.counter = self.counter.wrapping_add(u16::from(frame_step));
                if self.counter >= LEADER_SWAP_FADE_FRAMES {
                    out.effects.push(LeaderSwapEffect::ClearCameraBusy);
                    self.phase = 0;
                }
            }
            5 => out.effects.push(LeaderSwapEffect::RetireController),
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swap_world(leader: u8) -> LeaderSwapWorld {
        LeaderSwapWorld {
            leader,
            request_byte: LEADER_SWAP_REQUEST_BIT,
            ..Default::default()
        }
    }

    /// Two of three characters are in the party: slots 0 and 2.
    fn two_present(base: u16) -> impl FnMut(u16) -> bool {
        move |f| f == LEADER_SWAP_ENABLE_FLAG || f == base || f == base + 2
    }

    #[test]
    fn the_swap_caches_the_poses_before_any_gate_can_stop_it() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            suppress_a: 1,
            ..swap_world(0)
        };
        let tick = c.step(&w, 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn a_clear_enable_flag_retires_the_controller() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        c.step(&swap_world(0), 1, |_| false);
        assert_eq!(c.phase, 5);
        let tick = c.step(&swap_world(0), 1, |_| false);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::RetireController]);
        assert_eq!(c.phase, 5, "state 5 does not advance");
    }

    #[test]
    fn a_full_party_has_nothing_to_swap_to() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let tick = c.step(&swap_world(0), 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn two_present_needs_the_leader_flag_as_well() {
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        // Leader is slot 1, whose flag is clear -> the extra test fails.
        let tick = c.step(&swap_world(1), 1, two_present(0x40));
        assert_eq!(tick.effects, vec![LeaderSwapEffect::CachePartyPoses]);
        assert_eq!(c.phase, 0);

        // Leader is slot 0, whose flag is set.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let tick = c.step(&swap_world(0), 1, two_present(0x40));
        assert!(tick.effects.contains(&LeaderSwapEffect::SpawnFadeOut));
        assert_eq!(c.phase, 1);
    }

    #[test]
    fn the_camera_busy_bit_reroutes_the_arm_test_to_the_flags() {
        // Request byte clear, so the fallback path would refuse.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            camera_flags: CAMERA_BUSY_BIT,
            ..swap_world(0)
        };
        assert!(
            c.step(&w, 1, two_present(0x40))
                .effects
                .contains(&LeaderSwapEffect::SpawnFadeOut)
        );

        // The pad bit does the same job.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            pad: LEADER_SWAP_PAD_BIT,
            ..swap_world(0)
        };
        assert!(
            c.step(&w, 1, two_present(0x40))
                .effects
                .contains(&LeaderSwapEffect::SpawnFadeOut)
        );

        // Neither set and the request byte clear: nothing happens.
        let mut c = LeaderSwap {
            flag_base: 0x40,
            ..Default::default()
        };
        let w = LeaderSwapWorld {
            request_byte: 0,
            ..swap_world(0)
        };
        assert_eq!(c.step(&w, 1, two_present(0x40)).effects.len(), 1);
    }

    #[test]
    fn the_two_hold_states_wait_the_fade_out() {
        let mut c = LeaderSwap {
            phase: 1,
            flag_base: 0x40,
            ..Default::default()
        };
        for _ in 0..0x1F {
            assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
            assert_eq!(c.phase, 1);
        }
        c.step(&swap_world(0), 1, |_| true);
        assert_eq!(c.phase, 2);
        assert_eq!(c.counter, 0, "the counter resets on the transition");
    }

    #[test]
    fn the_swap_picks_the_next_absent_slot_and_wraps() {
        let mut c = LeaderSwap {
            phase: 2,
            flag_base: 0x40,
            ..Default::default()
        };
        // Slots 0 and 2 present, leader 0 -> the search stops on slot 1.
        let tick = c.step(&swap_world(0), 1, two_present(0x40));
        assert_eq!(tick.new_leader, Some(1));
        assert_eq!(
            tick.effects,
            vec![
                LeaderSwapEffect::StoreOutgoingPose { slot: 0 },
                LeaderSwapEffect::CommitLeader { slot: 1 },
                LeaderSwapEffect::RefreshParty,
                LeaderSwapEffect::RecentreCamera { slot: 1 },
                LeaderSwapEffect::ClearIncomingPose { slot: 1 },
                LeaderSwapEffect::SpawnFadeIn,
            ]
        );
        assert_eq!(c.phase, 3);

        // From leader 2 the search wraps through 0 (present) onto 1.
        let mut c = LeaderSwap {
            phase: 2,
            flag_base: 0x40,
            ..Default::default()
        };
        assert_eq!(
            c.step(&swap_world(2), 1, two_present(0x40)).new_leader,
            Some(1)
        );
    }

    #[test]
    fn the_tail_states_release_the_fade_and_clear_the_busy_bit() {
        let mut c = LeaderSwap {
            phase: 3,
            flag_base: 0x40,
            ..Default::default()
        };
        assert_eq!(
            c.step(&swap_world(0), 1, |_| true).effects,
            vec![LeaderSwapEffect::ReleaseFadeObject]
        );
        assert_eq!(c.phase, 4);
        for _ in 0..0x1F {
            assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
        }
        let tick = c.step(&swap_world(0), 1, |_| true);
        assert_eq!(tick.effects, vec![LeaderSwapEffect::ClearCameraBusy]);
        assert_eq!(c.phase, 0);
    }

    #[test]
    fn an_out_of_range_phase_runs_no_body() {
        let mut c = LeaderSwap {
            phase: 9,
            ..Default::default()
        };
        assert!(c.step(&swap_world(0), 1, |_| true).effects.is_empty());
        assert_eq!(c.phase, 9);
    }
}
