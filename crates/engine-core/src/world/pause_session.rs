//! The field overlay's **pause-menu session** after the menu closes, and the
//! travel-art hand-off it makes for the two Door items.
//!
//! Retail opens the pause menu through the field overlay's subsystem actor.
//! The menu button (`FUN_801D01B0`, `0x801D0250..0x801D0328`) spawns it; its
//! installer `FUN_801F1278` stores handler id `7` (`0x801F140C`), and handler
//! `7` - `FUN_801F1F4C`, ported as
//! [`legaia_engine_vm::field_state_pick::state_pick`] - moves it on to handler
//! `0x30`, the session `FUN_801ED308`. The session ramps the screen, spawns the
//! menu (`FUN_801D841C`) and parks until the menu's exit code
//! `_DAT_8007B43C` reaches `6`. The menu's close adds `3` to the code its Use
//! flow left (`0x801DC9E0`), so a Door of Light (`4`) arrives as `7` and a Door
//! of Wind (`5`) as `8`. The ramp-down arm then stores `code - 1` as the next
//! phase: phase `6` hands the actor to handler `0x29` (Riremito,
//! `FUN_801EE094`), phase `7` to handler `0x2B` (Rula, `FUN_801EE328`).
//!
//! The engine's pause menu is the hosts' `MenuRuntime`, so phases `0..3` (the
//! ramp up, the spawn and the park) have already happened by the time a host
//! applies the closed menu's outcome. What this module runs is the rest:
//! [`World::begin_pause_session_exit`] seeds the session at the park phase with
//! the counter the close left, [`World::tick_pause_session`] runs the retail
//! ramp-down ([`legaia_engine_vm::world_map_panel_actors::fade_flash_tick`])
//! to its terminal arm, installs the travel art the arm's handler id names
//! ([`TravelArt::for_handler_id`]) and runs it
//! ([`legaia_engine_vm::travel_art_actor::TravelArtActor::tick`]) until its
//! resolve issues the scene transition.
//!
//! The resolve reads three words retail keeps at `0x80084624` / `0x80084628`
//! / `0x8008462C`: the destination's tile X, its raw CDNAME TOC index, and its
//! tile Z. A Door of Wind use writes all three from the picked quick-travel
//! record (`FUN_801D8B90`); for a Door of Light they hold the kingdom map the
//! party last stood on. The engine carries them as a [`PauseTravelTarget`].
//!
//! One deliberate divergence: retail runs the art whether or not the word
//! resolves and parks in the `UNFIND MAP NUMBER` phase on a miss, with the
//! opener program still holding the player. The engine resolves the target
//! before it installs anything and drops the use on a miss, as the direct
//! transition it replaces did.

use legaia_engine_vm::travel_art_actor::{self as ta, TravelArt, TravelArtActor};
use legaia_engine_vm::world_map_panel_actors::{
    self as wmpa, FadeFlashEffect, FadeFlashInput, fade_flash_tick,
};

use crate::world::World;

/// Handler id of the pause-menu session (`FUN_801ED308`).
pub const HANDLER_PAUSE_SESSION: u16 = 0x30;

/// The destination a travel art's resolve reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseTravelTarget {
    /// The scene the raw TOC index at `0x80084628` names.
    pub scene: String,
    /// Tile X (`0x80084624`).
    pub tile_x: u8,
    /// Tile Z (`0x8008462C`).
    pub tile_z: u8,
}

/// Where the session is.
#[derive(Debug, Clone, PartialEq)]
pub enum PauseSessionStage {
    /// `FUN_801ED308` is running (handler `0x30`).
    Session {
        /// Phase halfword `ctx[+0x54]`.
        phase: i16,
        /// Brightness accumulator `_DAT_8007B440`.
        level: i32,
        /// Exit code `_DAT_8007B43C`.
        counter: i32,
    },
    /// The session handed on to a travel art (handler `0x29` / `0x2B`).
    Art(TravelArtActor),
    /// The art issued its scene transition; the session waits for the scene
    /// host to take it, then drops the fade the art left holding black.
    Arriving,
}

/// One live pause-menu session.
#[derive(Debug, Clone, PartialEq)]
pub struct PauseSession {
    /// The subsystem actor's handler id `ctx[+0x50]`.
    pub handler_id: u16,
    /// Where the session is.
    pub stage: PauseSessionStage,
    /// What the art's resolve reads.
    pub target: PauseTravelTarget,
}

/// The fade the art's phase exit spawns (`FUN_80024E80` on the stack template
/// at `0x801EE160..0x801EE1A0` / `0x801EE470..0x801EE4B0`).
fn travel_fade_template() -> crate::fade::FadeTemplate {
    crate::fade::FadeTemplate {
        kind: ta::FLASH_FADE_KIND,
        duration: ta::FLASH_FADE_FRAMES,
        start_rgb: [0; 3],
        end_rgb: [ta::FLASH_FADE_END; 3],
        mode: [0, ta::FLASH_FADE_HOLD, 0],
    }
}

fn is_travel_fade(f: &crate::fade::FadeState) -> bool {
    f.kind == i32::from(ta::FLASH_FADE_KIND)
        && f.mode[1] == ta::FLASH_FADE_HOLD
        && f.mode[2] == ta::FLASH_FADE_ID
}

impl World {
    /// Start the session's post-menu half for a menu that closed on `exit_code`
    /// (retail's value before the close's `+3`).
    ///
    /// The session is seeded where retail's is while the menu is up: parked
    /// on phase [`wmpa::FADE_FLASH_HOLD_PHASE`] with the level pinned at
    /// [`wmpa::BRIGHTNESS_MAX`], and the counter the close leaves
    /// (`exit_code + 3`, [`wmpa::FLASH_COUNTER_DONE_STEP`]). A second use while
    /// one is in flight is ignored.
    pub fn begin_pause_session_exit(&mut self, exit_code: u32, target: PauseTravelTarget) {
        if self.menu.pause_session.is_some() {
            log::warn!("pause session: a travel art is already in flight; use ignored");
            return;
        }
        self.menu.pause_session = Some(PauseSession {
            handler_id: HANDLER_PAUSE_SESSION,
            stage: PauseSessionStage::Session {
                phase: wmpa::FADE_FLASH_HOLD_PHASE,
                level: wmpa::BRIGHTNESS_MAX,
                counter: exit_code as i32 + wmpa::FLASH_COUNTER_DONE_STEP,
            },
            target,
        });
    }

    /// `true` while a Door item's session or travel art is running.
    pub fn pause_session_active(&self) -> bool {
        self.menu.pause_session.is_some()
    }

    /// One frame of the pause-menu session: the ramp-down to its terminal
    /// arm, then the travel art that arm installs.
    ///
    /// The session half is `FUN_801ED308`'s phases `3..7`
    /// ([`fade_flash_tick`]); its terminal [`FadeFlashEffect::Exit`] names the
    /// next handler, and the two ids [`TravelArt::for_handler_id`] accepts
    /// install the art. The art half applies every output the world-map binding
    /// applies ([`World::apply_travel_art_outputs`]) plus the phase-exit fade,
    /// and on the resolve frame issues the named scene transition the scene
    /// host drains ([`World::pending_named_scene_transition`]).
    ///
    /// REF: FUN_801ED308 (ported as `fade_flash_tick`), FUN_801EE094,
    /// FUN_801EE328 (ported as `TravelArtActor::tick`)
    pub fn tick_pause_session(&mut self) {
        let Some(mut session) = self.menu.pause_session.take() else {
            return;
        };
        let frame_delta = self.clock.frame_step;
        match session.stage {
            PauseSessionStage::Session {
                phase,
                level,
                counter,
            } => {
                let (phase, level, counter, effects) = fade_flash_tick(
                    phase,
                    FadeFlashInput {
                        frame_delta: i32::from(frame_delta),
                        level,
                        flash_counter: counter,
                        handler_id: session.handler_id,
                    },
                );
                session.stage = PauseSessionStage::Session {
                    phase,
                    level,
                    counter,
                };
                for e in effects {
                    match e {
                        FadeFlashEffect::Exit(exit) => {
                            session.handler_id = exit.next_handler;
                            match TravelArt::for_handler_id(exit.next_handler) {
                                Some(art) => {
                                    log::info!(
                                        "pause session: handler 0x{:02X} -> {art:?}",
                                        exit.next_handler
                                    );
                                    let y = self.player_world_y().unwrap_or(0);
                                    session.stage = PauseSessionStage::Art(
                                        TravelArtActor::new(art).with_player_y(y),
                                    );
                                }
                                None => {
                                    log::info!(
                                        "pause session: handler 0x{:02X} is not a travel art",
                                        exit.next_handler
                                    );
                                    return;
                                }
                            }
                        }
                        // Phase 5, the ordinary close: the session parks
                        // until the scene manager releases it.
                        FadeFlashEffect::ClearSceneField3E => return,
                        _ => {}
                    }
                }
                self.menu.pause_session = Some(session);
            }
            PauseSessionStage::Art(mut actor) => {
                if let Some(y) = self.player_world_y() {
                    actor.player_y = y;
                }
                let effect_busy =
                    self.system_flag_test(u16::from(crate::field_actor_program::FLAG_PLAYER_BUSY));
                let target = session.target.clone();
                let out = actor.tick(effect_busy, i16::from(frame_delta), || {
                    Some(ta::destination_for(
                        0,
                        i32::from(target.tile_x),
                        i32::from(target.tile_z),
                    ))
                });
                self.apply_travel_art_outputs(
                    out.queue_effect.map(|p| p as u16),
                    out.clear_warp_hold,
                    out.lift_player_y,
                    out.restore_player,
                );
                if out.spawn_flash {
                    crate::fade::spawn_fade(
                        &mut self.presentation.fade,
                        &travel_fade_template(),
                        ta::FLASH_FADE_ID,
                    );
                }
                if out.unfound {
                    log::warn!("pause session: travel art found no destination");
                    return;
                }
                if out.destination.is_some() {
                    log::info!(
                        "pause session: travel art warps to {} ({}, {})",
                        target.scene,
                        target.tile_x,
                        target.tile_z
                    );
                    self.pending_named_scene_transition =
                        Some((target.scene, target.tile_x, target.tile_z, 0));
                    session.stage = PauseSessionStage::Arriving;
                } else {
                    session.stage = PauseSessionStage::Art(actor);
                }
                self.menu.pause_session = Some(session);
            }
            PauseSessionStage::Arriving => {
                if self.pending_named_scene_transition.is_some() {
                    self.menu.pause_session = Some(session);
                    return;
                }
                // Retail's fade actor goes with the old scene's actor list;
                // the engine's one fade seat has to be emptied by hand.
                if self.presentation.fade.as_ref().is_some_and(is_travel_fade) {
                    self.presentation.fade = None;
                }
            }
        }
    }

    fn player_world_y(&self) -> Option<i16> {
        self.player_actor_slot
            .and_then(|slot| self.actors.get(slot as usize))
            .map(|a| a.move_state.world_y)
    }
}
